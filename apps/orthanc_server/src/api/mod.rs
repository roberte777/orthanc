use axum::Router;
use std::sync::Arc;

pub mod error;
pub mod state;

pub mod auth;
pub mod libraries;
pub mod media;
pub mod metadata_api;
pub mod settings;
pub mod users;

pub use state::AppState;

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/health", axum::routing::get(health))
        .route("/api/setup-status", axum::routing::get(setup_status))
        .nest("/api/auth", auth::router())
        .nest("/api/admin/users", users::router())
        .nest("/api/settings", settings::user_router())
        .nest("/api/admin/settings", settings::admin_router())
        .nest("/api/admin/libraries", libraries::router())
        .nest("/api/admin/metadata", metadata_api::router())
        .nest("/api/media", media::router())
        .with_state(state.clone())
        .nest_service(
            "/api/images",
            tower_http::services::ServeDir::new(&state.image_cache_dir),
        )
}

async fn health(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> impl axum::response::IntoResponse {
    match crate::db::health_check(&state.db).await {
        Ok(_) => axum::Json(serde_json::json!({"status": "ok"})),
        Err(_) => axum::Json(serde_json::json!({"status": "error"})),
    }
}

async fn setup_status(
    axum::extract::State(state): axum::extract::State<Arc<AppState>>,
) -> impl axum::response::IntoResponse {
    let count: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap_or((0,));
    axum::Json(serde_json::json!({"needs_setup": count.0 == 0}))
}
