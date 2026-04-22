//! A direct connection to a sqlite db

use std::{path::Path, str::FromStr, time::Duration};

use anyhow::{Context, Result};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use tracing::info;

/// Database configuration
#[derive(Debug, Clone)]
pub struct DbConfig {
    /// Database file path
    pub database_url: String,
    /// Maximum number of connections in the pool
    pub max_connections: u32,
    /// Minimum number of idle connections
    pub min_connections: u32,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Maximum connection lifetime
    pub max_lifetime: Duration,
    /// Enable WAL mode for better concurrency
    pub enable_wal: bool,
}

impl Default for DbConfig {
    fn default() -> Self {
        Self {
            database_url: "sqlite:./orthanc.db".to_string(),
            max_connections: 10,
            min_connections: 2,
            connect_timeout: Duration::from_secs(30),
            max_lifetime: Duration::from_mins(30),
            enable_wal: true,
        }
    }
}

impl DbConfig {
    /// Create a new database configuration from environment variables
    pub fn from_env() -> Self {
        Self {
            database_url: std::env::var("DATABASE_URL")
                .unwrap_or_else(|_| "sqlite:./orthanc.db".to_string()),
            max_connections: std::env::var("DATABASE_MAX_CONNECTIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(10),
            min_connections: std::env::var("DATABASE_MIN_CONNECTIONS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(2),
            connect_timeout: std::env::var("DATABASE_CONNECT_TIMEOUT")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_secs(30)),
            max_lifetime: std::env::var("DATABASE_MAX_LIFETIME")
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .map(Duration::from_secs)
                .unwrap_or(Duration::from_mins(30)),
            enable_wal: std::env::var("DATABASE_ENABLE_WAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(true),
        }
    }

    /// Get the database file path from the database URL
    pub fn database_path(&self) -> Result<String> {
        let path = self
            .database_url
            .strip_prefix("sqlite:")
            .context("Invalid database URL: must start with 'sqlite:'")?;
        Ok(path.to_string())
    }
}

/// Initialize the database connection pool
///
/// This function:
/// - Creates the database file if it doesn't exist
/// - Configures SQLite for optimal performance
/// - Sets up connection pooling
/// - Does NOT run migrations (use `run_migrations` separately)
async fn init_pool(config: &DbConfig) -> Result<SqlitePool> {
    info!("Initializing database connection pool");

    // Extract database path
    let db_path = config.database_path()?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = Path::new(&db_path).parent()
        && !parent.exists()
    {
        info!("Creating database directory: {}", parent.display());
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
    }

    let mut connect_options = SqliteConnectOptions::from_str(&config.database_url)?
        .create_if_missing(true)
        .busy_timeout(Duration::from_secs(30));

    if config.enable_wal {
        connect_options = connect_options
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal);
        info!("WAL mode enabled for better concurrent access");
    }

    // Create connection pool
    let pool = SqlitePoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(config.connect_timeout)
        .max_lifetime(config.max_lifetime)
        .connect_with(connect_options)
        .await
        .context("Failed to create database connection pool")?;

    info!(
        "Database pool initialized (max: {}, min: {})",
        config.max_connections, config.min_connections
    );

    Ok(pool)
}

/// Run database migrations
///
/// This applies all pending migrations from the `migrations/` directory.
/// Should be called after `init_pool` and before starting the server.
async fn run_migrations(pool: &SqlitePool) -> Result<()> {
    info!("Running database migrations");

    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .context("Failed to run database migrations")?;

    info!("Database migrations completed successfully");
    Ok(())
}

/// Initialize the database pool and run migrations
pub async fn db_init(config: &DbConfig) -> Result<SqlitePool> {
    let pool = init_pool(config).await?;
    run_migrations(&pool).await?;
    Ok(pool)
}

/// Check database health
///
/// Performs a simple query to verify database connectivity.
/// Useful for health check endpoints.
pub async fn health_check(pool: &SqlitePool) -> Result<()> {
    sqlx::query("SELECT 1")
        .fetch_one(pool)
        .await
        .context("Database health check failed")?;
    Ok(())
}

/// Close the database connection pool gracefully
pub async fn close_pool(pool: SqlitePool) {
    info!("Closing database connection pool");
    pool.close().await;
    info!("Database connection pool closed");
}

/// Database statistics for monitoring
#[derive(Debug)]
pub struct PoolStats {
    pub connections: u32,
    pub idle_connections: u32,
}

/// Get current pool statistics
pub fn pool_stats(pool: &SqlitePool) -> PoolStats {
    PoolStats {
        connections: pool.size(),
        idle_connections: pool.num_idle() as u32,
    }
}
