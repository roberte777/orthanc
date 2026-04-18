pub mod parser;
pub mod walker;

use crate::db::DbPool;
use crate::models::library::Library;
use parser::ParsedMedia;
use tracing::{info, warn};

/// Video file extensions we recognize
const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "avi", "mov", "wmv", "flv", "webm", "m4v", "mpg", "mpeg", "ts", "m2ts",
    "vob", "ogv", "3gp", "divx", "xvid",
];

pub fn is_video_file(path: &std::path::Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| VIDEO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

/// Default scan interval in minutes when a library doesn't specify one.
const DEFAULT_SCAN_INTERVAL_MINUTES: u64 = 30;

/// Background loop: scans on startup, then re-checks every minute which libraries
/// are due for a scan based on their `scan_interval_minutes` and `last_scan_at`.
pub async fn background_scan_loop(db: DbPool) {
    // Initial scan on startup
    info!("Running initial library scan");
    scan_all_due_libraries(&db).await;

    // Then check every 60 seconds which libraries need scanning
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;
        scan_all_due_libraries(&db).await;
    }
}

async fn scan_all_due_libraries(db: &DbPool) {
    let libraries = match sqlx::query_as::<_, Library>(
        "SELECT * FROM libraries WHERE is_enabled = 1",
    )
    .fetch_all(db)
    .await
    {
        Ok(libs) => libs,
        Err(e) => {
            warn!("Failed to load libraries for background scan: {}", e);
            return;
        }
    };

    for lib in &libraries {
        // Check if library has paths
        let path_count: (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM library_paths WHERE library_id = ? AND is_enabled = 1",
        )
        .bind(lib.id)
        .fetch_one(db)
        .await
        .unwrap_or((0,));

        if path_count.0 == 0 {
            continue;
        }

        // Determine scan interval
        let interval_mins = lib
            .scan_interval_minutes
            .filter(|&m| m > 0)
            .map(|m| m as u64)
            .unwrap_or(DEFAULT_SCAN_INTERVAL_MINUTES);

        // Check if enough time has passed since last scan
        if let Some(ref last_scan) = lib.last_scan_at {
            if let Ok(last) = chrono::NaiveDateTime::parse_from_str(last_scan, "%Y-%m-%d %H:%M:%S")
            {
                let now = chrono::Utc::now().naive_utc();
                let elapsed = now.signed_duration_since(last);
                if elapsed.num_minutes() < interval_mins as i64 {
                    continue;
                }
            }
        }

        info!("Background scan: scanning '{}'", lib.name);
        match scan_library(db, lib).await {
            Ok(result) => {
                if result.added > 0 {
                    info!(
                        "Background scan '{}': {} added, {} unchanged",
                        lib.name, result.added, result.unchanged
                    );
                }
            }
            Err(e) => {
                warn!("Background scan failed for '{}': {}", lib.name, e);
            }
        }
    }
}

/// Scan a single library: walk all its paths, parse filenames, and upsert into media_items.
pub async fn scan_library(db: &DbPool, library: &Library) -> Result<ScanResult, anyhow::Error> {
    let paths = sqlx::query_as::<_, crate::models::library::LibraryPath>(
        "SELECT * FROM library_paths WHERE library_id = ? AND is_enabled = 1",
    )
    .bind(library.id)
    .fetch_all(db)
    .await?;

    let mut result = ScanResult::default();

    for lib_path in &paths {
        let root = std::path::Path::new(&lib_path.path);
        if !root.is_dir() {
            warn!("Library path is not a directory, skipping: {}", lib_path.path);
            result.errors.push(format!("Not a directory: {}", lib_path.path));
            continue;
        }

        let files = walker::walk_directory(root);
        info!(
            "Found {} video files in {}",
            files.len(),
            lib_path.path
        );

        for file_info in files {
            let parsed = parser::parse_media_file(&file_info, &library.library_type);

            match upsert_media(db, library, &file_info, &parsed).await {
                Ok(true) => result.added += 1,
                Ok(false) => result.unchanged += 1,
                Err(e) => {
                    warn!("Failed to upsert media: {:?} - {}", file_info.path, e);
                    result.errors.push(format!("{}: {}", file_info.path.display(), e));
                }
            }
        }
    }

    // Update last_scan_at
    sqlx::query("UPDATE libraries SET last_scan_at = datetime('now') WHERE id = ?")
        .bind(library.id)
        .execute(db)
        .await?;

    info!(
        "Scan complete for '{}': {} added, {} unchanged, {} errors",
        library.name, result.added, result.unchanged, result.errors.len()
    );

    Ok(result)
}

#[derive(Debug, Default, serde::Serialize)]
pub struct ScanResult {
    pub added: u32,
    pub unchanged: u32,
    pub errors: Vec<String>,
}

/// Insert or skip a media item. Returns Ok(true) if inserted, Ok(false) if already existed.
async fn upsert_media(
    db: &DbPool,
    library: &Library,
    file_info: &walker::FileInfo,
    parsed: &ParsedMedia,
) -> Result<bool, anyhow::Error> {
    let file_path_str = file_info.path.to_string_lossy().to_string();

    // Check if already scanned
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM media_items WHERE file_path = ?")
            .bind(&file_path_str)
            .fetch_optional(db)
            .await?;

    if existing.is_some() {
        return Ok(false);
    }

    let ext = file_info
        .path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match parsed {
        ParsedMedia::Movie {
            title,
            year,
            part,
            ..
        } => {
            let display_title = if let Some(p) = part {
                format!("{} (Part {})", title, p)
            } else {
                title.clone()
            };

            let sort_title = make_sort_title(&display_title);

            sqlx::query(
                "INSERT INTO media_items (library_id, media_type, title, sort_title, release_date, file_path, file_size_bytes, container_format, date_modified)
                 VALUES (?, 'movie', ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(library.id)
            .bind(&display_title)
            .bind(&sort_title)
            .bind(year.map(|y| format!("{}-01-01", y)))
            .bind(&file_path_str)
            .bind(file_info.size as i64)
            .bind(&ext)
            .bind(&file_info.modified)
            .execute(db)
            .await?;
        }
        ParsedMedia::Episode {
            show_name,
            season,
            episode,
            episode_title,
            year,
            ..
        } => {
            // Find or create the TV show
            let show_id = find_or_create_show(db, library, show_name, *year).await?;

            // Find or create the season
            let season_id = find_or_create_season(db, show_id, *season).await?;

            // Insert the episode
            let ep_title = episode_title
                .clone()
                .unwrap_or_else(|| format!("Episode {}", episode));
            let sort_title = make_sort_title(&ep_title);

            sqlx::query(
                "INSERT INTO media_items (media_type, title, sort_title, parent_id, season_number, episode_number, file_path, file_size_bytes, container_format, date_modified)
                 VALUES ('episode', ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(&ep_title)
            .bind(&sort_title)
            .bind(season_id)
            .bind(season)
            .bind(episode)
            .bind(&file_path_str)
            .bind(file_info.size as i64)
            .bind(&ext)
            .bind(&file_info.modified)
            .execute(db)
            .await?;
        }
        ParsedMedia::Unknown { filename } => {
            // Store it as a movie with the raw filename as title
            let sort_title = make_sort_title(filename);
            sqlx::query(
                "INSERT INTO media_items (library_id, media_type, title, sort_title, file_path, file_size_bytes, container_format, date_modified)
                 VALUES (?, 'movie', ?, ?, ?, ?, ?, ?)",
            )
            .bind(library.id)
            .bind(filename)
            .bind(&sort_title)
            .bind(&file_path_str)
            .bind(file_info.size as i64)
            .bind(&ext)
            .bind(&file_info.modified)
            .execute(db)
            .await?;
        }
    }

    Ok(true)
}

async fn find_or_create_show(
    db: &DbPool,
    library: &Library,
    name: &str,
    year: Option<u16>,
) -> Result<i64, anyhow::Error> {
    // Try to find existing show by name in this library
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM media_items WHERE library_id = ? AND media_type = 'tv_show' AND title = ?",
    )
    .bind(library.id)
    .bind(name)
    .fetch_optional(db)
    .await?;

    if let Some((id,)) = existing {
        return Ok(id);
    }

    let sort_title = make_sort_title(name);
    let result = sqlx::query(
        "INSERT INTO media_items (library_id, media_type, title, sort_title, release_date)
         VALUES (?, 'tv_show', ?, ?, ?)",
    )
    .bind(library.id)
    .bind(name)
    .bind(&sort_title)
    .bind(year.map(|y| format!("{}-01-01", y)))
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}

async fn find_or_create_season(
    db: &DbPool,
    show_id: i64,
    season_number: u32,
) -> Result<i64, anyhow::Error> {
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM media_items WHERE parent_id = ? AND media_type = 'season' AND season_number = ?",
    )
    .bind(show_id)
    .bind(season_number)
    .fetch_optional(db)
    .await?;

    if let Some((id,)) = existing {
        return Ok(id);
    }

    let title = format!("Season {}", season_number);
    let result = sqlx::query(
        "INSERT INTO media_items (media_type, title, sort_title, parent_id, season_number)
         VALUES ('season', ?, ?, ?, ?)",
    )
    .bind(&title)
    .bind(&title)
    .bind(show_id)
    .bind(season_number)
    .execute(db)
    .await?;

    Ok(result.last_insert_rowid())
}

fn make_sort_title(title: &str) -> String {
    let lower = title.to_lowercase();
    if lower.starts_with("the ") {
        lower[4..].to_string()
    } else if lower.starts_with("a ") {
        lower[2..].to_string()
    } else if lower.starts_with("an ") {
        lower[3..].to_string()
    } else {
        lower
    }
}
