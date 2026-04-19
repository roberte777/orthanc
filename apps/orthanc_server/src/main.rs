mod api;
mod auth;
mod db;
mod metadata;
mod models;
mod scanner;
mod transcoding;

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
    seed_default_profiles(&pool).await?;

    let state = Arc::new(api::AppState::from_env(pool.clone()));

    // Spawn background scanner that runs on an interval
    let scan_pool = pool.clone();
    let scan_api_key = state.tmdb_api_key.clone();
    let scan_tvdb_key = state.tvdb_api_key.clone();
    let scan_cache_dir = state.image_cache_dir.clone();
    let scan_ffprobe = state.ffprobe_path.clone();
    tokio::spawn(async move {
        scanner::background_scan_loop(
            scan_pool,
            scan_api_key,
            scan_tvdb_key,
            scan_cache_dir,
            scan_ffprobe,
        )
        .await;
    });

    // Spawn transcode cleanup task
    let cleanup_manager = state.transcode_manager.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            cleanup_manager
                .cleanup_stale(std::time::Duration::from_secs(300))
                .await;
        }
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

async fn seed_default_profiles(pool: &db::DbPool) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT OR IGNORE INTO transcoding_profiles (name, description, container_format, video_codec, video_bitrate_kbps, video_width, video_height, audio_codec, audio_bitrate_kbps, audio_channels, is_default)
         VALUES ('1080p', 'Full HD', 'hls', 'h264', 8000, 1920, 1080, 'aac', 192, 2, 1)"
    ).execute(pool).await?;

    sqlx::query(
        "INSERT OR IGNORE INTO transcoding_profiles (name, description, container_format, video_codec, video_bitrate_kbps, video_width, video_height, audio_codec, audio_bitrate_kbps, audio_channels, is_default)
         VALUES ('720p', 'HD', 'hls', 'h264', 4000, 1280, 720, 'aac', 128, 2, 0)"
    ).execute(pool).await?;

    sqlx::query(
        "INSERT OR IGNORE INTO transcoding_profiles (name, description, container_format, video_codec, video_bitrate_kbps, video_width, video_height, audio_codec, audio_bitrate_kbps, audio_channels, is_default)
         VALUES ('480p', 'SD', 'hls', 'h264', 2000, 854, 480, 'aac', 128, 2, 0)"
    ).execute(pool).await?;

    Ok(())
}
