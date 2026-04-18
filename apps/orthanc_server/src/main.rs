mod api;
mod auth;
mod db;
mod metadata;
mod models;
mod scanner;

use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orthanc_server=debug,info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("Starting Orthanc server");

    let db_config = db::DbConfig::from_env();
    info!("Database URL: {}", db_config.database_url);

    let pool = db::init_pool(&db_config).await?;
    db::run_migrations(&pool).await?;
    db::health_check(&pool).await?;
    info!("Database connection verified");

    api::libraries::create_default_libraries(&pool).await?;

    let state = Arc::new(api::AppState::from_env(pool.clone()));

    // Spawn background scanner that runs on an interval
    let scan_pool = pool.clone();
    let scan_api_key = state.tmdb_api_key.clone();
    let scan_cache_dir = state.image_cache_dir.clone();
    tokio::spawn(async move {
        scanner::background_scan_loop(scan_pool, scan_api_key, scan_cache_dir).await;
    });

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let app = api::router(state)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(cors);

    let bind_addr =
        std::env::var("SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    info!("Listening on {}", bind_addr);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Shutting down server");
    db::close_pool(pool).await;

    Ok(())
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for ctrl_c");
}
