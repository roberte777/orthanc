use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::{AppState, StreamTokenData},
    },
    auth::middleware::AuthUser,
    models::{media::MediaItem, media_stream::MediaStream},
    transcoding,
};
use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
    routing::{get, post, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::io::ReaderStream;

// ---------------------------------------------------------------------------
// Routes
// ---------------------------------------------------------------------------

/// Routes that require JWT auth (mounted under /api/media).
pub fn media_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/stream-token", post(create_stream_token))
        .route("/transcode-seek", post(transcode_seek))
        .route(
            "/transcode/{session_id}",
            axum::routing::delete(stop_transcode),
        )
        .route("/{id}/progress", get(get_progress))
        .route("/{id}/progress", put(update_progress))
}

/// The streaming endpoint itself (mounted under /api/stream).
/// Auth is via query-param token, not JWT.
pub fn stream_router() -> Router<Arc<AppState>> {
    Router::new().route("/{media_id}", get(stream_media))
}

/// HLS segment serving (mounted under /api/hls).
pub fn hls_router() -> Router<Arc<AppState>> {
    Router::new().route("/{session_id}/{*path}", get(serve_hls))
}

// ---------------------------------------------------------------------------
// Stream token
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamTokenRequest {
    media_item_id: i64,
    /// Optional start time for resume (seconds). Defaults to 0.
    #[serde(default)]
    start_time: f64,
    /// Video codecs the client can play (e.g. ["h264", "hevc"])
    #[serde(default)]
    supported_video_codecs: Vec<String>,
    /// Audio codecs the client can play (e.g. ["aac", "opus"])
    #[serde(default)]
    supported_audio_codecs: Vec<String>,
    /// Container formats the client can play (e.g. ["mp4", "webm"])
    #[serde(default)]
    supported_containers: Vec<String>,
}

#[derive(Serialize)]
struct StreamTokenResponse {
    token: String,
    stream_url: String,
    mode: String,
    title: String,
    duration_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    transcode_session_id: Option<String>,
}

async fn create_stream_token(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<StreamTokenRequest>,
) -> ApiResult<Json<StreamTokenResponse>> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid user id".into()))?;

    // Verify media item exists and has a file
    let item = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE id = ?",
    )
    .bind(body.media_item_id)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(ApiError::NotFound("Media item not found".into()))?;

    let file_path = item
        .file_path
        .as_deref()
        .ok_or(ApiError::BadRequest("Media item has no file".into()))?
        .to_string();

    // Query media streams to decide transcode mode
    let streams = sqlx::query_as::<_, MediaStream>(
        "SELECT * FROM media_streams WHERE media_item_id = ? ORDER BY stream_index",
    )
    .bind(body.media_item_id)
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let client_caps = transcoding::ClientCapabilities {
        video_codecs: body.supported_video_codecs,
        audio_codecs: body.supported_audio_codecs,
        containers: body.supported_containers,
    };
    let mode = transcoding::decide_transcode_mode(&streams, item.container_format.as_deref(), &client_caps);

    // Generate token
    let token = {
        use rand::RngExt;
        let bytes: Vec<u8> = (0..32).map(|_| rand::rng().random::<u8>()).collect();
        hex::encode(bytes)
    };

    let (stream_url, transcode_session_id) = if mode == transcoding::TranscodeMode::Direct {
        (
            format!("/api/stream/{}?token={}", body.media_item_id, token),
            None,
        )
    } else {
        // Start transcode session
        let video_stream = streams.iter().find(|s| s.stream_type == "video");
        let audio_stream = streams.iter().find(|s| s.stream_type == "audio");

        let session = state
            .transcode_manager
            .start_session(
                user_id,
                body.media_item_id,
                &file_path,
                mode,
                video_stream,
                audio_stream,
                body.start_time,
            )
            .await
            .map_err(|e| {
                let msg = e.to_string();
                if msg.contains("Maximum concurrent transcodes") {
                    ApiError::TooManyRequests("Too many active streams. Please stop playback on another device or try again shortly.".into())
                } else {
                    ApiError::Internal(anyhow::anyhow!("Transcode start failed: {}", e))
                }
            })?;

        // Wait for first segment (with timeout)
        let sid = session.session_id.clone();
        if !session.wait_until_ready(std::time::Duration::from_secs(30)).await {
            tracing::warn!("Timed out waiting for transcode to produce first segment");
        }

        (
            format!("/api/hls/{}/stream.m3u8?token={}", sid, token),
            Some(sid),
        )
    };

    let data = StreamTokenData {
        user_id,
        media_item_id: body.media_item_id,
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
        transcode_session_id: transcode_session_id.clone(),
    };

    state.stream_tokens.write().await.insert(token.clone(), data);

    // Clean expired tokens opportunistically
    let now = chrono::Utc::now();
    state
        .stream_tokens
        .write()
        .await
        .retain(|_, v| v.expires_at > now);

    Ok(Json(StreamTokenResponse {
        token,
        stream_url,
        mode: mode.as_str().to_string(),
        title: item.title.clone(),
        duration_seconds: item.duration_seconds,
        transcode_session_id,
    }))
}

// ---------------------------------------------------------------------------
// Transcode seek
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct TranscodeSeekRequest {
    session_id: String,
    seek_time: f64,
}

#[derive(Serialize)]
struct TranscodeSeekResponse {
    ready: bool,
    seek_time: f64,
}

async fn transcode_seek(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<TranscodeSeekRequest>,
) -> ApiResult<Json<TranscodeSeekResponse>> {
    let session = state
        .transcode_manager
        .seek_session(&body.session_id, body.seek_time)
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Seek failed: {}", e)))?;

    let ready = session
        .wait_until_ready(std::time::Duration::from_secs(30))
        .await;

    Ok(Json(TranscodeSeekResponse {
        ready,
        seek_time: body.seek_time,
    }))
}

// ---------------------------------------------------------------------------
// Stop transcode session
// ---------------------------------------------------------------------------

async fn stop_transcode(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> StatusCode {
    state.transcode_manager.stop_session(&session_id).await;
    StatusCode::NO_CONTENT
}

// ---------------------------------------------------------------------------
// Streaming endpoint
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamQuery {
    token: String,
}

/// Guard that decrements the active stream count on drop.
struct StreamGuard {
    active_streams: Arc<tokio::sync::RwLock<std::collections::HashMap<i64, usize>>>,
    user_id: i64,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let streams = self.active_streams.clone();
        let user_id = self.user_id;
        // Use try_write since we're in a sync Drop context
        if let Ok(mut map) = streams.try_write() {
            if let Some(count) = map.get_mut(&user_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    map.remove(&user_id);
                }
            }
        } else {
            // If we can't get the lock synchronously, spawn a task
            tokio::spawn(async move {
                let mut map = streams.write().await;
                if let Some(count) = map.get_mut(&user_id) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        map.remove(&user_id);
                    }
                }
            });
        }
    }
}

fn detect_mime_type(db_mime: Option<&str>, file_path: &str) -> String {
    if let Some(mime) = db_mime {
        if !mime.is_empty() {
            return mime.to_string();
        }
    }
    match file_path
        .rsplit('.')
        .next()
        .map(|e| e.to_lowercase())
        .as_deref()
    {
        Some("mp4" | "m4v") => "video/mp4".to_string(),
        Some("mkv") => "video/x-matroska".to_string(),
        Some("avi") => "video/x-msvideo".to_string(),
        Some("webm") => "video/webm".to_string(),
        Some("mov") => "video/quicktime".to_string(),
        Some("ts") => "video/mp2t".to_string(),
        Some("wmv") => "video/x-ms-wmv".to_string(),
        Some("flv") => "video/x-flv".to_string(),
        _ => "application/octet-stream".to_string(),
    }
}

async fn stream_media(
    State(state): State<Arc<AppState>>,
    Path(media_id): Path<i64>,
    Query(query): Query<StreamQuery>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    // Validate stream token
    let user_id = {
        let tokens = state.stream_tokens.read().await;
        let token_data = tokens
            .get(&query.token)
            .ok_or(ApiError::Unauthorized)?;

        if token_data.expires_at < chrono::Utc::now() {
            return Err(ApiError::Unauthorized);
        }
        if token_data.media_item_id != media_id {
            return Err(ApiError::Unauthorized);
        }
        token_data.user_id
    };

    // Check concurrent stream limit
    {
        let mut streams = state.active_streams.write().await;
        let count = streams.entry(user_id).or_insert(0);
        if *count >= state.max_concurrent_streams {
            return Err(ApiError::BadRequest(format!(
                "Maximum concurrent streams ({}) exceeded",
                state.max_concurrent_streams
            )));
        }
        *count += 1;
    }

    // Create guard to decrement on drop
    let _guard = StreamGuard {
        active_streams: state.active_streams.clone(),
        user_id,
    };

    // Look up media item
    let item = sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id = ?")
        .bind(media_id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("Media item not found".into()))?;

    let file_path = item
        .file_path
        .as_deref()
        .ok_or(ApiError::NotFound("No file path for media item".into()))?;

    let mime_type = detect_mime_type(item.mime_type.as_deref(), file_path);

    // Open the file
    let file = tokio::fs::File::open(file_path)
        .await
        .map_err(|e| {
            tracing::error!("Failed to open media file '{}': {}", file_path, e);
            ApiError::NotFound("Media file not found on disk".into())
        })?;

    let metadata = file
        .metadata()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to read file metadata: {}", e)))?;
    let file_size = metadata.len();

    // Parse Range header
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| parse_range(s, file_size));

    match range {
        Some((start, end)) => {
            // Partial content (206)
            let length = end - start + 1;
            let mut file = file;
            file.seek(std::io::SeekFrom::Start(start))
                .await
                .map_err(|e| ApiError::Internal(anyhow::anyhow!("Seek failed: {}", e)))?;

            let limited = file.take(length);
            let stream = ReaderStream::new(limited);
            let body = Body::from_stream(apply_throttle(stream, state.max_bandwidth_bytes_per_sec));

            Ok(Response::builder()
                .status(StatusCode::PARTIAL_CONTENT)
                .header(header::CONTENT_TYPE, &mime_type)
                .header(header::CONTENT_LENGTH, length)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(
                    header::CONTENT_RANGE,
                    format!("bytes {}-{}/{}", start, end, file_size),
                )
                .header(header::CACHE_CONTROL, "no-cache")
                .body(body)
                .unwrap())
        }
        None => {
            // Full content (200)
            let stream = ReaderStream::new(file);
            let body = Body::from_stream(apply_throttle(stream, state.max_bandwidth_bytes_per_sec));

            Ok(Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, &mime_type)
                .header(header::CONTENT_LENGTH, file_size)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CACHE_CONTROL, "no-cache")
                .body(body)
                .unwrap())
        }
    }
}

/// Parse a Range header value like "bytes=0-1023" into (start, end).
fn parse_range(header: &str, file_size: u64) -> Option<(u64, u64)> {
    let range_str = header.strip_prefix("bytes=")?;
    let mut parts = range_str.splitn(2, '-');
    let start_str = parts.next()?.trim();
    let end_str = parts.next()?.trim();

    if start_str.is_empty() {
        // Suffix range: bytes=-500 means last 500 bytes
        let suffix_len: u64 = end_str.parse().ok()?;
        let start = file_size.saturating_sub(suffix_len);
        Some((start, file_size - 1))
    } else {
        let start: u64 = start_str.parse().ok()?;
        let end = if end_str.is_empty() {
            file_size - 1
        } else {
            end_str.parse().ok()?
        };
        if start > end || start >= file_size {
            return None;
        }
        Some((start, end.min(file_size - 1)))
    }
}

// ---------------------------------------------------------------------------
// Bandwidth throttling
// ---------------------------------------------------------------------------

fn apply_throttle<S>(
    stream: S,
    max_bytes_per_sec: Option<u64>,
) -> ThrottledStream<S> {
    ThrottledStream {
        inner: stream,
        max_bytes_per_sec,
        bytes_sent_in_window: 0,
        window_start: tokio::time::Instant::now(),
    }
}

use std::pin::Pin;
use std::task::{Context, Poll};

struct ThrottledStream<S> {
    inner: S,
    max_bytes_per_sec: Option<u64>,
    bytes_sent_in_window: u64,
    window_start: tokio::time::Instant,
}

impl<S, T, E> futures_core::Stream for ThrottledStream<S>
where
    S: futures_core::Stream<Item = Result<T, E>> + Unpin,
    T: AsRef<[u8]>,
{
    type Item = Result<T, E>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Result<T, E>>> {
        let this = self.get_mut();

        if let Some(limit) = this.max_bytes_per_sec {
            let elapsed = this.window_start.elapsed();
            if elapsed >= std::time::Duration::from_secs(1) {
                // Reset window
                this.bytes_sent_in_window = 0;
                this.window_start = tokio::time::Instant::now();
            } else if this.bytes_sent_in_window >= limit {
                // We've hit the limit for this window, schedule a wake-up
                let remaining = std::time::Duration::from_secs(1) - elapsed;
                let waker = cx.waker().clone();
                tokio::spawn(async move {
                    tokio::time::sleep(remaining).await;
                    waker.wake();
                });
                return Poll::Pending;
            }
        }

        match Pin::new(&mut this.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(chunk))) => {
                this.bytes_sent_in_window += chunk.as_ref().len() as u64;
                Poll::Ready(Some(Ok(chunk)))
            }
            other => other,
        }
    }
}

// ---------------------------------------------------------------------------
// HLS serving
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OptionalTokenQuery {
    token: Option<String>,
}

async fn serve_hls(
    State(state): State<Arc<AppState>>,
    Path((session_id, path)): Path<(String, String)>,
    Query(query): Query<OptionalTokenQuery>,
) -> Result<Response, ApiError> {
    // Validate token if provided
    if let Some(ref token) = query.token {
        let tokens = state.stream_tokens.read().await;
        if let Some(token_data) = tokens.get(token) {
            if token_data.expires_at < chrono::Utc::now() {
                return Err(ApiError::Unauthorized);
            }
        } else {
            return Err(ApiError::Unauthorized);
        }
    }

    // Get session
    let session = state
        .transcode_manager
        .get_session(&session_id)
        .await
        .ok_or(ApiError::NotFound("Transcode session not found".into()))?;

    // Sanitize path — reject directory traversal
    if path.contains("..") {
        return Err(ApiError::BadRequest("Invalid path".into()));
    }

    let file_path = session.output_dir.join(&path);

    // Wait for the file if it doesn't exist yet (FFmpeg may still be producing it)
    let wait_duration = if path.ends_with(".ts") {
        std::time::Duration::from_secs(10)
    } else {
        std::time::Duration::from_secs(5)
    };
    let deadline = tokio::time::Instant::now() + wait_duration;

    let file = loop {
        match tokio::fs::File::open(&file_path).await {
            Ok(f) => break f,
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
            Err(_) => {
                return Err(ApiError::NotFound(format!("HLS file not found: {}", path)));
            }
        }
    };

    let metadata = file.metadata().await.map_err(|e| {
        ApiError::Internal(anyhow::anyhow!("Failed to read file metadata: {}", e))
    })?;

    let (content_type, cache_control) = if path.ends_with(".m3u8") {
        ("application/vnd.apple.mpegurl", "no-cache, no-store")
    } else if path.ends_with(".ts") {
        // Segments get replaced on seek (same filename, new content),
        // so we must not cache them
        ("video/mp2t", "no-cache, no-store")
    } else {
        ("application/octet-stream", "no-cache")
    };

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_LENGTH, metadata.len())
        .header(header::CACHE_CONTROL, cache_control)
        .body(body)
        .unwrap())
}

// ---------------------------------------------------------------------------
// Progress tracking
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ProgressUpdate {
    position_seconds: i32,
}

#[derive(Serialize)]
struct ProgressResponse {
    position_seconds: i32,
    is_completed: bool,
}

async fn update_progress(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Path(media_id): Path<i64>,
    Json(body): Json<ProgressUpdate>,
) -> ApiResult<StatusCode> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid user id".into()))?;

    // Get duration to check if completed (>= 90% watched)
    let duration: Option<(Option<i32>,)> =
        sqlx::query_as("SELECT duration_seconds FROM media_items WHERE id = ?")
            .bind(media_id)
            .fetch_optional(&state.db)
            .await
            .map_err(anyhow::Error::from)?;

    let is_completed = duration
        .and_then(|(d,)| d)
        .map(|d| body.position_seconds as f64 / d as f64 >= 0.9)
        .unwrap_or(false);

    sqlx::query(
        "INSERT INTO user_media_progress (user_id, media_item_id, playback_position_seconds, is_completed, completed_at, last_updated_at)
         VALUES (?, ?, ?, ?, ?, datetime('now'))
         ON CONFLICT(user_id, media_item_id) DO UPDATE SET
            playback_position_seconds = excluded.playback_position_seconds,
            is_completed = excluded.is_completed,
            completed_at = CASE WHEN excluded.is_completed = 1 AND user_media_progress.is_completed = 0 THEN datetime('now') ELSE user_media_progress.completed_at END,
            last_updated_at = datetime('now')"
    )
    .bind(user_id)
    .bind(media_id)
    .bind(body.position_seconds)
    .bind(is_completed)
    .bind(if is_completed { Some(chrono::Utc::now().to_rfc3339()) } else { None })
    .execute(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(StatusCode::NO_CONTENT)
}

async fn get_progress(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Path(media_id): Path<i64>,
) -> ApiResult<Json<ProgressResponse>> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid user id".into()))?;

    let row: Option<(i32, bool)> = sqlx::query_as(
        "SELECT playback_position_seconds, is_completed FROM user_media_progress WHERE user_id = ? AND media_item_id = ?",
    )
    .bind(user_id)
    .bind(media_id)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let (position_seconds, is_completed) = row.unwrap_or((0, false));

    Ok(Json(ProgressResponse {
        position_seconds,
        is_completed,
    }))
}
