pub mod parser;
pub mod sidecar;
pub mod walker;

use crate::db::DbPool;
use crate::models::library::Library;
use parser::ParsedMedia;
use serde::Deserialize;
use tracing::{debug, info, warn};

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
/// Also triggers metadata refresh for newly added items when a TMDB API key is available.
pub async fn background_scan_loop(
    db: DbPool,
    tmdb_api_key: Option<String>,
    tvdb_api_key: String,
    image_cache_dir: String,
    ffprobe_path: String,
) {
    // Backfill stream info for items scanned before ffprobe integration
    backfill_streams(&db, &ffprobe_path).await;
    // Backfill sidecar subtitles for items that predate sidecar support.
    backfill_external_subtitles(&db).await;

    // Initial scan on startup
    info!("Running initial library scan");
    scan_all_due_libraries(&db, tmdb_api_key.as_deref(), &tvdb_api_key, &image_cache_dir, &ffprobe_path).await;

    // Then check every 60 seconds which libraries need scanning
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
    loop {
        interval.tick().await;
        scan_all_due_libraries(&db, tmdb_api_key.as_deref(), &tvdb_api_key, &image_cache_dir, &ffprobe_path).await;
    }
}

async fn scan_all_due_libraries(
    db: &DbPool,
    tmdb_api_key: Option<&str>,
    tvdb_api_key: &str,
    image_cache_dir: &str,
    ffprobe_path: &str,
) {
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

    // Admin-controlled default scan interval, applied when a library row has no
    // explicit `scan_interval_minutes`. Falls back to the hardcoded default if unset.
    let default_interval = crate::api::settings::read_setting(db, "library_scan_interval_minutes")
        .await
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&m| m > 0)
        .unwrap_or(DEFAULT_SCAN_INTERVAL_MINUTES);

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

        // Determine scan interval: per-library override > admin default > hardcoded fallback.
        let interval_mins = lib
            .scan_interval_minutes
            .filter(|&m| m > 0)
            .map(|m| m as u64)
            .unwrap_or(default_interval);

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
        match scan_library(db, lib, ffprobe_path).await {
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

        // Always try metadata refresh if we have an API key — standard mode
        // only fills missing fields so it's cheap for already-enriched items.
        if let Some(api_key) = tmdb_api_key {
            // Check if any top-level items are missing metadata
            let (unscanned,): (i64,) = sqlx::query_as(
                "SELECT COUNT(*) FROM media_items WHERE library_id = ? AND media_type IN ('movie', 'tv_show') AND tmdb_id IS NULL",
            )
            .bind(lib.id)
            .fetch_one(db)
            .await
            .unwrap_or((0,));

            if unscanned > 0 {
                info!("Refreshing metadata for {} unscanned items in '{}'", unscanned, lib.name);
                if let Err(e) = crate::metadata::refresh_library(
                    db,
                    api_key,
                    tvdb_api_key,
                    image_cache_dir,
                    lib.id,
                    crate::metadata::RefreshMode::Standard,
                )
                .await
                {
                    warn!("Metadata refresh failed for '{}': {}", lib.name, e);
                }
            }
        }
    }
}

/// Scan a single library: walk all its paths, parse filenames, and upsert into media_items.
pub async fn scan_library(db: &DbPool, library: &Library, ffprobe_path: &str) -> Result<ScanResult, anyhow::Error> {
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
                Ok(Some(new_id)) => {
                    result.added += 1;
                    // Probe streams for newly added items
                    let path_str = file_info.path.to_string_lossy();
                    probe_and_store_streams(db, new_id, &path_str, ffprobe_path).await;
                    sync_external_subtitles(db, new_id, &file_info.path).await;
                }
                Ok(None) => result.unchanged += 1,
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

/// Insert or skip a media item. Returns Ok(Some(id)) if inserted, Ok(None) if already existed.
async fn upsert_media(
    db: &DbPool,
    library: &Library,
    file_info: &walker::FileInfo,
    parsed: &ParsedMedia,
) -> Result<Option<i64>, anyhow::Error> {
    let file_path_str = file_info.path.to_string_lossy().to_string();

    // Check if already scanned
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM media_items WHERE file_path = ?")
            .bind(&file_path_str)
            .fetch_optional(db)
            .await?;

    if existing.is_some() {
        return Ok(None);
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

            let result = sqlx::query(
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
            Ok(Some(result.last_insert_rowid()))
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

            let result = sqlx::query(
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
            Ok(Some(result.last_insert_rowid()))
        }
        ParsedMedia::Unknown { filename } => {
            // Store it as a movie with the raw filename as title
            let sort_title = make_sort_title(filename);
            let result = sqlx::query(
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
            Ok(Some(result.last_insert_rowid()))
        }
    }
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

// ---------------------------------------------------------------------------
// ffprobe integration
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct FfprobeOutput {
    streams: Vec<FfprobeStream>,
    format: Option<FfprobeFormat>,
}

#[derive(Debug, Deserialize)]
struct FfprobeStream {
    index: i32,
    codec_type: Option<String>,
    codec_name: Option<String>,
    width: Option<i32>,
    height: Option<i32>,
    display_aspect_ratio: Option<String>,
    r_frame_rate: Option<String>,
    #[serde(default)]
    bits_per_raw_sample: Option<String>,
    color_space: Option<String>,
    channels: Option<i32>,
    sample_rate: Option<String>,
    bit_rate: Option<String>,
    #[serde(default)]
    tags: Option<FfprobeTags>,
    #[serde(default)]
    disposition: Option<FfprobeDisposition>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeTags {
    language: Option<String>,
    title: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct FfprobeDisposition {
    default: Option<i32>,
    forced: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct FfprobeFormat {
    duration: Option<String>,
}

/// Run ffprobe on a file and populate the media_streams table.
/// Also updates duration_seconds on the media_item if available.
pub async fn probe_and_store_streams(
    db: &DbPool,
    media_item_id: i64,
    file_path: &str,
    ffprobe_path: &str,
) {
    let output = match tokio::process::Command::new(ffprobe_path)
        .args([
            "-v", "quiet",
            "-print_format", "json",
            "-show_streams",
            "-show_format",
            file_path,
        ])
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => {
            warn!("Failed to run ffprobe on '{}': {}", file_path, e);
            return;
        }
    };

    if !output.status.success() {
        warn!(
            "ffprobe failed for '{}': {}",
            file_path,
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }

    let probe: FfprobeOutput = match serde_json::from_slice(&output.stdout) {
        Ok(p) => p,
        Err(e) => {
            warn!("Failed to parse ffprobe output for '{}': {}", file_path, e);
            return;
        }
    };

    // Delete existing embedded streams for this item. External (sidecar)
    // subtitle rows are preserved — they're managed by sidecar discovery.
    if let Err(e) = sqlx::query(
        "DELETE FROM media_streams WHERE media_item_id = ? AND is_external = 0",
    )
    .bind(media_item_id)
    .execute(db)
    .await
    {
        warn!("Failed to clear old media_streams: {}", e);
        return;
    }

    // Insert streams
    for s in &probe.streams {
        let stream_type = match s.codec_type.as_deref() {
            Some("video") => "video",
            Some("audio") => "audio",
            Some("subtitle") => "subtitle",
            _ => continue,
        };

        let tags = s.tags.as_ref();
        let disp = s.disposition.as_ref();
        let frame_rate = s.r_frame_rate.as_deref().and_then(parse_frame_rate);
        let bit_depth = s
            .bits_per_raw_sample
            .as_deref()
            .and_then(|b| b.parse::<i32>().ok());
        let sample_rate = s
            .sample_rate
            .as_deref()
            .and_then(|r| r.parse::<i32>().ok());
        let bit_rate = s
            .bit_rate
            .as_deref()
            .and_then(|r| r.parse::<i32>().ok());

        if let Err(e) = sqlx::query(
            "INSERT INTO media_streams (media_item_id, stream_index, stream_type, codec, language, title, is_default, is_forced, width, height, aspect_ratio, frame_rate, bit_depth, color_space, channels, sample_rate, bit_rate)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(media_item_id)
        .bind(s.index)
        .bind(stream_type)
        .bind(&s.codec_name)
        .bind(tags.and_then(|t| t.language.as_deref()))
        .bind(tags.and_then(|t| t.title.as_deref()))
        .bind(disp.and_then(|d| d.default).unwrap_or(0) != 0)
        .bind(disp.and_then(|d| d.forced).unwrap_or(0) != 0)
        .bind(s.width)
        .bind(s.height)
        .bind(&s.display_aspect_ratio)
        .bind(frame_rate)
        .bind(bit_depth)
        .bind(&s.color_space)
        .bind(s.channels)
        .bind(sample_rate)
        .bind(bit_rate)
        .execute(db)
        .await
        {
            warn!("Failed to insert media_stream: {}", e);
        }
    }

    // Update duration from format
    if let Some(ref fmt) = probe.format {
        if let Some(ref dur_str) = fmt.duration {
            if let Ok(dur) = dur_str.parse::<f64>() {
                let _ = sqlx::query(
                    "UPDATE media_items SET duration_seconds = ? WHERE id = ? AND (duration_seconds IS NULL OR duration_seconds = 0)",
                )
                .bind(dur as i32)
                .bind(media_item_id)
                .execute(db)
                .await;
            }
        }
    }

    debug!(
        "Stored {} streams for media_item {}",
        probe.streams.len(),
        media_item_id
    );
}

/// Parse ffprobe frame rate fraction like "24000/1001" into a float.
fn parse_frame_rate(rate: &str) -> Option<f64> {
    let mut parts = rate.split('/');
    let num: f64 = parts.next()?.parse().ok()?;
    let den: f64 = parts.next().and_then(|d| d.parse().ok()).unwrap_or(1.0);
    if den == 0.0 {
        return None;
    }
    Some(num / den)
}

/// Discover sidecar subtitle files for a media item and sync them into the
/// `media_streams` table. Rows for sidecars that no longer exist on disk are
/// removed; rows for newly discovered files are inserted. Existing rows are
/// left untouched (cached VTT extractions keyed by their id remain valid).
///
/// Sidecar streams use synthetic `stream_index` values starting at 1000 to
/// avoid colliding with embedded ffprobe stream indices.
pub async fn sync_external_subtitles(db: &DbPool, media_item_id: i64, video_path: &std::path::Path) {
    let discovered = sidecar::discover_sidecars(video_path);

    // Load currently-indexed external subtitles for this item
    let existing: Vec<(i64, Option<String>)> = match sqlx::query_as(
        "SELECT id, external_file_path FROM media_streams WHERE media_item_id = ? AND is_external = 1",
    )
    .bind(media_item_id)
    .fetch_all(db)
    .await
    {
        Ok(rows) => rows,
        Err(e) => {
            warn!("Failed to load existing external subtitles for {}: {}", media_item_id, e);
            return;
        }
    };

    let discovered_paths: std::collections::HashSet<String> = discovered
        .iter()
        .map(|s| s.path.to_string_lossy().to_string())
        .collect();
    let existing_paths: std::collections::HashSet<String> = existing
        .iter()
        .filter_map(|(_, p)| p.clone())
        .collect();

    // Remove rows for sidecars that have been deleted from disk
    for (row_id, path) in &existing {
        if let Some(p) = path {
            if !discovered_paths.contains(p) {
                if let Err(e) = sqlx::query("DELETE FROM media_streams WHERE id = ?")
                    .bind(row_id)
                    .execute(db)
                    .await
                {
                    warn!("Failed to delete stale external subtitle row {}: {}", row_id, e);
                }
            }
        }
    }

    // Find the highest existing synthetic index so we can append without collisions
    let base_index: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(stream_index), 999) + 1 FROM media_streams WHERE media_item_id = ? AND is_external = 1",
    )
    .bind(media_item_id)
    .fetch_one(db)
    .await
    .unwrap_or(1000);
    let mut next_index = base_index.max(1000);

    for info in &discovered {
        let path_str = info.path.to_string_lossy().to_string();
        if existing_paths.contains(&path_str) {
            continue;
        }
        let codec = info.format.codec();
        let title = info.title.clone();
        if let Err(e) = sqlx::query(
            "INSERT INTO media_streams (media_item_id, stream_index, stream_type, codec, language, title, is_default, is_forced, is_external, external_file_path)
             VALUES (?, ?, 'subtitle', ?, ?, ?, ?, ?, 1, ?)",
        )
        .bind(media_item_id)
        .bind(next_index as i32)
        .bind(codec)
        .bind(&info.language)
        .bind(&title)
        .bind(info.is_default)
        .bind(info.is_forced)
        .bind(&path_str)
        .execute(db)
        .await
        {
            warn!("Failed to insert external subtitle row for {}: {}", path_str, e);
        } else {
            next_index += 1;
        }
    }

    if !discovered.is_empty() {
        debug!(
            "Synced {} external subtitle sidecar(s) for media_item {}",
            discovered.len(),
            media_item_id
        );
    }
}

/// Scan every existing media item for sidecar subtitles and sync them into
/// `media_streams`. Used on startup so libraries that predate sidecar support
/// pick up their external subtitles without a full re-scan.
pub async fn backfill_external_subtitles(db: &DbPool) {
    let items: Vec<(i64, String)> = match sqlx::query_as(
        "SELECT id, file_path FROM media_items WHERE file_path IS NOT NULL AND media_type IN ('movie', 'episode')",
    )
    .fetch_all(db)
    .await
    {
        Ok(items) => items,
        Err(e) => {
            warn!("Failed to query items for sidecar backfill: {}", e);
            return;
        }
    };

    if items.is_empty() {
        return;
    }

    let mut scanned = 0usize;
    for (id, path) in &items {
        let p = std::path::Path::new(path);
        if !p.is_file() {
            continue;
        }
        sync_external_subtitles(db, *id, p).await;
        scanned += 1;
    }
    info!("Sidecar subtitle backfill complete: scanned {} item(s)", scanned);
}

/// Backfill media_streams for items that were scanned before ffprobe integration.
pub async fn backfill_streams(db: &DbPool, ffprobe_path: &str) {
    let items: Vec<(i64, String)> = match sqlx::query_as(
        "SELECT mi.id, mi.file_path FROM media_items mi
         WHERE mi.file_path IS NOT NULL
         AND mi.id NOT IN (SELECT DISTINCT media_item_id FROM media_streams)",
    )
    .fetch_all(db)
    .await
    {
        Ok(items) => items,
        Err(e) => {
            warn!("Failed to query items for stream backfill: {}", e);
            return;
        }
    };

    if items.is_empty() {
        return;
    }

    info!("Backfilling stream info for {} media items", items.len());
    for (id, path) in &items {
        probe_and_store_streams(db, *id, path, ffprobe_path).await;
    }
    info!("Stream backfill complete");
}
