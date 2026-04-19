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
pub fn decide_transcode_mode(
    streams: &[MediaStream],
    container_format: Option<&str>,
    client: &ClientCapabilities,
) -> TranscodeMode {
    let video = streams.iter().find(|s| s.stream_type == "video");
    let audio = streams.iter().find(|s| s.stream_type == "audio");

    // No video stream — probably audio file, serve directly
    let Some(video) = video else {
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

    if video_ok && audio_ok && container_ok {
        TranscodeMode::Direct
    } else if video_ok && audio_ok && !container_ok {
        TranscodeMode::Remux
    } else if video_ok && !audio_ok {
        TranscodeMode::AudioTranscode
    } else {
        TranscodeMode::FullTranscode
    }
}

// ---------------------------------------------------------------------------
// Transcode session
// ---------------------------------------------------------------------------

pub struct TranscodeSession {
    pub session_id: String,
    pub media_item_id: i64,
    pub output_dir: PathBuf,
    pub mode: TranscodeMode,
    pub file_path: String,
    pub video_height: Option<i32>,
    pub start_time: RwLock<f64>,
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
        media_item_id: i64,
        file_path: &str,
        mode: TranscodeMode,
        video_stream: Option<&MediaStream>,
        _audio_stream: Option<&MediaStream>,
        start_time: f64,
    ) -> Result<Arc<TranscodeSession>, anyhow::Error> {
        // Check concurrent limit
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

        let child = Self::spawn_ffmpeg(
            &self.ffmpeg_path,
            file_path,
            &output_dir,
            mode,
            video_height,
            start_time,
        )
        .await?;

        let (ready_tx, ready_rx) = watch::channel(false);

        let session = Arc::new(TranscodeSession {
            session_id: session_id.clone(),
            media_item_id,
            output_dir: output_dir.clone(),
            mode,
            file_path: file_path.to_string(),
            video_height,
            start_time: RwLock::new(start_time),
            ffmpeg_child: RwLock::new(Some(child)),
            last_accessed: RwLock::new(Instant::now()),
            ready_tx,
            ready_rx,
        });

        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session.clone());

        Self::watch_for_ready(session.clone(), output_dir);

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

        // Spawn new FFmpeg from the seek position
        let child = Self::spawn_ffmpeg(
            &self.ffmpeg_path,
            &session.file_path,
            &session.output_dir,
            session.mode,
            session.video_height,
            seek_time,
        )
        .await?;

        *session.ffmpeg_child.write().await = Some(child);
        *session.start_time.write().await = seek_time;
        *session.last_accessed.write().await = Instant::now();

        Self::watch_for_ready(session.clone(), session.output_dir.clone());

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
    ) -> Result<tokio::process::Child, anyhow::Error> {
        tokio::fs::create_dir_all(output_dir).await?;

        let segment_pattern = output_dir.join("seg_%04d.ts");
        let playlist_path = output_dir.join("stream.m3u8");

        let mut cmd = tokio::process::Command::new(ffmpeg_path);

        // Seek before input for fast seeking (uses keyframes)
        if start_time > 0.0 {
            cmd.args(["-ss", &format!("{:.3}", start_time)]);
        }

        cmd.arg("-i").arg(file_path);
        cmd.args(["-y", "-nostdin"]);

        match mode {
            TranscodeMode::Remux => {
                cmd.args(["-c", "copy"]);
            }
            TranscodeMode::AudioTranscode => {
                cmd.args(["-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-ac", "2"]);
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
                cmd.args([
                    "-c:v",
                    "libx264",
                    "-preset",
                    "fast",
                    "-crf",
                    "22",
                    "-maxrate",
                    bitrate,
                    "-bufsize",
                    bufsize,
                    "-vf",
                    &format!("scale=-2:{}", height),
                    "-c:a",
                    "aac",
                    "-b:a",
                    "192k",
                    "-ac",
                    "2",
                ]);
            }
            TranscodeMode::Direct => unreachable!(),
        }

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
    fn watch_for_ready(session: Arc<TranscodeSession>, output_dir: PathBuf) {
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
