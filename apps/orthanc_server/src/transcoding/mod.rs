use crate::models::media_stream::MediaStream;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{watch, RwLock};
use tracing::{debug, info, warn};

// ---------------------------------------------------------------------------
// Transcode mode decision
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TranscodeMode {
    Direct,
    Remux,
    AudioTranscode,
    FullTranscode,
}

impl TranscodeMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            TranscodeMode::Direct => "direct",
            TranscodeMode::Remux => "remux",
            TranscodeMode::AudioTranscode => "audio_transcode",
            TranscodeMode::FullTranscode => "full_transcode",
        }
    }
}

/// Client-reported codec/container support, probed via MediaSource.isTypeSupported().
pub struct ClientCapabilities {
    pub video_codecs: Vec<String>,
    pub audio_codecs: Vec<String>,
    pub containers: Vec<String>,
}

/// Fallback lists used when the client sends empty capabilities.
const DEFAULT_VIDEO_CODECS: &[&str] = &["h264", "avc1", "avc"];
const DEFAULT_AUDIO_CODECS: &[&str] = &["aac", "opus", "mp3"];
const DEFAULT_CONTAINERS: &[&str] = &["mp4", "m4v", "webm", "mov"];

/// Decide the transcode mode based on media streams, container, and client capabilities.
///
/// `selected_audio` is the audio stream the client requested (or the default if omitted).
/// When it's not the file's first audio stream, Direct is no longer possible because
/// HTML5 `<video>` has no reliable audio-track-selection API — we must at least Remux
/// so the HLS output carries only the selected track.
///
/// If `burn_requested` is true, returns `FullTranscode` unconditionally — burning
/// a subtitle into the video stream requires a full re-encode.
///
/// If `normalize_requested` is true, the minimum mode becomes `AudioTranscode` —
/// the loudnorm filter only runs during a real audio re-encode.
pub fn decide_transcode_mode(
    streams: &[MediaStream],
    container_format: Option<&str>,
    client: &ClientCapabilities,
    selected_audio: Option<&MediaStream>,
    burn_requested: bool,
    normalize_requested: bool,
) -> TranscodeMode {
    if burn_requested {
        return TranscodeMode::FullTranscode;
    }

    let video = streams.iter().find(|s| s.stream_type == "video");
    let first_audio = streams.iter().find(|s| s.stream_type == "audio");
    let audio = selected_audio.or(first_audio);

    // No video stream — probably audio file, serve directly
    let Some(video) = video else {
        if normalize_requested {
            return TranscodeMode::AudioTranscode;
        }
        return TranscodeMode::Direct;
    };

    let video_ok = video
        .codec
        .as_deref()
        .map(|codec| {
            if client.video_codecs.is_empty() {
                DEFAULT_VIDEO_CODECS.contains(&codec)
            } else {
                client.video_codecs.iter().any(|c| c == codec)
            }
        })
        .unwrap_or(false);

    let audio_ok = audio
        .map(|a| {
            a.codec
                .as_deref()
                .map(|codec| {
                    if client.audio_codecs.is_empty() {
                        DEFAULT_AUDIO_CODECS.contains(&codec)
                    } else {
                        client.audio_codecs.iter().any(|c| c == codec)
                    }
                })
                .unwrap_or(false)
        })
        .unwrap_or(true); // No audio = fine

    let container_ok = container_format
        .map(|fmt| {
            let fmt_lower = fmt.to_lowercase();
            if client.containers.is_empty() {
                DEFAULT_CONTAINERS.contains(&fmt_lower.as_str())
            } else {
                client.containers.iter().any(|c| c == &fmt_lower)
            }
        })
        .unwrap_or(false);

    // Non-default audio track forces at least Remux (see function docs).
    let non_default_audio = match (selected_audio, first_audio) {
        (Some(sel), Some(first)) => sel.id != first.id,
        _ => false,
    };

    let base = if video_ok && audio_ok && container_ok && !non_default_audio {
        TranscodeMode::Direct
    } else if video_ok && audio_ok {
        TranscodeMode::Remux
    } else if video_ok && !audio_ok {
        TranscodeMode::AudioTranscode
    } else {
        TranscodeMode::FullTranscode
    };

    if normalize_requested {
        match base {
            TranscodeMode::Direct | TranscodeMode::Remux => TranscodeMode::AudioTranscode,
            other => other,
        }
    } else {
        base
    }
}

// ---------------------------------------------------------------------------
// Transcode session
// ---------------------------------------------------------------------------

/// Describes a subtitle stream selected for burn-in (hardcoded overlay).
#[derive(Debug, Clone)]
pub struct BurnSubtitle {
    /// `media_streams.id`
    pub stream_id: i64,
    /// For embedded subtitles: the original ffprobe stream_index to pass to
    /// FFmpeg's `subtitles=...:si=N` filter. `None` for external files.
    pub stream_index: Option<i32>,
    /// Absolute filesystem path for external subtitles; ignored for embedded.
    pub external_file_path: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_forced: bool,
    pub is_external: bool,
}

/// Resolved filesystem paths used by the FFmpeg invocation for a burn-in.
#[derive(Debug, Clone)]
struct BurnResolved {
    /// The sanitized (symlinked/copied) source path FFmpeg should read.
    sanitized_source: PathBuf,
    /// For embedded: the stream index to pass as `si=N`. None for external.
    stream_index: Option<i32>,
}

pub struct TranscodeSession {
    pub session_id: String,
    pub user_id: i64,
    pub media_item_id: i64,
    pub output_dir: PathBuf,
    pub mode: TranscodeMode,
    pub file_path: String,
    pub video_height: Option<i32>,
    pub start_time: RwLock<f64>,
    /// True PTS of the first HLS segment, captured after FFmpeg produces it.
    /// Clients use this as the subtitle offset; it may differ from
    /// `start_time` by up to one GOP because of `-ss`-before-input keyframe alignment.
    pub actual_start_time: RwLock<f64>,
    /// Subtitle to burn into the video stream (FullTranscode only). Clonable so
    /// it survives seek restarts.
    pub burn_subtitle: Option<BurnSubtitle>,
    /// Resolved burn paths (symlinked into output_dir). Cleaned up when the
    /// session's output_dir is removed.
    burn_resolved: RwLock<Option<BurnResolved>>,
    /// Absolute ffprobe `stream_index` of the audio track to map into the output.
    /// `None` means "first audio stream" — FFmpeg uses `0:a:0?` as a safe fallback.
    pub audio_stream_index: Option<i32>,
    /// If true, append `loudnorm` to the audio filter chain. Only effective
    /// in AudioTranscode / FullTranscode modes.
    pub audio_normalize: bool,
    /// DB id of the selected audio stream, for admin observability and echoing
    /// back to the client.
    pub audio_stream_id: Option<i64>,
    ffmpeg_child: RwLock<Option<tokio::process::Child>>,
    pub last_accessed: RwLock<Instant>,
    /// Sends `true` when the first .ts segment is ready. Reset to `false` on seek.
    ready_tx: watch::Sender<bool>,
    ready_rx: watch::Receiver<bool>,
}

impl TranscodeSession {
    /// Wait until the first HLS segment is ready, or timeout.
    pub async fn wait_until_ready(&self, timeout: Duration) -> bool {
        let mut rx = self.ready_rx.clone();
        tokio::time::timeout(timeout, async {
            loop {
                if *rx.borrow_and_update() {
                    return true;
                }
                if rx.changed().await.is_err() {
                    return false;
                }
            }
        })
        .await
        .unwrap_or(false)
    }
}

/// Snapshot of a live transcode session for admin observability.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActiveSessionInfo {
    pub session_id: String,
    pub user_id: i64,
    pub media_item_id: i64,
    pub mode: &'static str,
    pub video_height: Option<i32>,
    pub file_path: String,
    pub start_time_seconds: f64,
    pub idle_seconds: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub burned_subtitle: Option<BurnedSubtitleDisplay>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_stream_id: Option<i64>,
    pub audio_normalize: bool,
}

/// Admin-visible description of a subtitle being burned into a session's video.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BurnedSubtitleDisplay {
    pub stream_id: i64,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_forced: bool,
    pub is_external: bool,
}

// ---------------------------------------------------------------------------
// Session manager
// ---------------------------------------------------------------------------

pub struct TranscodeSessionManager {
    sessions: RwLock<HashMap<String, Arc<TranscodeSession>>>,
    cache_dir: PathBuf,
    ffmpeg_path: String,
    max_concurrent: usize,
}

impl TranscodeSessionManager {
    pub fn new(cache_dir: PathBuf, ffmpeg_path: String, max_concurrent: usize) -> Self {
        // Any pre-existing contents are orphaned sessions from a previous process —
        // transcode sessions live only in memory, so nothing here can still be ours.
        if cache_dir.exists() {
            info!("Clearing stale transcode cache at {:?} (may take a while)", cache_dir);
            let start = Instant::now();
            // remove_dir_all can race with the OS on large trees (macOS APFS,
            // Spotlight, fseventsd) and return ENOTEMPTY. Retry a few times.
            let mut last_err = None;
            for attempt in 1..=5 {
                match std::fs::remove_dir_all(&cache_dir) {
                    Ok(()) => {
                        info!(
                            "Transcode cache cleared in {:.1}s (attempt {})",
                            start.elapsed().as_secs_f64(),
                            attempt
                        );
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        if !cache_dir.exists() {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(250));
                    }
                }
            }
            if let Some(e) = last_err {
                warn!("Failed to clear stale transcode cache at {:?}: {}", cache_dir, e);
            }
        }
        if let Err(e) = std::fs::create_dir_all(&cache_dir) {
            warn!("Failed to create transcode cache dir {:?}: {}", cache_dir, e);
        }

        Self {
            sessions: RwLock::new(HashMap::new()),
            cache_dir,
            ffmpeg_path,
            max_concurrent,
        }
    }

    /// Start a new transcode session. Spawns FFmpeg and returns once the process is running.
    /// The caller should await `session.ready.notified()` before serving the HLS URL.
    pub async fn start_session(
        &self,
        user_id: i64,
        media_item_id: i64,
        file_path: &str,
        mode: TranscodeMode,
        video_stream: Option<&MediaStream>,
        audio_stream: Option<&MediaStream>,
        start_time: f64,
        burn_subtitle: Option<BurnSubtitle>,
        audio_normalize: bool,
    ) -> Result<Arc<TranscodeSession>, anyhow::Error> {
        // Check global concurrent limit
        {
            let sessions = self.sessions.read().await;
            if sessions.len() >= self.max_concurrent {
                anyhow::bail!(
                    "Maximum concurrent transcodes ({}) exceeded",
                    self.max_concurrent
                );
            }
        }

        let session_id = uuid_v4();
        let output_dir = self.cache_dir.join(&session_id);
        let video_height = video_stream.and_then(|v| v.height);
        let audio_stream_index = audio_stream.map(|s| s.stream_index);
        let audio_stream_id = audio_stream.map(|s| s.id);

        tokio::fs::create_dir_all(&output_dir).await?;

        // Resolve burn-in source into a sanitized path inside the session dir
        // (avoids all FFmpeg filter-string escaping pitfalls).
        let burn_resolved = if let Some(ref burn) = burn_subtitle {
            Some(resolve_burn_source(burn, file_path, &output_dir).await?)
        } else {
            None
        };

        let child = Self::spawn_ffmpeg(
            &self.ffmpeg_path,
            file_path,
            &output_dir,
            mode,
            video_height,
            start_time,
            burn_resolved.as_ref(),
            audio_stream_index,
            audio_normalize,
        )
        .await?;

        let (ready_tx, ready_rx) = watch::channel(false);

        let session = Arc::new(TranscodeSession {
            session_id: session_id.clone(),
            user_id,
            media_item_id,
            output_dir: output_dir.clone(),
            mode,
            file_path: file_path.to_string(),
            video_height,
            start_time: RwLock::new(start_time),
            actual_start_time: RwLock::new(start_time),
            burn_subtitle,
            burn_resolved: RwLock::new(burn_resolved),
            audio_stream_index,
            audio_normalize,
            audio_stream_id,
            ffmpeg_child: RwLock::new(Some(child)),
            last_accessed: RwLock::new(Instant::now()),
            ready_tx,
            ready_rx,
        });

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session.clone());

        Self::watch_for_ready(session.clone(), output_dir, self.ffmpeg_path.clone());

        info!(
            "Started {:?} transcode session {} for media {} at {:.0}s",
            mode, session_id, media_item_id, start_time
        );
        Ok(session)
    }

    /// Restart an existing session from a new seek position.
    /// Kills the old FFmpeg, clears segments, and starts fresh.
    /// Returns the session so the caller can await readiness.
    pub async fn seek_session(
        &self,
        session_id: &str,
        seek_time: f64,
    ) -> Result<Arc<TranscodeSession>, anyhow::Error> {
        let session = {
            let sessions = self.sessions.read().await;
            sessions
                .get(session_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Session not found"))?
        };

        // Kill existing FFmpeg
        if let Some(mut child) = session.ffmpeg_child.write().await.take() {
            let _ = child.kill().await;
        }

        // Clear old segments and playlist
        if session.output_dir.exists() {
            let _ = tokio::fs::remove_dir_all(&session.output_dir).await;
        }

        // Reset readiness before spawning new FFmpeg
        let _ = session.ready_tx.send(false);

        // Re-resolve burn source (the old symlink is still in the output_dir
        // but may have been cleared by the segment wipe above).
        let burn_resolved = if let Some(ref burn) = session.burn_subtitle {
            Some(resolve_burn_source(burn, &session.file_path, &session.output_dir).await?)
        } else {
            None
        };

        // Spawn new FFmpeg from the seek position
        let child = Self::spawn_ffmpeg(
            &self.ffmpeg_path,
            &session.file_path,
            &session.output_dir,
            session.mode,
            session.video_height,
            seek_time,
            burn_resolved.as_ref(),
            session.audio_stream_index,
            session.audio_normalize,
        )
        .await?;

        *session.ffmpeg_child.write().await = Some(child);
        *session.start_time.write().await = seek_time;
        *session.actual_start_time.write().await = seek_time;
        *session.burn_resolved.write().await = burn_resolved;
        *session.last_accessed.write().await = Instant::now();

        Self::watch_for_ready(session.clone(), session.output_dir.clone(), self.ffmpeg_path.clone());

        info!(
            "Restarted transcode session {} seeking to {:.0}s",
            session_id, seek_time
        );
        Ok(session)
    }

    /// Spawn FFmpeg process for HLS output.
    async fn spawn_ffmpeg(
        ffmpeg_path: &str,
        file_path: &str,
        output_dir: &PathBuf,
        mode: TranscodeMode,
        video_height: Option<i32>,
        start_time: f64,
        burn: Option<&BurnResolved>,
        audio_stream_index: Option<i32>,
        audio_normalize: bool,
    ) -> Result<tokio::process::Child, anyhow::Error> {
        tokio::fs::create_dir_all(output_dir).await?;

        let segment_pattern = output_dir.join("seg_%04d.ts");
        let playlist_path = output_dir.join("stream.m3u8");

        let mut cmd = tokio::process::Command::new(ffmpeg_path);

        // Seek before input for fast seeking (uses keyframes)
        if start_time > 0.0 {
            cmd.args(["-ss", &format!("{:.3}", start_time)]);
        }

        // Regenerate PTS if the input is missing or inconsistent — helps hls.js
        // see clean timestamps and avoids the "audio starts late" effect that
        // comes from fast-seeking into a stream where video and audio packets
        // don't share a common origin.
        cmd.args(["-fflags", "+genpts"]);

        cmd.arg("-i").arg(file_path);
        cmd.args(["-y", "-nostdin"]);

        // Stream mapping: first video always; audio is the user-selected absolute
        // ffprobe index when provided, otherwise the file's first audio stream.
        // Subtitles are delivered separately as WebVTT; we never include them in
        // the HLS output, so no -map 0:s.
        let audio_map = match audio_stream_index {
            Some(idx) => format!("0:{}?", idx),
            None => "0:a:0?".to_string(),
        };
        cmd.args(["-map", "0:v:0?", "-map", audio_map.as_str()]);

        // Loudness normalization applies only when audio is being re-encoded.
        // Chained in front of aresample so timestamp fixup runs on the
        // post-normalized signal.
        let audio_filter = if audio_normalize {
            "loudnorm=I=-16:LRA=11:TP=-1.5,aresample=async=1:first_pts=0"
        } else {
            "aresample=async=1:first_pts=0"
        };

        match mode {
            TranscodeMode::Remux => {
                cmd.args(["-c", "copy"]);
            }
            TranscodeMode::AudioTranscode => {
                // -af aresample=async=1:first_pts=0 pads leading silence so
                // audio aligns with video from frame 0; without it the AAC
                // encoder's priming delay plus any gap between the video
                // keyframe and nearest audio packet produces a noticeable
                // "video starts, audio joins later" effect.
                cmd.args([
                    "-c:v", "copy",
                    "-c:a", "aac",
                    "-b:a", "192k",
                    "-ac", "2",
                    "-af", audio_filter,
                ]);
            }
            TranscodeMode::FullTranscode => {
                let height = video_height.map(|h| h.min(1080)).unwrap_or(1080);
                let bitrate = match height {
                    0..=480 => "2000k",
                    481..=720 => "4000k",
                    _ => "8000k",
                };
                let bufsize = match height {
                    0..=480 => "4000k",
                    481..=720 => "8000k",
                    _ => "16000k",
                };
                // Build -vf chain: subtitles filter first (if burning), then scale.
                let scale = format!("scale=-2:{}", height);
                let vf = if let Some(b) = burn {
                    let path_str = b.sanitized_source.to_string_lossy();
                    match b.stream_index {
                        Some(idx) => format!("subtitles='{}':si={},{}", path_str, idx, scale),
                        None => format!("subtitles='{}',{}", path_str, scale),
                    }
                } else {
                    scale
                };
                cmd.args([
                    "-c:v", "libx264",
                    "-preset", "fast",
                    "-crf", "22",
                    "-maxrate", bitrate,
                    "-bufsize", bufsize,
                    "-vf", &vf,
                    "-c:a", "aac",
                    "-b:a", "192k",
                    "-ac", "2",
                    "-af", audio_filter,
                ]);
            }
            TranscodeMode::Direct => unreachable!(),
        }

        // Normalize timestamps: force the minimum PTS across streams to 0 so
        // video and audio share a common origin in the HLS output. Without
        // this, fast-seek (`-ss` before `-i`) can leave audio with a small
        // positive offset relative to video.
        cmd.args(["-avoid_negative_ts", "make_zero"]);

        // Eliminate the MPEG-TS muxer's default buffering (700ms) that FFmpeg
        // inserts for broadcast decoder priming. For HLS playback this just
        // manifests as a dead-air gap at the start of the first segment —
        // video frames arrive, audio packets don't, hls.js plays the video
        // and waits for audio to catch up.
        cmd.args(["-muxdelay", "0", "-muxpreload", "0"]);

        cmd.args([
            "-f",
            "hls",
            "-hls_time",
            "6",
            "-hls_list_size",
            "0",
            "-hls_playlist_type",
            "event",
            "-hls_segment_type",
            "mpegts",
            "-hls_segment_filename",
        ]);
        cmd.arg(&segment_pattern);
        cmd.arg(&playlist_path);

        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        Ok(cmd.spawn()?)
    }

    /// Watch for the first HLS segment and signal readiness via the watch channel.
    ///
    /// Note: we intentionally do NOT probe the output segment's PTS. FFmpeg's
    /// MPEG-TS muxer resets output timestamps (typically starting at ~1.4s of
    /// buffering), so the output PTS does not represent the original-file
    /// offset. hls.js further normalizes `video.currentTime=0` to the first
    /// frame of the playlist, so the offset we need for subtitle alignment is
    /// exactly the requested `start_time` (set when the session starts or
    /// when `seek_session` restarts FFmpeg). Keyframe snapping introduces at
    /// most a GOP-size error, which is tolerable for subtitle timing.
    fn watch_for_ready(session: Arc<TranscodeSession>, output_dir: PathBuf, _ffmpeg_path: String) {
        tokio::spawn(async move {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if Instant::now() > deadline {
                    warn!("Timed out waiting for first HLS segment");
                    let _ = session.ready_tx.send(true);
                    return;
                }
                if let Ok(content) =
                    tokio::fs::read_to_string(output_dir.join("stream.m3u8")).await
                {
                    if content.contains(".ts") {
                        let _ = session.ready_tx.send(true);
                        debug!(
                            "First HLS segment ready for session {}",
                            session.session_id
                        );
                        return;
                    }
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        });
    }

    pub async fn get_session(&self, session_id: &str) -> Option<Arc<TranscodeSession>> {
        let sessions = self.sessions.read().await;
        if let Some(session) = sessions.get(session_id) {
            *session.last_accessed.write().await = Instant::now();
            Some(session.clone())
        } else {
            None
        }
    }

    pub async fn stop_session(&self, session_id: &str) {
        let session = {
            let mut sessions = self.sessions.write().await;
            sessions.remove(session_id)
        };

        if let Some(session) = session {
            // Kill FFmpeg process
            if let Some(mut child) = session.ffmpeg_child.write().await.take() {
                let _ = child.kill().await;
            }
            // Remove temp directory
            if session.output_dir.exists() {
                let _ = tokio::fs::remove_dir_all(&session.output_dir).await;
            }
            info!("Stopped transcode session {}", session_id);
        }
    }

    /// Stop all transcode sessions owned by a specific user.
    pub async fn stop_user_sessions(&self, user_id: i64) {
        let user_session_ids: Vec<String> = {
            let sessions = self.sessions.read().await;
            sessions
                .iter()
                .filter(|(_, s)| s.user_id == user_id)
                .map(|(id, _)| id.clone())
                .collect()
        };
        for id in &user_session_ids {
            info!("Stopping transcode session {} for user {}", id, user_id);
            self.stop_session(id).await;
        }
    }

    pub async fn cleanup_stale(&self, max_idle: Duration) {
        let stale_ids: Vec<String> = {
            let sessions = self.sessions.read().await;
            let mut stale = Vec::new();
            for (id, session) in sessions.iter() {
                let last = *session.last_accessed.read().await;
                if last.elapsed() > max_idle {
                    stale.push(id.clone());
                }
            }
            stale
        };

        for id in &stale_ids {
            debug!("Cleaning up stale transcode session {}", id);
            self.stop_session(id).await;
        }
    }

    /// Snapshot of currently-active transcode sessions, for admin observability.
    pub async fn list_sessions(&self) -> Vec<ActiveSessionInfo> {
        let sessions = self.sessions.read().await;
        let mut out = Vec::with_capacity(sessions.len());
        for session in sessions.values() {
            let start_time = *session.start_time.read().await;
            let idle_seconds = session.last_accessed.read().await.elapsed().as_secs();
            let burned_subtitle = session.burn_subtitle.as_ref().map(|b| BurnedSubtitleDisplay {
                stream_id: b.stream_id,
                language: b.language.clone(),
                title: b.title.clone(),
                is_forced: b.is_forced,
                is_external: b.is_external,
            });
            out.push(ActiveSessionInfo {
                session_id: session.session_id.clone(),
                user_id: session.user_id,
                media_item_id: session.media_item_id,
                mode: session.mode.as_str(),
                video_height: session.video_height,
                file_path: session.file_path.clone(),
                start_time_seconds: start_time,
                idle_seconds,
                burned_subtitle,
                audio_stream_id: session.audio_stream_id,
                audio_normalize: session.audio_normalize,
            });
        }
        out
    }

    pub async fn stop_all(&self) {
        let ids: Vec<String> = {
            let sessions = self.sessions.read().await;
            sessions.keys().cloned().collect()
        };
        for id in &ids {
            self.stop_session(id).await;
        }
    }
}

/// Create a sanitized symlink (or copy) of a burn-in subtitle source into the
/// session's output_dir so the FFmpeg filter string never contains special chars.
async fn resolve_burn_source(
    burn: &BurnSubtitle,
    video_path: &str,
    output_dir: &std::path::Path,
) -> Result<BurnResolved, anyhow::Error> {
    let (src_path, ext) = if burn.is_external {
        let ext_path = burn
            .external_file_path
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("burn subtitle marked external but has no path"))?;
        let src = PathBuf::from(ext_path);
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("srt")
            .to_string();
        (src, ext)
    } else {
        // Embedded — FFmpeg reads the video itself; symlink the video path.
        let src = PathBuf::from(video_path);
        let ext = src
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mkv")
            .to_string();
        (src, ext)
    };

    if !src_path.exists() {
        anyhow::bail!("burn subtitle source does not exist: {:?}", src_path);
    }

    let dest = output_dir.join(format!("burn_src.{}", ext));
    let _ = tokio::fs::remove_file(&dest).await;

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        if symlink(&src_path, &dest).is_ok() {
            return Ok(BurnResolved {
                sanitized_source: dest,
                stream_index: burn.stream_index,
            });
        }
    }
    // Fallback: copy (slow for large video files, but ensures correctness).
    tokio::fs::copy(&src_path, &dest).await?;
    Ok(BurnResolved {
        sanitized_source: dest,
        stream_index: burn.stream_index,
    })
}

fn uuid_v4() -> String {
    use rand::RngExt;
    let bytes: Vec<u8> = (0..16).map(|_| rand::rng().random::<u8>()).collect();
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3],
        bytes[4], bytes[5],
        (bytes[6] & 0x0f) | 0x40, bytes[7],
        (bytes[8] & 0x3f) | 0x80, bytes[9],
        bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stream(t: &str, codec: &str) -> MediaStream {
        MediaStream {
            id: 1,
            media_item_id: 1,
            stream_index: 0,
            stream_type: t.to_string(),
            codec: Some(codec.to_string()),
            language: None,
            title: None,
            is_default: false,
            is_forced: false,
            width: None,
            height: Some(1080),
            aspect_ratio: None,
            frame_rate: None,
            bit_depth: None,
            color_space: None,
            channels: None,
            sample_rate: None,
            bit_rate: None,
            is_external: false,
            external_file_path: None,
        }
    }

    fn caps() -> ClientCapabilities {
        ClientCapabilities {
            video_codecs: vec!["h264".into()],
            audio_codecs: vec!["aac".into()],
            containers: vec!["mp4".into()],
        }
    }

    #[test]
    fn direct_when_all_compatible() {
        let streams = vec![stream("video", "h264"), stream("audio", "aac")];
        let mode = decide_transcode_mode(&streams, Some("mp4"), &caps(), None, false, false);
        assert_eq!(mode, TranscodeMode::Direct);
    }

    #[test]
    fn burn_forces_full_transcode_even_when_compatible() {
        let streams = vec![stream("video", "h264"), stream("audio", "aac")];
        let mode = decide_transcode_mode(&streams, Some("mp4"), &caps(), None, true, false);
        assert_eq!(mode, TranscodeMode::FullTranscode);
    }

    #[test]
    fn remux_when_container_mismatch() {
        let streams = vec![stream("video", "h264"), stream("audio", "aac")];
        let mode = decide_transcode_mode(&streams, Some("mkv"), &caps(), None, false, false);
        assert_eq!(mode, TranscodeMode::Remux);
    }

    #[test]
    fn burn_overrides_remux_decision() {
        let streams = vec![stream("video", "h264"), stream("audio", "aac")];
        let mode = decide_transcode_mode(&streams, Some("mkv"), &caps(), None, true, false);
        assert_eq!(mode, TranscodeMode::FullTranscode);
    }

    #[test]
    fn audio_transcode_when_audio_incompatible() {
        let streams = vec![stream("video", "h264"), stream("audio", "ac3")];
        let mode = decide_transcode_mode(&streams, Some("mp4"), &caps(), None, false, false);
        assert_eq!(mode, TranscodeMode::AudioTranscode);
    }

    #[test]
    fn full_transcode_when_video_incompatible() {
        let streams = vec![stream("video", "hevc"), stream("audio", "aac")];
        let mode = decide_transcode_mode(&streams, Some("mp4"), &caps(), None, false, false);
        assert_eq!(mode, TranscodeMode::FullTranscode);
    }

    #[test]
    fn non_default_audio_selection_forces_at_least_remux() {
        let mut first = stream("audio", "aac");
        first.id = 10;
        let mut second = stream("audio", "aac");
        second.id = 11;
        let streams = vec![stream("video", "h264"), first, second.clone()];
        let mode = decide_transcode_mode(
            &streams,
            Some("mp4"),
            &caps(),
            Some(&second),
            false,
            false,
        );
        assert_eq!(mode, TranscodeMode::Remux);
    }

    #[test]
    fn normalize_promotes_direct_to_audio_transcode() {
        let streams = vec![stream("video", "h264"), stream("audio", "aac")];
        let mode = decide_transcode_mode(&streams, Some("mp4"), &caps(), None, false, true);
        assert_eq!(mode, TranscodeMode::AudioTranscode);
    }

    #[test]
    fn normalize_leaves_full_transcode_alone() {
        let streams = vec![stream("video", "hevc"), stream("audio", "aac")];
        let mode = decide_transcode_mode(&streams, Some("mp4"), &caps(), None, false, true);
        assert_eq!(mode, TranscodeMode::FullTranscode);
    }
}
