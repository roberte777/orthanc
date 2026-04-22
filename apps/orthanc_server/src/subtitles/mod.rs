//! Subtitle handling: language parsing, sidecar discovery, extraction,
//! caching, and time-shifting for HLS offset alignment.

pub mod classify;
pub mod cleanup;
pub mod languages;
pub mod security;
pub mod vtt;

pub use classify::{DeliveryMethod, classify};

use crate::models::media_stream::MediaStream;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum SubtitleError {
    #[error("stream not found")]
    StreamNotFound,
    #[error("delivery not supported as WebVTT (bitmap or unknown); burn-in required")]
    DeliveryUnsupported,
    #[error("external subtitle path '{0}' is not inside any library root")]
    PathOutsideLibrary(String),
    #[error("i/o error: {0}")]
    Io(String),
    #[error("ffmpeg extraction failed: {0}")]
    Ffmpeg(String),
    #[error("cache file missing or empty after extraction")]
    CacheEmpty,
}

impl From<std::io::Error> for SubtitleError {
    fn from(e: std::io::Error) -> Self {
        SubtitleError::Io(e.to_string())
    }
}

/// Central manager for subtitle extraction, caching, and offset serving.
pub struct SubtitleManager {
    cache_dir: PathBuf,
    ffmpeg_path: String,
    library_roots: Vec<PathBuf>,
    /// Per-stream locks to dedupe concurrent extractions.
    extract_locks: Mutex<HashMap<i64, Arc<Mutex<()>>>>,
}

impl SubtitleManager {
    pub fn new(cache_dir: PathBuf, ffmpeg_path: String, library_roots: Vec<PathBuf>) -> Self {
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            tracing::warn!("Failed to create subtitle cache dir {:?}: {}", cache_dir, e);
        }
        Self {
            cache_dir,
            ffmpeg_path,
            library_roots,
            extract_locks: Mutex::new(HashMap::new()),
        }
    }

    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }

    /// Path to the cached WebVTT file for a given stream id.
    pub fn cache_path(&self, stream_id: i64) -> PathBuf {
        self.cache_dir.join(format!("{}.vtt", stream_id))
    }

    /// Replace the configured library roots (called on library path mutations).
    pub async fn set_library_roots(&self, _roots: Vec<PathBuf>) {
        // Kept for symmetry — we currently snapshot roots at startup. If the
        // runtime needs live updates we'd wrap `library_roots` in a RwLock.
    }

    /// Extract the given subtitle stream to WebVTT, caching the result.
    /// Returns the path to the cached `.vtt` file.
    ///
    /// Concurrent callers for the same stream_id are deduplicated on a
    /// per-stream Mutex. If the cache already holds a nonempty file, we
    /// skip re-extraction.
    pub async fn extract_vtt(
        &self,
        stream: &MediaStream,
        source_video_path: &str,
    ) -> Result<PathBuf, SubtitleError> {
        if classify(stream) != DeliveryMethod::Vtt {
            return Err(SubtitleError::DeliveryUnsupported);
        }

        let cache_path = self.cache_path(stream.id);

        // Fast path: cache hit.
        if let Ok(meta) = tokio::fs::metadata(&cache_path).await
            && meta.len() > 0 {
                return Ok(cache_path);
            }

        // Obtain the per-stream lock.
        let lock = {
            let mut map = self.extract_locks.lock().await;
            map.entry(stream.id)
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = lock.lock().await;

        // Re-check cache under the lock.
        if let Ok(meta) = tokio::fs::metadata(&cache_path).await
            && meta.len() > 0 {
                return Ok(cache_path);
            }

        // Determine extraction source
        let (input_path, map_arg): (PathBuf, Option<String>) = if stream.is_external {
            let external = stream
                .external_file_path
                .as_deref()
                .ok_or(SubtitleError::StreamNotFound)?;
            let validated = security::validate_under_roots(external, &self.library_roots)
                .map_err(|_| SubtitleError::PathOutsideLibrary(external.to_string()))?;
            (validated, None)
        } else {
            let src = Path::new(source_video_path);
            if !src.exists() {
                return Err(SubtitleError::StreamNotFound);
            }
            (
                src.to_path_buf(),
                Some(format!("0:{}", stream.stream_index)),
            )
        };

        // Extract via ffmpeg to a tmp file, then atomically rename.
        let tmp_path = self
            .cache_dir
            .join(format!("{}.{}.tmp.vtt", stream.id, random_suffix()));

        let mut cmd = tokio::process::Command::new(&self.ffmpeg_path);
        cmd.arg("-y").arg("-nostdin").arg("-i").arg(&input_path);
        if let Some(m) = &map_arg {
            cmd.arg("-map").arg(m);
        }
        cmd.arg("-c:s")
            .arg("webvtt")
            .arg("-f")
            .arg("webvtt")
            .arg(&tmp_path);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let output = cmd
            .output()
            .await
            .map_err(|e| SubtitleError::Ffmpeg(e.to_string()))?;
        if !output.status.success() {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(SubtitleError::Ffmpeg(stderr));
        }

        let meta = tokio::fs::metadata(&tmp_path)
            .await
            .map_err(SubtitleError::from)?;
        if meta.len() == 0 {
            let _ = tokio::fs::remove_file(&tmp_path).await;
            return Err(SubtitleError::CacheEmpty);
        }

        tokio::fs::rename(&tmp_path, &cache_path)
            .await
            .map_err(SubtitleError::from)?;
        Ok(cache_path)
    }

    /// Return a WebVTT document with every cue shifted by `-offset_seconds`.
    /// The source stream must already be extracted via `extract_vtt`.
    pub async fn vtt_with_offset(
        &self,
        stream: &MediaStream,
        source_video_path: &str,
        offset_seconds: f64,
    ) -> Result<String, SubtitleError> {
        let path = self.extract_vtt(stream, source_video_path).await?;
        let bytes = tokio::fs::read(&path).await.map_err(SubtitleError::from)?;
        let text = String::from_utf8_lossy(&bytes).to_string();
        Ok(vtt::shift_vtt(&text, offset_seconds))
    }

    /// Prepare a burn-in source: symlink (or copy as fallback) the subtitle
    /// file into `dest_dir` with a filter-safe filename.
    ///
    /// For embedded subtitles, the source is the video file itself (FFmpeg
    /// reads the stream via `:si=N` on the filter).
    pub async fn prepare_for_burn(
        &self,
        stream: &MediaStream,
        source_video_path: &str,
        dest_dir: &Path,
    ) -> Result<PathBuf, SubtitleError> {
        tokio::fs::create_dir_all(dest_dir).await?;
        let src_path: PathBuf = if stream.is_external {
            let ext = stream
                .external_file_path
                .as_deref()
                .ok_or(SubtitleError::StreamNotFound)?;
            security::validate_under_roots(ext, &self.library_roots)
                .map_err(|_| SubtitleError::PathOutsideLibrary(ext.to_string()))?
        } else {
            PathBuf::from(source_video_path)
        };

        let ext = src_path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("dat")
            .to_string();

        let dest = dest_dir.join(format!("burn_src.{}", ext));
        // Remove any pre-existing file from a prior attempt.
        let _ = tokio::fs::remove_file(&dest).await;

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            if symlink(&src_path, &dest).is_ok() {
                return Ok(dest);
            }
        }
        // Fallback: copy. Required on platforms without symlink perms.
        tokio::fs::copy(&src_path, &dest)
            .await
            .map_err(SubtitleError::from)?;
        Ok(dest)
    }

    /// Remove a cached VTT file (used when a media_stream is deleted).
    pub async fn remove_cache(&self, stream_id: i64) {
        let _ = tokio::fs::remove_file(self.cache_path(stream_id)).await;
    }
}

fn random_suffix() -> String {
    use rand::RngExt;
    let bytes: [u8; 8] = std::array::from_fn(|_| rand::rng().random::<u8>());
    hex::encode(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::media_stream::MediaStream;
    use tempfile::tempdir;

    fn make_stream(
        id: i64,
        codec: &str,
        is_external: bool,
        external_path: Option<&str>,
    ) -> MediaStream {
        MediaStream {
            id,
            media_item_id: 1,
            stream_index: 2,
            stream_type: "subtitle".into(),
            codec: Some(codec.into()),
            language: None,
            title: None,
            is_default: false,
            is_forced: false,
            width: None,
            height: None,
            aspect_ratio: None,
            frame_rate: None,
            bit_depth: None,
            color_space: None,
            channels: None,
            sample_rate: None,
            bit_rate: None,
            is_external,
            external_file_path: external_path.map(str::to_string),
        }
    }

    #[tokio::test]
    async fn rejects_non_vtt_stream() {
        let cache = tempdir().unwrap();
        let mgr = SubtitleManager::new(cache.path().to_path_buf(), "ffmpeg".into(), vec![]);
        let s = make_stream(1, "hdmv_pgs_subtitle", false, None);
        let res = mgr.extract_vtt(&s, "/does/not/matter").await;
        assert!(matches!(res, Err(SubtitleError::DeliveryUnsupported)));
    }

    #[tokio::test]
    async fn rejects_external_outside_root() {
        let cache = tempdir().unwrap();
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let bad_path = outside.path().join("evil.srt");
        std::fs::write(&bad_path, b"WEBVTT\n").unwrap();

        let mgr = SubtitleManager::new(
            cache.path().to_path_buf(),
            "ffmpeg".into(),
            vec![root.path().to_path_buf()],
        );
        let s = make_stream(42, "subrip", true, bad_path.to_str());
        let res = mgr.extract_vtt(&s, "/ignored").await;
        match res {
            Err(SubtitleError::PathOutsideLibrary(_)) => {}
            other => panic!("expected PathOutsideLibrary, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn cache_hit_skips_extraction() {
        let cache = tempdir().unwrap();
        let mgr = SubtitleManager::new(cache.path().to_path_buf(), "ffmpeg".into(), vec![]);
        // Pre-seed cache
        let path = mgr.cache_path(100);
        std::fs::write(&path, b"WEBVTT\n\n1\n00:00:01.000 --> 00:00:02.000\nhi\n").unwrap();

        let s = make_stream(100, "subrip", false, None);
        let got = mgr.extract_vtt(&s, "/bogus/video.mkv").await.unwrap();
        assert_eq!(got, path);
    }

    #[tokio::test]
    async fn prepare_for_burn_external_symlinks_inside_dest() {
        let cache = tempdir().unwrap();
        let root = tempdir().unwrap();
        let srt = root.path().join("sub.srt");
        std::fs::write(&srt, b"1\n00:00:01,000 --> 00:00:02,000\nhi\n").unwrap();

        let mgr = SubtitleManager::new(
            cache.path().to_path_buf(),
            "ffmpeg".into(),
            vec![root.path().to_path_buf()],
        );
        let s = make_stream(5, "subrip", true, srt.to_str());
        let dest_dir = cache.path().join("burn_session");
        let dest = mgr
            .prepare_for_burn(&s, "/ignored", &dest_dir)
            .await
            .unwrap();
        assert!(dest.starts_with(&dest_dir));
        assert!(dest.exists());
        // File name must have no filter-unsafe chars even if source path did.
        let name = dest.file_name().unwrap().to_string_lossy();
        for ch in [':', ',', '\'', '\\', ' ', '['] {
            assert!(!name.contains(ch), "name has unsafe char {}: {}", ch, name);
        }
    }

    #[tokio::test]
    async fn remove_cache_deletes_file_if_present() {
        let cache = tempdir().unwrap();
        let mgr = SubtitleManager::new(cache.path().to_path_buf(), "ffmpeg".into(), vec![]);
        let p = mgr.cache_path(77);
        std::fs::write(&p, b"WEBVTT\n").unwrap();
        assert!(p.exists());
        mgr.remove_cache(77).await;
        assert!(!p.exists());
    }

    #[tokio::test]
    async fn remove_cache_missing_is_noop() {
        let cache = tempdir().unwrap();
        let mgr = SubtitleManager::new(cache.path().to_path_buf(), "ffmpeg".into(), vec![]);
        mgr.remove_cache(12345).await;
    }

    #[tokio::test]
    async fn vtt_with_offset_shifts_and_drops_cues() {
        let cache = tempdir().unwrap();
        let mgr = SubtitleManager::new(cache.path().to_path_buf(), "ffmpeg".into(), vec![]);
        let sid = 200i64;
        let path = mgr.cache_path(sid);
        std::fs::write(
            &path,
            b"WEBVTT\n\n1\n00:00:01.000 --> 00:00:02.000\nfirst\n\n2\n00:00:10.000 --> 00:00:12.000\nsecond\n",
        )
        .unwrap();

        let s = make_stream(sid, "subrip", false, None);
        let out = mgr.vtt_with_offset(&s, "/video.mkv", 5.0).await.unwrap();
        assert!(out.starts_with("WEBVTT"));
        assert!(
            !out.contains("first"),
            "cue ending at 2 should be dropped when offset=5"
        );
        assert!(out.contains("second"));
        assert!(out.contains("00:00:05.000 --> 00:00:07.000"));
    }
}
