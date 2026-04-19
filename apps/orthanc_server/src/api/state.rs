use crate::db::DbPool;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Data for a short-lived streaming token.
pub struct StreamTokenData {
    pub user_id: i64,
    pub media_item_id: i64,
    pub expires_at: chrono::DateTime<chrono::Utc>,
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
}

impl AppState {
    pub fn from_env(db: DbPool) -> Self {
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

        let tmdb_api_key = Some(
            std::env::var("TMDB_API_KEY").unwrap_or_else(|_| DEFAULT_TMDB_API_KEY.to_string()),
        );

        // Embedded TVDB project API key (like Jellyfin's TvdbPlugin). Users may
        // override with their own subscriber key via TVDB_API_KEY.
        const DEFAULT_TVDB_API_KEY: &str = "91090b09-8411-4b64-834a-733ab3f12a07";

        let tvdb_api_key =
            std::env::var("TVDB_API_KEY").unwrap_or_else(|_| DEFAULT_TVDB_API_KEY.to_string());

        let image_cache_dir = std::env::var("IMAGE_CACHE_DIR")
            .unwrap_or_else(|_| "./image_cache".to_string());

        // Ensure image cache directory exists
        if let Err(e) = std::fs::create_dir_all(&image_cache_dir) {
            tracing::error!("Failed to create image cache dir '{}': {}", image_cache_dir, e);
        }

        let max_concurrent_streams = std::env::var("MAX_CONCURRENT_STREAMS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);

        let max_bandwidth_bytes_per_sec = std::env::var("MAX_BANDWIDTH_MBPS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(|mbps| mbps * 1_000_000);

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
        }
    }
}
