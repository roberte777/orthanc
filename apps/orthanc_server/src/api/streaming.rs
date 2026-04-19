use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::{AppState, StreamTokenData},
    },
    auth::middleware::AuthUser,
    models::media::MediaItem,
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
        .route("/{id}/progress", get(get_progress))
        .route("/{id}/progress", put(update_progress))
}

/// The streaming endpoint itself (mounted under /api/stream).
/// Auth is via query-param token, not JWT.
pub fn stream_router() -> Router<Arc<AppState>> {
    Router::new().route("/{media_id}", get(stream_media))
}

// ---------------------------------------------------------------------------
// Stream token
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct StreamTokenRequest {
    media_item_id: i64,
}

#[derive(Serialize)]
struct StreamTokenResponse {
    token: String,
    stream_url: String,
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

    if item.file_path.is_none() {
        return Err(ApiError::BadRequest(
            "Media item has no file".into(),
        ));
    }

    // Generate token
    let token = {
        use rand::RngExt;
        let bytes: Vec<u8> = (0..32).map(|_| rand::rng().random::<u8>()).collect();
        hex::encode(bytes)
    };

    let data = StreamTokenData {
        user_id,
        media_item_id: body.media_item_id,
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(5),
    };

    state.stream_tokens.write().await.insert(token.clone(), data);

    // Clean expired tokens opportunistically
    let now = chrono::Utc::now();
    state
        .stream_tokens
        .write()
        .await
        .retain(|_, v| v.expires_at > now);

    let stream_url = format!("/api/stream/{}?token={}", body.media_item_id, token);

    Ok(Json(StreamTokenResponse { token, stream_url }))
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
