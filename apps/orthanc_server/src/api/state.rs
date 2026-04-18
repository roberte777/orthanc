use crate::db::DbPool;

pub struct AppState {
    pub db: DbPool,
    pub jwt_secret: String,
    pub access_token_expiry: u64,  // seconds
    pub refresh_token_expiry: u64, // seconds
    pub tmdb_api_key: Option<String>,
    pub image_cache_dir: String,
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

        let image_cache_dir = std::env::var("IMAGE_CACHE_DIR")
            .unwrap_or_else(|_| "./image_cache".to_string());

        // Ensure image cache directory exists
        if let Err(e) = std::fs::create_dir_all(&image_cache_dir) {
            tracing::error!("Failed to create image cache dir '{}': {}", image_cache_dir, e);
        }

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
            image_cache_dir,
        }
    }
}
