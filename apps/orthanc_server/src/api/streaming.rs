use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::{AppState, StreamTokenData},
    },
    auth::middleware::AuthUser,
    models::{
        media::MediaItem, media_stream::MediaStream, track_preference,
        track_preference::TrackPreference, user_preference,
    },
    transcoding,
};
use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::{get, post, put},
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
        .route("/track-preferences", put(save_track_preferences))
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

/// Subtitle WebVTT serving (mounted under /api/subtitles).
/// Auth is via query-param stream token (same as HLS).
pub fn subtitles_router() -> Router<Arc<AppState>> {
    Router::new().route("/{stream_filename}", get(serve_subtitle))
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
    /// If set, force FullTranscode and burn this subtitle into the video.
    #[serde(default)]
    burn_subtitle_id: Option<i64>,
    /// DB id of the audio stream the client wants to hear. When omitted, the
    /// server picks the `is_default`-flagged audio, else the first audio stream.
    #[serde(default)]
    audio_stream_id: Option<i64>,
    /// When true, apply EBU R128 loudness normalization. Forces at least an
    /// AudioTranscode because normalization requires a re-encode.
    #[serde(default)]
    audio_normalize: bool,
}

#[derive(Serialize)]
struct SubtitleTrackOut {
    id: i64,
    language: Option<String>,
    title: Option<String>,
    codec: Option<String>,
    is_default: bool,
    is_forced: bool,
    is_external: bool,
    /// "vtt" | "burn_required"
    delivery: &'static str,
}

#[derive(Serialize)]
struct AudioTrackOut {
    id: i64,
    language: Option<String>,
    title: Option<String>,
    codec: Option<String>,
    channels: Option<i32>,
    sample_rate: Option<i32>,
    bit_rate: Option<i32>,
    is_default: bool,
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
    #[serde(default)]
    subtitles: Vec<SubtitleTrackOut>,
    /// The subtitle track the client should attach as a VTT overlay (from saved
    /// preference or server default). `None` means no subtitle overlay.
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_subtitle_id: Option<i64>,
    /// When a burn was requested, echoes the honored subtitle id.
    #[serde(skip_serializing_if = "Option::is_none")]
    burned_subtitle_id: Option<i64>,
    /// Actual start-time PTS of the first HLS segment (populated for HLS modes
    /// once the segment is produced). Used as the subtitle offset.
    #[serde(skip_serializing_if = "Option::is_none")]
    transcode_actual_start_seconds: Option<f64>,
    #[serde(default)]
    audio_tracks: Vec<AudioTrackOut>,
    /// Echoes the audio stream id actually chosen by the server (either from the
    /// request or from default resolution). `None` when the item has no audio.
    #[serde(skip_serializing_if = "Option::is_none")]
    selected_audio_stream_id: Option<i64>,
    /// Echoes whether loudness normalization is active.
    audio_normalize: bool,
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
    let item = sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id = ?")
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

    // Load saved track preference for this user's scope (show for episodes,
    // movie otherwise). Used as a fallback when the request doesn't pin tracks.
    // If no per-show row exists, fall through to the user's global defaults.
    let scope_id = track_preference::resolve_scope_media_item_id(&state.db, body.media_item_id)
        .await
        .map_err(ApiError::Internal)?;
    let per_media_pref = track_preference::load_preference(&state.db, user_id, scope_id)
        .await
        .map_err(ApiError::Internal)?;
    let pref: Option<TrackPreference> = match per_media_pref {
        Some(p) => Some(p),
        None => user_preference::load_preference(&state.db, user_id)
            .await
            .map_err(ApiError::Internal)?
            .map(|g| TrackPreference {
                user_id,
                scope_media_item_id: scope_id,
                audio_language: g.preferred_audio_language,
                subtitle_language: g.preferred_subtitle_language,
                subtitles_enabled: g.subtitles_enabled_default,
                audio_normalize: g.audio_normalize_default,
                updated_at: g.updated_at,
            }),
    };

    // Resolve burn request (if any) before picking a mode — a burn forces FullTranscode.
    let mut burn_stream: Option<MediaStream> = None;
    if let Some(burn_id) = body.burn_subtitle_id {
        let s = streams
            .iter()
            .find(|s| s.id == burn_id && s.stream_type == "subtitle")
            .cloned()
            .ok_or_else(|| {
                ApiError::BadRequest(
                    "burn_subtitle_id does not match a subtitle stream for this item".into(),
                )
            })?;
        burn_stream = Some(s);
    }

    // Resolve audio selection: explicit request wins, else saved language
    // preference, else the `is_default` audio, else the first audio stream.
    let selected_audio: Option<MediaStream> = if let Some(audio_id) = body.audio_stream_id {
        let s = streams
            .iter()
            .find(|s| s.id == audio_id && s.stream_type == "audio")
            .cloned()
            .ok_or_else(|| {
                ApiError::BadRequest(
                    "audio_stream_id does not match an audio stream for this item".into(),
                )
            })?;
        Some(s)
    } else {
        let pref_audio = pref
            .as_ref()
            .and_then(|p| p.audio_language.as_deref())
            .and_then(|lang| {
                streams
                    .iter()
                    .find(|s| s.stream_type == "audio" && s.language.as_deref() == Some(lang))
            });
        pref_audio
            .or_else(|| {
                streams
                    .iter()
                    .find(|s| s.stream_type == "audio" && s.is_default)
            })
            .or_else(|| streams.iter().find(|s| s.stream_type == "audio"))
            .cloned()
    };

    // Resolve subtitle preference when no explicit burn is already requested.
    // VTT-deliverable picks become `selected_subtitle_id`; burn-required picks
    // force FullTranscode via `burn_stream` (same path as an explicit burn).
    let mut selected_subtitle_id: Option<i64> = None;
    if let (true, Some(p)) = (burn_stream.is_none(), pref.as_ref())
        && p.subtitles_enabled
        && let Some(lang) = p.subtitle_language.as_deref()
        && let Some(s) = streams
            .iter()
            .find(|s| s.stream_type == "subtitle" && s.language.as_deref() == Some(lang))
    {
        match crate::subtitles::classify(s) {
            crate::subtitles::DeliveryMethod::Vtt => {
                selected_subtitle_id = Some(s.id);
            }
            crate::subtitles::DeliveryMethod::BurnRequired => {
                burn_stream = Some(s.clone());
            }
            crate::subtitles::DeliveryMethod::Unsupported => {}
        }
    }

    // Audio normalize: explicit request wins if true; otherwise fall back to pref.
    let audio_normalize =
        body.audio_normalize || pref.as_ref().map(|p| p.audio_normalize).unwrap_or(false);

    let mode = transcoding::decide_transcode_mode(
        &streams,
        item.container_format.as_deref(),
        &client_caps,
        selected_audio.as_ref(),
        burn_stream.is_some(),
        audio_normalize,
    );

    // Generate token
    let token = {
        use rand::RngExt;
        let bytes: Vec<u8> = (0..32).map(|_| rand::rng().random::<u8>()).collect();
        hex::encode(bytes)
    };

    // Pre-warm subtitle extraction during the long path (transcode warmup).
    // For direct play, this runs in parallel with token insertion; for HLS,
    // it overlaps with the 30-second wait_until_ready window.
    let prewarm_handle = spawn_subtitle_prewarm(state.clone(), streams.clone(), file_path.clone());

    let (stream_url, transcode_session_id, actual_start_seconds) = if mode
        == transcoding::TranscodeMode::Direct
    {
        (
            format!("/api/stream/{}?token={}", body.media_item_id, token),
            None,
            None,
        )
    } else {
        // Start transcode session
        let video_stream = streams.iter().find(|s| s.stream_type == "video");

        let burn = build_burn_option(&state, burn_stream.as_ref(), &file_path).await?;

        let session = state
                .transcode_manager
                .start_session(
                    user_id,
                    body.media_item_id,
                    &file_path,
                    mode,
                    video_stream,
                    selected_audio.as_ref(),
                    body.start_time,
                    burn,
                    audio_normalize,
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
        if !session
            .wait_until_ready(std::time::Duration::from_secs(30))
            .await
        {
            tracing::warn!("Timed out waiting for transcode to produce first segment");
        }
        let actual = *session.actual_start_time.read().await;
        let actual_opt = if actual > 0.0 { Some(actual) } else { None };

        (
            format!("/api/hls/{}/stream.m3u8?token={}", sid, token),
            Some(sid),
            actual_opt,
        )
    };
    // Best-effort pre-warm; we don't block on it here.
    drop(prewarm_handle);

    let expiry_minutes =
        crate::api::settings::read_setting(&state.db, "stream_token_expiry_minutes")
            .await
            .and_then(|s| s.parse::<i64>().ok())
            .filter(|&m| m > 0)
            .unwrap_or(5);

    let data = StreamTokenData {
        user_id,
        media_item_id: body.media_item_id,
        expires_at: chrono::Utc::now() + chrono::Duration::minutes(expiry_minutes),
        transcode_session_id: transcode_session_id.clone(),
    };

    state
        .stream_tokens
        .write()
        .await
        .insert(token.clone(), data);

    // Clean expired tokens opportunistically
    let now = chrono::Utc::now();
    state
        .stream_tokens
        .write()
        .await
        .retain(|_, v| v.expires_at > now);

    let subtitles_out = build_subtitle_list(&streams);
    let audio_tracks_out = build_audio_list(&streams);
    let burned_subtitle_id = burn_stream.as_ref().map(|s| s.id);
    let selected_audio_stream_id = selected_audio.as_ref().map(|s| s.id);

    Ok(Json(StreamTokenResponse {
        token,
        stream_url,
        mode: mode.as_str().to_string(),
        title: item.title.clone(),
        duration_seconds: item.duration_seconds,
        transcode_session_id,
        subtitles: subtitles_out,
        selected_subtitle_id,
        burned_subtitle_id,
        transcode_actual_start_seconds: actual_start_seconds,
        audio_tracks: audio_tracks_out,
        selected_audio_stream_id,
        audio_normalize,
    }))
}

fn build_subtitle_list(streams: &[MediaStream]) -> Vec<SubtitleTrackOut> {
    let mut out = Vec::new();
    for s in streams {
        if s.stream_type != "subtitle" {
            continue;
        }
        let delivery = match crate::subtitles::classify(s) {
            crate::subtitles::DeliveryMethod::Vtt => "vtt",
            crate::subtitles::DeliveryMethod::BurnRequired => "burn_required",
            crate::subtitles::DeliveryMethod::Unsupported => continue,
        };
        out.push(SubtitleTrackOut {
            id: s.id,
            language: s.language.clone(),
            title: s.title.clone(),
            codec: s.codec.clone(),
            is_default: s.is_default,
            is_forced: s.is_forced,
            is_external: s.is_external,
            delivery,
        });
    }
    out
}

fn build_audio_list(streams: &[MediaStream]) -> Vec<AudioTrackOut> {
    streams
        .iter()
        .filter(|s| s.stream_type == "audio")
        .map(|s| AudioTrackOut {
            id: s.id,
            language: s.language.clone(),
            title: s.title.clone(),
            codec: s.codec.clone(),
            channels: s.channels,
            sample_rate: s.sample_rate,
            bit_rate: s.bit_rate,
            is_default: s.is_default,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Track preferences
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SaveTrackPreferencesRequest {
    media_item_id: i64,
    #[serde(default)]
    audio_language: Option<String>,
    #[serde(default)]
    subtitle_language: Option<String>,
    #[serde(default)]
    subtitles_enabled: bool,
    #[serde(default)]
    audio_normalize: bool,
}

async fn save_track_preferences(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Json(body): Json<SaveTrackPreferencesRequest>,
) -> ApiResult<StatusCode> {
    let user_id: i64 = claims
        .sub
        .parse()
        .map_err(|_| ApiError::BadRequest("Invalid user id".into()))?;

    let scope_id = track_preference::resolve_scope_media_item_id(&state.db, body.media_item_id)
        .await
        .map_err(ApiError::Internal)?;

    track_preference::upsert_preference(
        &state.db,
        user_id,
        scope_id,
        body.audio_language.as_deref(),
        body.subtitle_language.as_deref(),
        body.subtitles_enabled,
        body.audio_normalize,
    )
    .await
    .map_err(ApiError::Internal)?;

    Ok(StatusCode::NO_CONTENT)
}

/// Spawn a background task that pre-extracts WebVTT for every text-deliverable
/// subtitle stream. Best-effort — errors are logged but not surfaced.
fn spawn_subtitle_prewarm(
    state: Arc<AppState>,
    streams: Vec<MediaStream>,
    file_path: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        for s in &streams {
            if crate::subtitles::classify(s) != crate::subtitles::DeliveryMethod::Vtt {
                continue;
            }
            if let Err(e) = state.subtitle_manager.extract_vtt(s, &file_path).await {
                tracing::debug!("Subtitle pre-warm failed for stream {}: {}", s.id, e);
            }
        }
    })
}

/// Build a burn option from the selected subtitle and (for embedded streams)
/// also return the sanitized source path. For bitmap-required burns (PGS), we
/// currently return a clear error rather than attempt an overlay graph.
async fn build_burn_option(
    _state: &Arc<AppState>,
    burn_stream: Option<&MediaStream>,
    _video_path: &str,
) -> Result<Option<transcoding::BurnSubtitle>, ApiError> {
    let Some(stream) = burn_stream else {
        return Ok(None);
    };
    // Only text codecs are supported for burn-in in the MVP.
    let is_text = matches!(
        stream.codec.as_deref().map(str::to_lowercase).as_deref(),
        Some("subrip" | "srt" | "webvtt" | "vtt" | "mov_text" | "ass" | "ssa" | "text")
    );
    if !is_text {
        return Err(ApiError::BadRequest(
            "bitmap subtitle burn-in (PGS/VobSub/DVB) is not yet supported".into(),
        ));
    }
    Ok(Some(transcoding::BurnSubtitle {
        stream_id: stream.id,
        stream_index: if stream.is_external {
            None
        } else {
            Some(stream.stream_index)
        },
        external_file_path: stream.external_file_path.clone(),
        language: stream.language.clone(),
        title: stream.title.clone(),
        is_forced: stream.is_forced,
        is_external: stream.is_external,
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
    /// True PTS of the first segment after restart — use as subtitle offset.
    actual_start_seconds: f64,
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
    let actual = *session.actual_start_time.read().await;

    Ok(Json(TranscodeSeekResponse {
        ready,
        seek_time: body.seek_time,
        actual_start_seconds: actual,
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
    if let Some(mime) = db_mime
        && !mime.is_empty() {
            return mime.to_string();
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
        let token_data = tokens.get(&query.token).ok_or(ApiError::Unauthorized)?;

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
    let file = tokio::fs::File::open(file_path).await.map_err(|e| {
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

fn apply_throttle<S>(stream: S, max_bytes_per_sec: Option<u64>) -> ThrottledStream<S> {
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

    let metadata = file
        .metadata()
        .await
        .map_err(|e| ApiError::Internal(anyhow::anyhow!("Failed to read file metadata: {}", e)))?;

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

// ---------------------------------------------------------------------------
// Subtitle serving
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct SubtitleQuery {
    token: Option<String>,
    /// Offset (in seconds) to subtract from every cue timestamp. Used by the
    /// player during HLS playback to align cue times with the transcoded
    /// playlist, which starts at `actual_start_time`.
    #[serde(default)]
    offset: f64,
}

async fn serve_subtitle(
    State(state): State<Arc<AppState>>,
    Path(stream_filename): Path<String>,
    Query(query): Query<SubtitleQuery>,
) -> Result<Response, ApiError> {
    // Parse `<stream_id>.vtt` from the path segment.
    let stream_id: i64 = stream_filename
        .strip_suffix(".vtt")
        .ok_or(ApiError::BadRequest("expected <id>.vtt".into()))?
        .parse()
        .map_err(|_| ApiError::BadRequest("invalid stream id".into()))?;

    // Token validation mirrors serve_hls: lenient — validate if present.
    // We also require the token's media_item match this subtitle's item, so
    // clients can't use a random stream token to pull subtitles for other items.
    let token_media_id: Option<i64> = if let Some(ref token) = query.token {
        let tokens = state.stream_tokens.read().await;
        let data = tokens.get(token).ok_or(ApiError::Unauthorized)?;
        if data.expires_at < chrono::Utc::now() {
            return Err(ApiError::Unauthorized);
        }
        Some(data.media_item_id)
    } else {
        None
    };

    // Load the subtitle stream row and its parent media item.
    let stream = sqlx::query_as::<_, crate::models::media_stream::MediaStream>(
        "SELECT * FROM media_streams WHERE id = ? AND stream_type = 'subtitle'",
    )
    .bind(stream_id)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(ApiError::NotFound("subtitle stream not found".into()))?;

    if let Some(tok_id) = token_media_id
        && tok_id != stream.media_item_id {
            return Err(ApiError::Unauthorized);
        }

    let item = sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id = ?")
        .bind(stream.media_item_id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("media item not found".into()))?;

    let file_path = item
        .file_path
        .as_deref()
        .ok_or(ApiError::NotFound("media item has no file".into()))?;

    let body = state
        .subtitle_manager
        .vtt_with_offset(&stream, file_path, query.offset)
        .await
        .map_err(|e| match e {
            crate::subtitles::SubtitleError::StreamNotFound => {
                ApiError::NotFound("subtitle source missing".into())
            }
            crate::subtitles::SubtitleError::DeliveryUnsupported => ApiError::BadRequest(
                "this subtitle cannot be served as WebVTT (bitmap); request burn-in instead".into(),
            ),
            crate::subtitles::SubtitleError::PathOutsideLibrary(p) => {
                tracing::warn!("Refusing subtitle path outside library: {}", p);
                ApiError::Forbidden
            }
            crate::subtitles::SubtitleError::CacheEmpty
            | crate::subtitles::SubtitleError::Ffmpeg(_) => {
                tracing::warn!("Subtitle extraction failed: {}", e);
                ApiError::Internal(anyhow::anyhow!("subtitle extraction failed"))
            }
            crate::subtitles::SubtitleError::Io(s) => {
                ApiError::Internal(anyhow::anyhow!("subtitle i/o: {}", s))
            }
        })?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/vtt; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .unwrap())
}
