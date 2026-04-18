use crate::db::DbPool;

pub struct AppState {
    pub db: DbPool,
    pub jwt_secret: String,
    pub access_token_expiry: u64,  // seconds
    pub refresh_token_expiry: u64, // seconds
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
        Self {
            db,
            jwt_secret,
            access_token_expiry: std::env::var("ACCESS_TOKEN_EXPIRY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900), // 15 minutes
            refresh_token_expiry: std::env::var("REFRESH_TOKEN_EXPIRY")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2592000), // 30 days
        }
    }
}
