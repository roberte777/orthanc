use crate::db::DbPool;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Data for a short-lived streaming token.
pub struct StreamTokenData {
    pub user_id: i64,
    pub media_item_id: i64,
    pub expires_at: chrono::DateTime<chrono::Utc>,
    pub transcode_session_id: Option<String>,
}

pub struct AppState {
    pub db: DbPool,
    pub jwt_secret: String,
    pub access_token_expiry: u64,  // seconds
    pub refresh_token_expiry: u64, // seconds
    pub tmdb_api_key: Option<String>,
    pub tvdb_api_key: String,
    pub image_cache_dir: String,
    // Streaming
    pub stream_tokens: Arc<RwLock<HashMap<String, StreamTokenData>>>,
    pub active_streams: Arc<RwLock<HashMap<i64, usize>>>,
    pub max_concurrent_streams: usize,
    pub max_bandwidth_bytes_per_sec: Option<u64>,
    // Transcoding
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    pub transcode_cache_dir: String,
    pub transcode_manager: Arc<crate::transcoding::TranscodeSessionManager>,
    // Subtitles
    pub subtitle_cache_dir: String,
    pub subtitle_cache_max_bytes: u64,
    pub subtitle_manager: Arc<crate::subtitles::SubtitleManager>,
}

/// Admin-configurable values resolved from the `settings` table at startup.
/// Precedence at boot: DB (non-empty) > env var > built-in default.
#[derive(Default, Debug)]
struct AdminOverrides {
    tmdb_api_key: Option<String>,
    tvdb_api_key: Option<String>,
    max_concurrent_streams: Option<usize>,
    max_concurrent_transcodes: Option<usize>,
}

async fn load_admin_overrides(db: &DbPool) -> AdminOverrides {
    let mut o = AdminOverrides::default();
    let rows = sqlx::query_as::<_, (String, String)>("SELECT key, value FROM settings")
        .fetch_all(db)
        .await
        .unwrap_or_default();
    for (key, value) in rows {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            continue;
        }
        match key.as_str() {
            "tmdb_api_key" => o.tmdb_api_key = Some(trimmed.to_string()),
            "tvdb_api_key" => o.tvdb_api_key = Some(trimmed.to_string()),
            "max_concurrent_streams" => o.max_concurrent_streams = trimmed.parse().ok(),
            "max_concurrent_transcodes" => o.max_concurrent_transcodes = trimmed.parse().ok(),
            _ => {}
        }
    }
    o
}

impl AppState {
    /// Build AppState using env vars for paths and the DB to resolve library roots.
    pub async fn from_env_async(db: DbPool) -> Self {
        let library_roots = load_library_roots(&db).await;
        let overrides = load_admin_overrides(&db).await;
        Self::build(db, library_roots, overrides)
    }

    #[cfg(test)]
    pub fn from_env(db: DbPool) -> Self {
        Self::build(db, Vec::new(), AdminOverrides::default())
    }

    fn build(db: DbPool, library_roots: Vec<PathBuf>, overrides: AdminOverrides) -> Self {
        let jwt_secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| {
            tracing::warn!("JWT_SECRET not set, using random secret (not suitable for production)");
            use rand::RngExt;
            let secret: String = rand::rng()
                .sample_iter(rand::distr::Alphanumeric)
                .take(64)
                .map(char::from)
                .collect();
            secret
        });

        const DEFAULT_TMDB_API_KEY: &str = "90c1d19c76fe6f06350a3df495e75365";

        let tmdb_api_key = Some(overrides.tmdb_api_key.clone().unwrap_or_else(|| {
            std::env::var("TMDB_API_KEY").unwrap_or_else(|_| DEFAULT_TMDB_API_KEY.to_string())
        }));

        // Embedded TVDB project API key (like Jellyfin's TvdbPlugin). Users may
        // override with their own subscriber key via the admin settings UI or TVDB_API_KEY.
        const DEFAULT_TVDB_API_KEY: &str = "91090b09-8411-4b64-834a-733ab3f12a07";

        let tvdb_api_key = overrides.tvdb_api_key.clone().unwrap_or_else(|| {
            std::env::var("TVDB_API_KEY").unwrap_or_else(|_| DEFAULT_TVDB_API_KEY.to_string())
        });

        let image_cache_dir =
            std::env::var("IMAGE_CACHE_DIR").unwrap_or_else(|_| "./image_cache".to_string());

        // Ensure image cache directory exists
        if let Err(e) = std::fs::create_dir_all(&image_cache_dir) {
            tracing::error!(
                "Failed to create image cache dir '{}': {}",
                image_cache_dir,
                e
            );
        }

        let max_concurrent_streams = overrides.max_concurrent_streams.unwrap_or_else(|| {
            std::env::var("MAX_CONCURRENT_STREAMS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(3)
        });

        let max_bandwidth_bytes_per_sec = std::env::var("MAX_BANDWIDTH_MBPS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mbps| mbps * 1_000_000);

        let ffmpeg_path = std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string());
        let ffprobe_path = std::env::var("FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string());
        let transcode_cache_dir = std::env::var("TRANSCODE_CACHE_DIR")
            .unwrap_or_else(|_| "./transcode_cache".to_string());

        if let Err(e) = std::fs::create_dir_all(&transcode_cache_dir) {
            tracing::error!(
                "Failed to create transcode cache dir '{}': {}",
                transcode_cache_dir,
                e
            );
        }

        let max_concurrent_transcodes = overrides.max_concurrent_transcodes.unwrap_or_else(|| {
            std::env::var("MAX_CONCURRENT_TRANSCODES")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2)
        });

        let transcode_manager = Arc::new(crate::transcoding::TranscodeSessionManager::new(
            transcode_cache_dir.clone().into(),
            ffmpeg_path.clone(),
            max_concurrent_transcodes,
        ));

        let subtitle_cache_dir =
            std::env::var("SUBTITLE_CACHE_DIR").unwrap_or_else(|_| "./subtitle_cache".to_string());
        let subtitle_cache_max_mb = std::env::var("SUBTITLE_CACHE_MAX_MB")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(500);
        let subtitle_cache_max_bytes = subtitle_cache_max_mb.saturating_mul(1_024 * 1_024);

        let subtitle_manager = Arc::new(crate::subtitles::SubtitleManager::new(
            PathBuf::from(&subtitle_cache_dir),
            ffmpeg_path.clone(),
            library_roots,
        ));

        Self {
            db,
            jwt_secret,
            access_token_expiry: std::env::var("ACCESS_TOKEN_EXPIRY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900),
            refresh_token_expiry: std::env::var("REFRESH_TOKEN_EXPIRY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2592000),
            tmdb_api_key,
            tvdb_api_key,
            image_cache_dir,
            stream_tokens: Arc::new(RwLock::new(HashMap::new())),
            active_streams: Arc::new(RwLock::new(HashMap::new())),
            max_concurrent_streams,
            max_bandwidth_bytes_per_sec,
            ffmpeg_path,
            ffprobe_path,
            transcode_cache_dir,
            transcode_manager,
            subtitle_cache_dir,
            subtitle_cache_max_bytes,
            subtitle_manager,
        }
    }
}

async fn load_library_roots(db: &DbPool) -> Vec<PathBuf> {
    let rows: Vec<(String,)> =
        match sqlx::query_as("SELECT DISTINCT path FROM library_paths WHERE is_enabled = 1")
            .fetch_all(db)
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("Failed to load library_paths for subtitle security: {}", e);
                return Vec::new();
            }
        };
    rows.into_iter()
        .filter_map(|(p,)| {
            let path = PathBuf::from(&p);
            if path.exists() {
                Some(path)
            } else {
                tracing::warn!(
                    "Library path '{}' does not exist; skipping for subtitle roots",
                    p
                );
                None
            }
        })
        .collect()
}
