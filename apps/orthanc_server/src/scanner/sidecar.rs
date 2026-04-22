//! External subtitle sidecar discovery.
//!
//! For a given video file, find adjacent subtitle files (same directory plus
//! one level of `Subs/` or `Subtitles/`) whose filename stems match the video's
//! stem. Parses language / forced / SDH flags from the filename suffix.

use crate::subtitles::languages::Suffix;
use std::path::{Path, PathBuf};

/// Subtitle file formats we recognize as sidecars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidecarFormat {
    /// SubRip (.srt) — text, convertible to WebVTT
    Subrip,
    /// WebVTT (.vtt) — text, native
    WebVtt,
    /// SubStation Alpha (.ass/.ssa) — text with styling
    Ass,
    /// VobSub — bitmap (.idx + .sub pair)
    VobSub,
    /// PGS / Blu-ray bitmap (.sup)
    Pgs,
}

impl SidecarFormat {
    /// Codec string to store in `media_streams.codec`.
    pub fn codec(&self) -> &'static str {
        match self {
            SidecarFormat::Subrip => "subrip",
            SidecarFormat::WebVtt => "webvtt",
            SidecarFormat::Ass => "ass",
            SidecarFormat::VobSub => "vobsub",
            SidecarFormat::Pgs => "hdmv_pgs_subtitle",
        }
    }

    /// Whether this format can be converted to WebVTT (vs. requiring burn-in).
    pub fn is_text(&self) -> bool {
        matches!(
            self,
            SidecarFormat::Subrip | SidecarFormat::WebVtt | SidecarFormat::Ass
        )
    }

    fn from_ext(ext: &str) -> Option<Self> {
        match ext.to_lowercase().as_str() {
            "srt" => Some(Self::Subrip),
            "vtt" => Some(Self::WebVtt),
            "ass" | "ssa" => Some(Self::Ass),
            "sup" => Some(Self::Pgs),
            _ => None,
        }
    }
}

/// A discovered sidecar subtitle file.
#[derive(Debug, Clone)]
pub struct SidecarInfo {
    pub path: PathBuf,
    pub format: SidecarFormat,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    pub is_sdh: bool,
}

/// Discover sidecar subtitle files adjacent to `video_path`.
///
/// Searches:
///   1. The video's parent directory.
///   2. `Subs/` and `Subtitles/` siblings of the parent (one level deep).
///
/// Returns entries sorted by path for deterministic ordering.
pub fn discover_sidecars(video_path: &Path) -> Vec<SidecarInfo> {
    let Some(parent) = video_path.parent() else {
        return Vec::new();
    };
    let Some(video_stem) = video_path.file_stem().and_then(|s| s.to_str()) else {
        return Vec::new();
    };

    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut scanned_dirs: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
    let mut scan =
        |dir: &Path, out: &mut Vec<PathBuf>, seen: &mut std::collections::HashSet<PathBuf>| {
            let key = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
            if seen.insert(key) {
                collect_files(dir, out);
            }
        };
    scan(parent, &mut candidates, &mut scanned_dirs);
    for sub in &["Subs", "Subtitles", "subs", "subtitles"] {
        let sub_dir = parent.join(sub);
        if sub_dir.is_dir() {
            scan(&sub_dir, &mut candidates, &mut scanned_dirs);
        }
    }

    let mut found: Vec<SidecarInfo> = Vec::new();
    let mut vobsub_seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();

    for path in &candidates {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };

        // `stem` must start with `video_stem` (otherwise unrelated file).
        let Some(suffix_raw) = stem.strip_prefix(video_stem) else {
            continue;
        };
        // Either exact match (no suffix) or suffix begins with '.'
        let suffix = if suffix_raw.is_empty() {
            ""
        } else if let Some(s) = suffix_raw.strip_prefix('.') {
            s
        } else {
            // Prefix collision like "movie2.en.srt" matching stem "movie"
            continue;
        };

        // VobSub: `.idx` is the "primary" file; the matching `.sub` must exist as sibling.
        let ext_lower = ext.to_lowercase();
        if ext_lower == "idx" {
            let sub_path = path.with_extension("sub");
            if sub_path.is_file() {
                let suf = Suffix::parse(suffix);
                found.push(make_info(path.clone(), SidecarFormat::VobSub, suf));
                vobsub_seen.insert(sub_path);
            }
            continue;
        }

        // Skip `.sub` files whose `.idx` sibling we already indexed
        if ext_lower == "sub" && vobsub_seen.contains(path) {
            continue;
        }
        // Also skip orphan `.sub` (no paired `.idx`) — raw MicroDVD is rare and we'd misclassify
        if ext_lower == "sub" {
            continue;
        }

        let Some(format) = SidecarFormat::from_ext(&ext_lower) else {
            continue;
        };

        let suf = Suffix::parse(suffix);
        found.push(make_info(path.clone(), format, suf));
    }

    found.sort_by(|a, b| a.path.cmp(&b.path));
    found
}

fn make_info(path: PathBuf, format: SidecarFormat, suf: Suffix) -> SidecarInfo {
    SidecarInfo {
        path,
        format,
        language: suf.language.clone(),
        title: suf.title(),
        is_default: suf.is_default,
        is_forced: suf.is_forced,
        is_sdh: suf.is_sdh,
    }
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            out.push(path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"").unwrap();
    }

    #[test]
    fn finds_plain_srt_next_to_video() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("movie.srt"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].format, SidecarFormat::Subrip);
        assert!(results[0].language.is_none());
        assert!(!results[0].is_forced);
    }

    #[test]
    fn parses_language_from_filename() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("movie.en.srt"));
        touch(&dir.path().join("movie.es.srt"));
        touch(&dir.path().join("movie.fre.srt"));

        let results = discover_sidecars(&video);
        let langs: Vec<Option<String>> = results.iter().map(|s| s.language.clone()).collect();
        assert!(langs.contains(&Some("en".into())));
        assert!(langs.contains(&Some("es".into())));
        assert!(langs.contains(&Some("fr".into())));
    }

    #[test]
    fn parses_forced_flag() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("movie.forced.srt"));
        touch(&dir.path().join("movie.en.forced.srt"));
        touch(&dir.path().join("movie.forced.en.srt"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.is_forced, "expected forced for {:?}", r.path);
        }
        let with_lang: Vec<_> = results.iter().filter(|r| r.language.is_some()).collect();
        assert_eq!(with_lang.len(), 2);
    }

    #[test]
    fn parses_sdh_flag() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("movie.en.sdh.srt"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 1);
        assert!(results[0].is_sdh);
        assert_eq!(results[0].language.as_deref(), Some("en"));
    }

    #[test]
    fn ignores_unrelated_files() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("other.srt"));
        touch(&dir.path().join("movie2.en.srt"));
        touch(&dir.path().join("readme.txt"));

        let results = discover_sidecars(&video);
        assert!(results.is_empty());
    }

    #[test]
    fn finds_subs_subdirectory() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("Subs/movie.en.srt"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].language.as_deref(), Some("en"));
        assert!(results[0].path.to_string_lossy().contains("Subs"));
    }

    #[test]
    fn finds_subtitles_subdirectory_case_insensitive() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("subtitles/movie.fr.srt"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].language.as_deref(), Some("fr"));
    }

    #[test]
    fn ignores_nested_deeper_than_one_level() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("Subs/nested/movie.en.srt"));

        let results = discover_sidecars(&video);
        assert!(results.is_empty());
    }

    #[test]
    fn collapses_vobsub_pair() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("movie.en.idx"));
        touch(&dir.path().join("movie.en.sub"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].format, SidecarFormat::VobSub);
        assert!(results[0].path.to_string_lossy().ends_with(".idx"));
    }

    #[test]
    fn orphan_sub_ignored() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        // .sub with no .idx sibling
        touch(&dir.path().join("movie.en.sub"));

        let results = discover_sidecars(&video);
        assert!(results.is_empty());
    }

    #[test]
    fn recognizes_all_text_formats() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("movie.en.srt"));
        touch(&dir.path().join("movie.en.vtt"));
        touch(&dir.path().join("movie.en.ass"));
        touch(&dir.path().join("movie.en.ssa"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 4);
        let formats: Vec<_> = results.iter().map(|r| r.format).collect();
        assert!(formats.contains(&SidecarFormat::Subrip));
        assert!(formats.contains(&SidecarFormat::WebVtt));
        assert!(formats.contains(&SidecarFormat::Ass));
    }

    #[test]
    fn recognizes_pgs_sup_as_bitmap() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("movie.en.sup"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].format, SidecarFormat::Pgs);
        assert!(!results[0].format.is_text());
    }

    #[test]
    fn title_is_derived_from_suffix() {
        let dir = tempdir().unwrap();
        let video = dir.path().join("movie.mkv");
        touch(&video);
        touch(&dir.path().join("movie.en.forced.srt"));

        let results = discover_sidecars(&video);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title.as_deref(), Some("English · Forced"));
    }
}
