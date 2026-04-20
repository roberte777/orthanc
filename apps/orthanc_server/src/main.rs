mod api;
mod auth;
mod db;
mod metadata;
mod models;
mod scanner;
mod subtitles;
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

    let state = Arc::new(api::AppState::from_env_async(pool.clone()).await);

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

    // Spawn transcode cleanup task — sweep every 30s, kill sessions idle for 60s.
    // HLS segment requests update last_accessed, acting as a natural heartbeat.
    let cleanup_manager = state.transcode_manager.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            cleanup_manager
                .cleanup_stale(std::time::Duration::from_secs(60))
                .await;
        }
    });

    // Spawn subtitle cache sweeper — every 6 hours, remove orphaned cached
    // WebVTT files and enforce the configured size cap.
    {
        let subtitle_dir = std::path::PathBuf::from(&state.subtitle_cache_dir);
        let max_bytes = state.subtitle_cache_max_bytes;
        let db = pool.clone();

        // Startup sweep: remove orphans left over from a previous run.
        {
            let subtitle_dir = subtitle_dir.clone();
            let db = db.clone();
            let live_ids = sqlx::query_as::<_, (i64,)>(
                "SELECT id FROM media_streams WHERE stream_type = 'subtitle'",
            )
            .fetch_all(&db)
            .await
            .map(|rows| rows.into_iter().map(|(id,)| id).collect::<std::collections::HashSet<_>>())
            .unwrap_or_default();
            tokio::task::spawn_blocking(move || {
                subtitles::cleanup::full_sweep(&subtitle_dir, &live_ids, max_bytes);
            })
            .await
            .ok();
        }

        tokio::spawn(async move {
            subtitles::cleanup::run_cleanup_loop(
                subtitle_dir,
                db,
                max_bytes,
                std::time::Duration::from_secs(6 * 3600),
            )
            .await;
        });
    }

    let cors = tower_http::cors::CorsLayer::new()
        .allow_origin(tower_http::cors::Any)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any);

    let state_for_shutdown = state.clone();
    let app = api::router(state)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(cors);

    let bind_addr =
        std::env::var("SERVER_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
    info!("Listening on {}", bind_addr);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    let shutdown_manager = state_for_shutdown.transcode_manager.clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Shutting down server");
    shutdown_manager.stop_all().await;
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
