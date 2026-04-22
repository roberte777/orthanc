pub mod anidb;
pub mod tmdb;
pub mod tvdb;

use crate::db::DbPool;
use crate::models::media::MediaItem;
use tmdb::TmdbClient;
use tracing::{debug, info, warn};
use tvdb::TvdbClient;

/// Refresh mode controls how metadata is applied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RefreshMode {
    /// Only fill in missing fields; leave existing data untouched.
    Standard,
    /// Wipe all metadata and re-pull from scratch (overwrites manual edits).
    Full,
}

/// Get enabled providers for a library, sorted by priority (lowest first).
async fn get_providers(db: &DbPool, library_id: i64) -> Result<Vec<String>, anyhow::Error> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT provider FROM library_metadata_providers WHERE library_id = ? AND is_enabled = 1 ORDER BY priority ASC",
    )
    .bind(library_id)
    .fetch_all(db)
    .await?;
    Ok(rows.into_iter().map(|(p,)| p).collect())
}

/// Shared context for provider clients so we don't re-create them per item.
pub struct ProviderClients {
    pub tmdb: Option<TmdbClient>,
    pub anidb: Option<anidb::AnidbClient>,
    pub tvdb: Option<TvdbClient>,
}

impl ProviderClients {
    pub async fn init(tmdb_api_key: &str, tvdb_api_key: &str, providers: &[String]) -> Self {
        let tmdb = if providers.contains(&"tmdb".to_string()) {
            Some(TmdbClient::new(tmdb_api_key))
        } else {
            None
        };

        let anidb = if providers.contains(&"anidb".to_string()) {
            match anidb::AnidbClient::new().await {
                Ok(c) => Some(c),
                Err(e) => {
                    warn!("Failed to initialize AniDB client: {}", e);
                    None
                }
            }
        } else {
            None
        };

        let tvdb = if providers.contains(&"tvdb".to_string()) {
            Some(TvdbClient::new(tvdb_api_key))
        } else {
            None
        };

        Self { tmdb, anidb, tvdb }
    }
}

/// Refresh metadata for a single media item using providers in priority order.
pub async fn refresh_item(
    db: &DbPool,
    api_key: &str,
    tvdb_api_key: &str,
    image_cache_dir: &str,
    item_id: i64,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    let item = sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id = ?")
        .bind(item_id)
        .fetch_optional(db)
        .await?
        .ok_or_else(|| anyhow::anyhow!("Media item {} not found", item_id))?;

    // Determine the library_id (walk up to parent for seasons/episodes)
    let library_id = match item.library_id {
        Some(id) => id,
        None => {
            // For seasons/episodes, find the root show's library
            find_root_library_id(db, &item).await?.unwrap_or(0)
        }
    };

    if library_id == 0 {
        return Ok(());
    }

    let providers = get_providers(db, library_id).await?;
    if providers.is_empty() {
        return Ok(());
    }

    let clients = ProviderClients::init(api_key, tvdb_api_key, &providers).await;

    // Try providers in priority order
    for provider in &providers {
        let result = match provider.as_str() {
            "tmdb" => {
                if let Some(ref tmdb) = clients.tmdb {
                    refresh_item_tmdb(db, tmdb, image_cache_dir, &item, mode).await
                } else {
                    continue;
                }
            }
            "anidb" => {
                if let Some(ref anidb_client) = clients.anidb {
                    refresh_item_anidb(db, anidb_client, image_cache_dir, &item, mode).await
                } else {
                    continue;
                }
            }
            "tvdb" => {
                if let Some(ref tvdb_client) = clients.tvdb {
                    refresh_item_tvdb(db, tvdb_client, image_cache_dir, &item, mode).await
                } else {
                    continue;
                }
            }
            _ => continue,
        };

        match result {
            Ok(true) => {
                debug!("Provider '{}' succeeded for item {}", provider, item_id);
                break; // First successful provider wins
            }
            Ok(false) => {
                debug!(
                    "Provider '{}' found no match for item {}, trying next",
                    provider, item_id
                );
                continue;
            }
            Err(e) => {
                warn!("Provider '{}' failed for item {}: {}", provider, item_id, e);
                continue;
            }
        }
    }

    // Generate video thumbnail if the item has a file but no backdrop/thumbnail image
    generate_video_thumbnail(db, image_cache_dir, &item).await;

    // Mark as scanned
    sqlx::query("UPDATE media_items SET last_scanned_at = datetime('now') WHERE id = ?")
        .bind(item_id)
        .execute(db)
        .await?;

    Ok(())
}

/// Find the root library_id by walking up the parent chain.
async fn find_root_library_id(db: &DbPool, item: &MediaItem) -> Result<Option<i64>, anyhow::Error> {
    let mut current = item.clone();
    for _ in 0..5 {
        if let Some(lib_id) = current.library_id {
            return Ok(Some(lib_id));
        }
        if let Some(parent_id) = current.parent_id {
            current = match sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id = ?")
                .bind(parent_id)
                .fetch_optional(db)
                .await?
            {
                Some(p) => p,
                None => return Ok(None),
            };
        } else {
            return Ok(None);
        }
    }
    Ok(None)
}

/// TMDB refresh for a single item. Returns Ok(true) if matched, Ok(false) if no match.
async fn refresh_item_tmdb(
    db: &DbPool,
    client: &TmdbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    match item.media_type.as_str() {
        "movie" => refresh_movie(db, client, image_cache_dir, item, mode).await,
        "tv_show" => refresh_show(db, client, image_cache_dir, item, mode).await,
        "season" => {
            if let Some(parent_id) = item.parent_id {
                let parent =
                    sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id = ?")
                        .bind(parent_id)
                        .fetch_optional(db)
                        .await?;
                if let Some(parent) = parent
                    && let Some(ref id_str) = parent.tmdb_id
                        && let Ok(tmdb_id) = id_str.parse::<u64>() {
                            return refresh_season(
                                db,
                                client,
                                image_cache_dir,
                                item,
                                tmdb_id,
                                mode,
                            )
                            .await;
                        }
            }
            Ok(false)
        }
        "episode" => {
            if let Some(season_id) = item.parent_id {
                let season =
                    sqlx::query_as::<_, MediaItem>("SELECT * FROM media_items WHERE id = ?")
                        .bind(season_id)
                        .fetch_optional(db)
                        .await?;
                if let Some(season) = season
                    && let Some(show_id) = season.parent_id {
                        let show = sqlx::query_as::<_, MediaItem>(
                            "SELECT * FROM media_items WHERE id = ?",
                        )
                        .bind(show_id)
                        .fetch_optional(db)
                        .await?;
                        if let Some(show) = show
                            && let Some(ref id_str) = show.tmdb_id
                                && let Ok(tmdb_id) = id_str.parse::<u64>() {
                                    let season_num = season.season_number.unwrap_or(1) as u32;
                                    return refresh_episode(
                                        db,
                                        client,
                                        image_cache_dir,
                                        item,
                                        tmdb_id,
                                        season_num,
                                        mode,
                                    )
                                    .await;
                                }
                    }
            }
            Ok(false)
        }
        _ => Ok(false),
    }
}

/// AniDB refresh for a single item. Returns Ok(true) if matched, Ok(false) if no match.
async fn refresh_item_anidb(
    db: &DbPool,
    client: &anidb::AnidbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    // AniDB is anime-focused — only applicable to TV shows and their children
    match item.media_type.as_str() {
        "tv_show" => refresh_show_anidb(db, client, image_cache_dir, item, mode).await,
        "movie" => {
            // AniDB has anime movies too
            refresh_movie_anidb(db, client, image_cache_dir, item, mode).await
        }
        _ => Ok(false), // Seasons/episodes handled by the show-level refresh cascade
    }
}

/// TVDB refresh for a single item. Returns Ok(true) if matched, Ok(false) if no match.
async fn refresh_item_tvdb(
    db: &DbPool,
    client: &TvdbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    match item.media_type.as_str() {
        "tv_show" => refresh_show_tvdb(db, client, image_cache_dir, item, mode).await,
        "movie" => refresh_movie_tvdb(db, client, image_cache_dir, item, mode).await,
        _ => Ok(false), // Seasons/episodes handled by the show-level refresh cascade
    }
}

/// Refresh all items in a library.
pub async fn refresh_library(
    db: &DbPool,
    api_key: &str,
    tvdb_api_key: &str,
    image_cache_dir: &str,
    library_id: i64,
    mode: RefreshMode,
) -> Result<RefreshResult, anyhow::Error> {
    let items = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE library_id = ? AND media_type IN ('movie', 'tv_show') ORDER BY title",
    )
    .bind(library_id)
    .fetch_all(db)
    .await?;

    let mut result = RefreshResult::default();

    for item in &items {
        match refresh_item(db, api_key, tvdb_api_key, image_cache_dir, item.id, mode).await {
            Ok(_) => result.refreshed += 1,
            Err(e) => {
                warn!("Failed to refresh '{}': {}", item.title, e);
                result.errors.push(format!("{}: {}", item.title, e));
            }
        }
    }

    info!(
        "Library refresh complete: {} refreshed, {} errors",
        result.refreshed,
        result.errors.len()
    );

    Ok(result)
}

#[derive(Debug, Default, serde::Serialize)]
pub struct RefreshResult {
    pub refreshed: u32,
    pub errors: Vec<String>,
}

// ── Movie refresh ──

async fn refresh_movie(
    db: &DbPool,
    client: &TmdbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    // If we already have a tmdb_id from a previous lookup, use it directly
    let tmdb_id = if let Some(ref id) = item.tmdb_id {
        id.parse::<u64>().ok()
    } else {
        None
    };

    let tmdb_id = match tmdb_id {
        Some(id) if mode == RefreshMode::Standard => id,
        _ => {
            // Search TMDB
            let year = item
                .release_date
                .as_ref()
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok());
            let results = client.search_movie(&item.title, year).await?;
            match results.first() {
                Some(r) => r.id,
                None => {
                    debug!("No TMDB results for movie '{}'", item.title);
                    return Ok(false);
                }
            }
        }
    };

    let detail = client.movie_detail(tmdb_id).await?;

    // Get US content rating
    let content_rating = detail.content_ratings.as_ref().and_then(|cr| {
        cr.results
            .iter()
            .find(|r| r.iso_3166_1 == "US")
            .and_then(|r| {
                r.release_dates
                    .as_ref()
                    .and_then(|rds| rds.iter().find_map(|rd| rd.certification.clone()))
                    .or_else(|| r.rating.clone())
            })
    });

    if mode == RefreshMode::Full {
        // Wipe existing metadata
        sqlx::query(
            "UPDATE media_items SET description = NULL, rating = NULL, content_rating = NULL, tagline = NULL, imdb_id = NULL, tmdb_id = NULL WHERE id = ?",
        )
        .bind(item.id)
        .execute(db)
        .await?;
        // Clear old images, genres, credits
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM media_genres WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM media_credits WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
    }

    // Update fields — in Standard mode, only fill NULLs
    if (mode == RefreshMode::Full || item.description.is_none())
        && let Some(ref overview) = detail.overview {
            sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                .bind(overview)
                .bind(item.id)
                .execute(db)
                .await?;
        }
    if (mode == RefreshMode::Full || item.rating.is_none())
        && let Some(rating) = detail.vote_average {
            sqlx::query("UPDATE media_items SET rating = ? WHERE id = ?")
                .bind(rating)
                .bind(item.id)
                .execute(db)
                .await?;
        }
    if (mode == RefreshMode::Full || item.content_rating.is_none())
        && let Some(ref cr) = content_rating {
            sqlx::query("UPDATE media_items SET content_rating = ? WHERE id = ?")
                .bind(cr)
                .bind(item.id)
                .execute(db)
                .await?;
        }
    if (mode == RefreshMode::Full || item.tagline.is_none())
        && let Some(ref tagline) = detail.tagline
            && !tagline.is_empty() {
                sqlx::query("UPDATE media_items SET tagline = ? WHERE id = ?")
                    .bind(tagline)
                    .bind(item.id)
                    .execute(db)
                    .await?;
            }

    // Always update IDs
    sqlx::query("UPDATE media_items SET tmdb_id = ?, imdb_id = COALESCE(imdb_id, ?), duration_seconds = COALESCE(duration_seconds, ?) WHERE id = ?")
        .bind(tmdb_id.to_string())
        .bind(&detail.imdb_id)
        .bind(detail.runtime.map(|m| m * 60))
        .bind(item.id)
        .execute(db)
        .await?;

    // Genres
    save_genres(db, item.id, &detail.genres, mode).await?;

    // Credits (top 20 cast + key crew)
    if let Some(ref credits) = detail.credits {
        save_credits(db, item.id, credits, mode).await?;
    }

    // Images
    save_images(
        db,
        client,
        image_cache_dir,
        item.id,
        detail.poster_path.as_deref(),
        detail.backdrop_path.as_deref(),
        mode,
    )
    .await?;

    info!(
        "Refreshed movie metadata: '{}' (tmdb:{})",
        item.title, tmdb_id
    );
    Ok(true)
}

// ── TV Show refresh ──

async fn refresh_show(
    db: &DbPool,
    client: &TmdbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    let tmdb_id = if let Some(ref id) = item.tmdb_id {
        id.parse::<u64>().ok()
    } else {
        None
    };

    let tmdb_id = match tmdb_id {
        Some(id) if mode == RefreshMode::Standard => id,
        _ => {
            let year = item
                .release_date
                .as_ref()
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok());
            let results = client.search_tv(&item.title, year).await?;
            match results.first() {
                Some(r) => r.id,
                None => {
                    debug!("No TMDB results for TV show '{}'", item.title);
                    return Ok(false);
                }
            }
        }
    };

    let detail = client.tv_detail(tmdb_id).await?;

    let content_rating = detail.content_ratings.as_ref().and_then(|cr| {
        cr.results
            .iter()
            .find(|r| r.iso_3166_1 == "US")
            .and_then(|r| r.rating.clone())
    });

    if mode == RefreshMode::Full {
        sqlx::query(
            "UPDATE media_items SET description = NULL, rating = NULL, content_rating = NULL, tagline = NULL, imdb_id = NULL, tmdb_id = NULL, tvdb_id = NULL WHERE id = ?",
        )
        .bind(item.id).execute(db).await?;
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM media_genres WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM media_credits WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
    }

    if (mode == RefreshMode::Full || item.description.is_none())
        && let Some(ref overview) = detail.overview {
            sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                .bind(overview)
                .bind(item.id)
                .execute(db)
                .await?;
        }
    if (mode == RefreshMode::Full || item.rating.is_none())
        && let Some(rating) = detail.vote_average {
            sqlx::query("UPDATE media_items SET rating = ? WHERE id = ?")
                .bind(rating)
                .bind(item.id)
                .execute(db)
                .await?;
        }
    if (mode == RefreshMode::Full || item.content_rating.is_none())
        && let Some(ref cr) = content_rating {
            sqlx::query("UPDATE media_items SET content_rating = ? WHERE id = ?")
                .bind(cr)
                .bind(item.id)
                .execute(db)
                .await?;
        }
    if (mode == RefreshMode::Full || item.tagline.is_none())
        && let Some(ref tagline) = detail.tagline
            && !tagline.is_empty() {
                sqlx::query("UPDATE media_items SET tagline = ? WHERE id = ?")
                    .bind(tagline)
                    .bind(item.id)
                    .execute(db)
                    .await?;
            }

    let imdb_id = detail.external_ids.as_ref().and_then(|e| e.imdb_id.clone());
    let tvdb_id = detail
        .external_ids
        .as_ref()
        .and_then(|e| e.tvdb_id)
        .map(|id| id.to_string());

    sqlx::query("UPDATE media_items SET tmdb_id = ?, imdb_id = COALESCE(imdb_id, ?), tvdb_id = COALESCE(tvdb_id, ?) WHERE id = ?")
        .bind(tmdb_id.to_string())
        .bind(&imdb_id)
        .bind(&tvdb_id)
        .bind(item.id)
        .execute(db)
        .await?;

    save_genres(db, item.id, &detail.genres, mode).await?;

    if let Some(ref credits) = detail.credits {
        save_credits(db, item.id, credits, mode).await?;
    }

    save_images(
        db,
        client,
        image_cache_dir,
        item.id,
        detail.poster_path.as_deref(),
        detail.backdrop_path.as_deref(),
        mode,
    )
    .await?;

    // Now refresh seasons + episodes
    let local_seasons = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'season' ORDER BY season_number",
    )
    .bind(item.id)
    .fetch_all(db)
    .await?;

    for season in &local_seasons {
        if let Err(e) = refresh_season(db, client, image_cache_dir, season, tmdb_id, mode).await {
            warn!("Failed to refresh season '{}': {}", season.title, e);
        }
    }

    info!(
        "Refreshed TV show metadata: '{}' (tmdb:{})",
        item.title, tmdb_id
    );
    Ok(true)
}

// ── Season refresh ──

async fn refresh_season(
    db: &DbPool,
    client: &TmdbClient,
    image_cache_dir: &str,
    season_item: &MediaItem,
    show_tmdb_id: u64,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    let season_num = season_item.season_number.unwrap_or(1) as u32;

    let season_detail = match client.tv_season_detail(show_tmdb_id, season_num).await? {
        Some(d) => d,
        None => {
            debug!(
                "TMDB has no season {} for show {}",
                season_num, show_tmdb_id
            );
            return Ok(false);
        }
    };

    // Update season poster
    if let Some(ref poster) = season_detail.poster_path {
        save_images(
            db,
            client,
            image_cache_dir,
            season_item.id,
            Some(poster),
            None,
            mode,
        )
        .await?;
    }

    if (mode == RefreshMode::Full || season_item.description.is_none())
        && let Some(ref overview) = season_detail.overview
            && !overview.is_empty() {
                sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                    .bind(overview)
                    .bind(season_item.id)
                    .execute(db)
                    .await?;
            }

    // Refresh episodes
    let local_episodes = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'episode' ORDER BY episode_number",
    )
    .bind(season_item.id)
    .fetch_all(db)
    .await?;

    for ep_item in &local_episodes {
        let ep_num = ep_item.episode_number.unwrap_or(0) as u32;
        if let Some(tmdb_ep) = season_detail
            .episodes
            .iter()
            .find(|e| e.episode_number == ep_num)
        {
            update_episode_from_tmdb(db, client, image_cache_dir, ep_item, tmdb_ep, mode).await?;
        }
        generate_video_thumbnail(db, image_cache_dir, ep_item).await;
    }

    Ok(true)
}

// ── Episode refresh ──

async fn refresh_episode(
    db: &DbPool,
    client: &TmdbClient,
    image_cache_dir: &str,
    ep_item: &MediaItem,
    show_tmdb_id: u64,
    season_num: u32,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    let ep_num = ep_item.episode_number.unwrap_or(0) as u32;

    if let Some(season_detail) = client.tv_season_detail(show_tmdb_id, season_num).await?
        && let Some(tmdb_ep) = season_detail
            .episodes
            .iter()
            .find(|e| e.episode_number == ep_num)
        {
            update_episode_from_tmdb(db, client, image_cache_dir, ep_item, tmdb_ep, mode).await?;
            return Ok(true);
        }

    Ok(false)
}

// ── AniDB refresh functions ──

async fn refresh_show_anidb(
    db: &DbPool,
    client: &anidb::AnidbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    // Search by title
    let results = client.search(&item.title);
    let aid = match results.first() {
        Some((aid, _)) => *aid,
        None => {
            debug!("No AniDB results for '{}'", item.title);
            return Ok(false);
        }
    };

    let detail = client.anime_detail(aid).await?;

    if mode == RefreshMode::Full {
        sqlx::query("UPDATE media_items SET description = NULL, rating = NULL WHERE id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
    }

    if (mode == RefreshMode::Full || item.description.is_none())
        && let Some(ref desc) = detail.description {
            let cleaned = anidb::clean_description(desc);
            if !cleaned.is_empty() {
                sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                    .bind(&cleaned)
                    .bind(item.id)
                    .execute(db)
                    .await?;
            }
        }

    if (mode == RefreshMode::Full || item.rating.is_none())
        && let Some(ref ratings) = detail.ratings
            && let Some(ref perm) = ratings.permanent
                && let Some(ref val) = perm.value
                    && let Ok(r) = val.parse::<f64>() {
                        sqlx::query("UPDATE media_items SET rating = ? WHERE id = ?")
                            .bind(r)
                            .bind(item.id)
                            .execute(db)
                            .await?;
                    }

    // Poster
    if let Some(ref picture) = detail.picture {
        save_anidb_image(
            db,
            client,
            image_cache_dir,
            item.id,
            picture,
            "poster",
            mode,
        )
        .await?;
    }

    // Tags as genres
    if let Some(ref tags) = detail.tags {
        let genre_tags: Vec<tmdb::Genre> = tags
            .tags
            .iter()
            .filter(|t| {
                t.weight
                    .as_ref()
                    .and_then(|w| w.parse::<u32>().ok())
                    .unwrap_or(0)
                    >= 200
            })
            .filter_map(|t| {
                t.name.as_ref().map(|n| tmdb::Genre {
                    id: 0,
                    name: n.clone(),
                })
            })
            .take(10)
            .collect();
        if !genre_tags.is_empty() {
            save_genres(db, item.id, &genre_tags, mode).await?;
        }
    }

    // Episodes
    if let Some(ref episodes) = detail.episodes {
        let local_seasons = sqlx::query_as::<_, MediaItem>(
            "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'season' ORDER BY season_number",
        )
        .bind(item.id)
        .fetch_all(db)
        .await?;

        for season_item in &local_seasons {
            let local_episodes = sqlx::query_as::<_, MediaItem>(
                "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'episode' ORDER BY episode_number",
            )
            .bind(season_item.id)
            .fetch_all(db)
            .await?;

            for ep_item in &local_episodes {
                let ep_num = ep_item.episode_number.unwrap_or(0) as u32;
                if let Some(anidb_ep) = episodes
                    .episodes
                    .iter()
                    .find(|e| e.regular_episode_number() == Some(ep_num))
                {
                    update_episode_from_anidb(db, ep_item, anidb_ep, mode).await?;
                }
                generate_video_thumbnail(db, image_cache_dir, ep_item).await;
            }
        }
    }

    info!(
        "Refreshed TV show metadata from AniDB: '{}' (aid:{})",
        item.title, aid
    );
    Ok(true)
}

async fn refresh_movie_anidb(
    db: &DbPool,
    client: &anidb::AnidbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    let results = client.search(&item.title);
    let aid = match results.first() {
        Some((aid, _)) => *aid,
        None => return Ok(false),
    };

    let detail = client.anime_detail(aid).await?;

    if mode == RefreshMode::Full {
        sqlx::query("UPDATE media_items SET description = NULL, rating = NULL WHERE id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
    }

    if (mode == RefreshMode::Full || item.description.is_none())
        && let Some(ref desc) = detail.description {
            let cleaned = anidb::clean_description(desc);
            if !cleaned.is_empty() {
                sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                    .bind(&cleaned)
                    .bind(item.id)
                    .execute(db)
                    .await?;
            }
        }

    if (mode == RefreshMode::Full || item.rating.is_none())
        && let Some(ref ratings) = detail.ratings
            && let Some(ref perm) = ratings.permanent
                && let Some(ref val) = perm.value
                    && let Ok(r) = val.parse::<f64>() {
                        sqlx::query("UPDATE media_items SET rating = ? WHERE id = ?")
                            .bind(r)
                            .bind(item.id)
                            .execute(db)
                            .await?;
                    }

    if let Some(ref picture) = detail.picture {
        save_anidb_image(
            db,
            client,
            image_cache_dir,
            item.id,
            picture,
            "poster",
            mode,
        )
        .await?;
    }

    info!(
        "Refreshed movie metadata from AniDB: '{}' (aid:{})",
        item.title, aid
    );
    Ok(true)
}

async fn update_episode_from_anidb(
    db: &DbPool,
    ep_item: &MediaItem,
    anidb_ep: &anidb::AnimeEpisode,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if mode == RefreshMode::Full {
        sqlx::query(
            "UPDATE media_items SET description = NULL, duration_seconds = NULL WHERE id = ?",
        )
        .bind(ep_item.id)
        .execute(db)
        .await?;
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(ep_item.id)
            .execute(db)
            .await?;
    }

    if (mode == RefreshMode::Full || ep_item.description.is_none())
        && let Some(ref summary) = anidb_ep.summary {
            let cleaned = anidb::clean_description(summary);
            if !cleaned.is_empty() {
                sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                    .bind(&cleaned)
                    .bind(ep_item.id)
                    .execute(db)
                    .await?;
            }
        }

    if let Some(title) = anidb_ep.english_title() {
        // Only override title if it looks like a generic "Episode X" title
        if ep_item.title.starts_with("Episode ") {
            sqlx::query("UPDATE media_items SET title = ? WHERE id = ?")
                .bind(title)
                .bind(ep_item.id)
                .execute(db)
                .await?;
        }
    }

    if let Some(runtime) = anidb_ep.runtime_seconds() {
        sqlx::query(
            "UPDATE media_items SET duration_seconds = COALESCE(duration_seconds, ?) WHERE id = ?",
        )
        .bind(runtime)
        .bind(ep_item.id)
        .execute(db)
        .await?;
    }

    Ok(())
}

async fn save_anidb_image(
    db: &DbPool,
    client: &anidb::AnidbClient,
    cache_dir: &str,
    media_id: i64,
    picture: &str,
    image_type: &str,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if mode == RefreshMode::Standard {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM images WHERE media_item_id = ? AND image_type = ?",
        )
        .bind(media_id)
        .bind(image_type)
        .fetch_one(db)
        .await?;
        if count > 0 {
            return Ok(());
        }
    }

    let bytes = match client.download_image(picture).await {
        Ok(b) => b,
        Err(e) => {
            debug!("Failed to download AniDB image {}: {}", picture, e);
            return Ok(());
        }
    };

    let ext = std::path::Path::new(picture)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    let filename = format!("{}_{}_anidb.{}", media_id, image_type, ext);
    let local_path = format!("{}/{}", cache_dir, filename);

    tokio::fs::write(&local_path, &bytes).await?;

    sqlx::query("DELETE FROM images WHERE media_item_id = ? AND image_type = ?")
        .bind(media_id)
        .bind(image_type)
        .execute(db)
        .await?;

    let url = anidb::AnidbClient::image_url(picture);
    sqlx::query(
        "INSERT INTO images (media_item_id, image_type, url, file_path, is_primary) VALUES (?, ?, ?, ?, 1)",
    )
    .bind(media_id).bind(image_type).bind(&url).bind(&filename)
    .execute(db).await?;

    Ok(())
}

// ── TVDB refresh functions ──

async fn refresh_show_tvdb(
    db: &DbPool,
    client: &TvdbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    // Reuse an existing tvdb_id if we have one (populated by TMDB or a prior run);
    // otherwise search by title.
    let tvdb_id = match item.tvdb_id.as_ref().and_then(|s| s.parse::<u64>().ok()) {
        Some(id) if mode == RefreshMode::Standard => id,
        _ => {
            let results = client.search_series(&item.title).await?;
            let parsed = results
                .iter()
                .find_map(|r| r.tvdb_id.as_ref()?.parse::<u64>().ok());
            match parsed {
                Some(id) => id,
                None => {
                    debug!("No TVDB results for TV show '{}'", item.title);
                    return Ok(false);
                }
            }
        }
    };

    let detail = client.series_extended(tvdb_id).await?;

    // Fetch English translation — the base response uses the show's original language.
    let translation = match client.series_translation(tvdb_id).await {
        Ok(t) => Some(t),
        Err(e) => {
            debug!("No English translation for TVDB series {}: {}", tvdb_id, e);
            None
        }
    };

    if mode == RefreshMode::Full {
        sqlx::query("UPDATE media_items SET description = NULL, rating = NULL WHERE id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM media_genres WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
    }

    if mode == RefreshMode::Full || item.description.is_none() {
        // Prefer English translation, fall back to original overview.
        let overview = translation
            .as_ref()
            .and_then(|t| t.overview.as_deref())
            .or(detail.overview.as_deref());
        if let Some(overview) = overview
            && !overview.is_empty() {
                sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                    .bind(overview)
                    .bind(item.id)
                    .execute(db)
                    .await?;
            }
    }

    if (mode == RefreshMode::Full || item.rating.is_none())
        && let Some(score) = detail.score {
            sqlx::query("UPDATE media_items SET rating = ? WHERE id = ?")
                .bind(score)
                .bind(item.id)
                .execute(db)
                .await?;
        }

    sqlx::query("UPDATE media_items SET tvdb_id = ? WHERE id = ?")
        .bind(tvdb_id.to_string())
        .bind(item.id)
        .execute(db)
        .await?;

    // Genres
    let genre_list: Vec<tmdb::Genre> = detail
        .genres
        .iter()
        .filter_map(|g| {
            g.name.as_ref().map(|n| tmdb::Genre {
                id: 0,
                name: n.clone(),
            })
        })
        .collect();
    if !genre_list.is_empty() {
        save_genres(db, item.id, &genre_list, mode).await?;
    }

    // Poster (TVDB artwork type 2 = series poster) and background (type 3)
    let poster = detail
        .artworks
        .iter()
        .find(|a| a.art_type == Some(2))
        .and_then(|a| a.image.as_deref())
        .or(detail.image.as_deref());
    let backdrop = detail
        .artworks
        .iter()
        .find(|a| a.art_type == Some(3))
        .and_then(|a| a.image.as_deref());

    if let Some(url) = poster {
        save_tvdb_image(db, client, image_cache_dir, item.id, url, "poster", mode).await?;
    }
    if let Some(url) = backdrop {
        save_tvdb_image(db, client, image_cache_dir, item.id, url, "backdrop", mode).await?;
    }

    // Season posters
    let local_seasons = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'season' ORDER BY season_number",
    )
    .bind(item.id)
    .fetch_all(db)
    .await?;

    for season_item in &local_seasons {
        let season_num = season_item.season_number.unwrap_or(0) as u32;
        if let Some(tvdb_season) = detail.seasons.iter().find(|s| {
            s.number == Some(season_num) && s.season_type.as_ref().and_then(|t| t.id) == Some(1)
        })
            && let Some(ref img) = tvdb_season.image {
                save_tvdb_image(
                    db,
                    client,
                    image_cache_dir,
                    season_item.id,
                    img,
                    "poster",
                    mode,
                )
                .await?;
            }
    }

    // Episodes
    let episodes = client.series_episodes(tvdb_id).await?;
    for season_item in &local_seasons {
        let season_num = season_item.season_number.unwrap_or(0) as u32;
        let local_episodes = sqlx::query_as::<_, MediaItem>(
            "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'episode' ORDER BY episode_number",
        )
        .bind(season_item.id)
        .fetch_all(db)
        .await?;

        for ep_item in &local_episodes {
            let ep_num = ep_item.episode_number.unwrap_or(0) as u32;
            if let Some(tvdb_ep) = episodes
                .episodes
                .iter()
                .find(|e| e.season_number == Some(season_num) && e.number == Some(ep_num))
            {
                update_episode_from_tvdb(db, client, image_cache_dir, ep_item, tvdb_ep, mode)
                    .await?;
            }
            generate_video_thumbnail(db, image_cache_dir, ep_item).await;
        }
    }

    info!(
        "Refreshed TV show metadata from TVDB: '{}' (tvdb:{})",
        item.title, tvdb_id
    );
    Ok(true)
}

async fn refresh_movie_tvdb(
    db: &DbPool,
    client: &TvdbClient,
    image_cache_dir: &str,
    item: &MediaItem,
    mode: RefreshMode,
) -> Result<bool, anyhow::Error> {
    let results = client.search_movie(&item.title).await?;
    let tvdb_id = match results
        .iter()
        .find_map(|r| r.tvdb_id.as_ref()?.parse::<u64>().ok())
    {
        Some(id) => id,
        None => {
            debug!("No TVDB results for movie '{}'", item.title);
            return Ok(false);
        }
    };

    let detail = client.movie_extended(tvdb_id).await?;

    // Fetch English translation — the base response uses the movie's original language.
    let translation = match client.movie_translation(tvdb_id).await {
        Ok(t) => Some(t),
        Err(e) => {
            debug!("No English translation for TVDB movie {}: {}", tvdb_id, e);
            None
        }
    };

    if mode == RefreshMode::Full {
        sqlx::query("UPDATE media_items SET description = NULL, rating = NULL WHERE id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
        sqlx::query("DELETE FROM media_genres WHERE media_item_id = ?")
            .bind(item.id)
            .execute(db)
            .await?;
    }

    if mode == RefreshMode::Full || item.description.is_none() {
        let overview = translation
            .as_ref()
            .and_then(|t| t.overview.as_deref())
            .or(detail.overview.as_deref());
        if let Some(overview) = overview
            && !overview.is_empty() {
                sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                    .bind(overview)
                    .bind(item.id)
                    .execute(db)
                    .await?;
            }
    }

    if (mode == RefreshMode::Full || item.rating.is_none())
        && let Some(score) = detail.score {
            sqlx::query("UPDATE media_items SET rating = ? WHERE id = ?")
                .bind(score)
                .bind(item.id)
                .execute(db)
                .await?;
        }

    if let Some(runtime) = detail.runtime {
        sqlx::query(
            "UPDATE media_items SET duration_seconds = COALESCE(duration_seconds, ?) WHERE id = ?",
        )
        .bind((runtime * 60) as i64)
        .bind(item.id)
        .execute(db)
        .await?;
    }

    sqlx::query("UPDATE media_items SET tvdb_id = ? WHERE id = ?")
        .bind(tvdb_id.to_string())
        .bind(item.id)
        .execute(db)
        .await?;

    let genre_list: Vec<tmdb::Genre> = detail
        .genres
        .iter()
        .filter_map(|g| {
            g.name.as_ref().map(|n| tmdb::Genre {
                id: 0,
                name: n.clone(),
            })
        })
        .collect();
    if !genre_list.is_empty() {
        save_genres(db, item.id, &genre_list, mode).await?;
    }

    let poster = detail
        .artworks
        .iter()
        .find(|a| a.art_type == Some(14))
        .and_then(|a| a.image.as_deref())
        .or(detail.image.as_deref());
    let backdrop = detail
        .artworks
        .iter()
        .find(|a| a.art_type == Some(15))
        .and_then(|a| a.image.as_deref());

    if let Some(url) = poster {
        save_tvdb_image(db, client, image_cache_dir, item.id, url, "poster", mode).await?;
    }
    if let Some(url) = backdrop {
        save_tvdb_image(db, client, image_cache_dir, item.id, url, "backdrop", mode).await?;
    }

    info!(
        "Refreshed movie metadata from TVDB: '{}' (tvdb:{})",
        item.title, tvdb_id
    );
    Ok(true)
}

async fn update_episode_from_tvdb(
    db: &DbPool,
    client: &TvdbClient,
    image_cache_dir: &str,
    ep_item: &MediaItem,
    tvdb_ep: &tvdb::Episode,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if mode == RefreshMode::Full {
        sqlx::query(
            "UPDATE media_items SET description = NULL, duration_seconds = NULL WHERE id = ?",
        )
        .bind(ep_item.id)
        .execute(db)
        .await?;
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(ep_item.id)
            .execute(db)
            .await?;
    }

    if (mode == RefreshMode::Full || ep_item.description.is_none())
        && let Some(ref overview) = tvdb_ep.overview
            && !overview.is_empty() {
                sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                    .bind(overview)
                    .bind(ep_item.id)
                    .execute(db)
                    .await?;
            }

    if let Some(ref name) = tvdb_ep.name
        && !name.is_empty() && ep_item.title.starts_with("Episode ") {
            sqlx::query("UPDATE media_items SET title = ? WHERE id = ?")
                .bind(name)
                .bind(ep_item.id)
                .execute(db)
                .await?;
        }

    if let Some(runtime) = tvdb_ep.runtime {
        sqlx::query(
            "UPDATE media_items SET duration_seconds = COALESCE(duration_seconds, ?) WHERE id = ?",
        )
        .bind((runtime * 60) as i64)
        .bind(ep_item.id)
        .execute(db)
        .await?;
    }

    if let Some(ref img) = tvdb_ep.image {
        save_tvdb_image(
            db,
            client,
            image_cache_dir,
            ep_item.id,
            img,
            "thumbnail",
            mode,
        )
        .await?;
    }

    Ok(())
}

async fn save_tvdb_image(
    db: &DbPool,
    client: &TvdbClient,
    cache_dir: &str,
    media_id: i64,
    url: &str,
    image_type: &str,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if mode == RefreshMode::Standard {
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM images WHERE media_item_id = ? AND image_type = ?",
        )
        .bind(media_id)
        .bind(image_type)
        .fetch_one(db)
        .await?;
        if count > 0 {
            return Ok(());
        }
    }

    let bytes = match client.download_image(url).await {
        Ok(b) => b,
        Err(e) => {
            debug!("Failed to download TVDB image {}: {}", url, e);
            return Ok(());
        }
    };

    let ext = std::path::Path::new(url)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");
    let filename = format!("{}_{}_tvdb.{}", media_id, image_type, ext);
    let local_path = format!("{}/{}", cache_dir, filename);

    tokio::fs::write(&local_path, &bytes).await?;

    sqlx::query("DELETE FROM images WHERE media_item_id = ? AND image_type = ?")
        .bind(media_id)
        .bind(image_type)
        .execute(db)
        .await?;

    sqlx::query(
        "INSERT INTO images (media_item_id, image_type, url, file_path, is_primary) VALUES (?, ?, ?, ?, 1)",
    )
    .bind(media_id).bind(image_type).bind(url).bind(&filename)
    .execute(db).await?;

    Ok(())
}

// ── TMDB episode helper ──

async fn update_episode_from_tmdb(
    db: &DbPool,
    client: &TmdbClient,
    image_cache_dir: &str,
    ep_item: &MediaItem,
    tmdb_ep: &tmdb::TvEpisode,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if mode == RefreshMode::Full {
        sqlx::query("UPDATE media_items SET description = NULL, rating = NULL, duration_seconds = NULL WHERE id = ?")
            .bind(ep_item.id).execute(db).await?;
        sqlx::query("DELETE FROM images WHERE media_item_id = ?")
            .bind(ep_item.id)
            .execute(db)
            .await?;
    }

    if (mode == RefreshMode::Full || ep_item.description.is_none())
        && let Some(ref overview) = tmdb_ep.overview
            && !overview.is_empty() {
                sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
                    .bind(overview)
                    .bind(ep_item.id)
                    .execute(db)
                    .await?;
            }

    if (mode == RefreshMode::Full || ep_item.rating.is_none())
        && let Some(rating) = tmdb_ep.vote_average {
            sqlx::query("UPDATE media_items SET rating = ? WHERE id = ?")
                .bind(rating)
                .bind(ep_item.id)
                .execute(db)
                .await?;
        }

    if let Some(runtime) = tmdb_ep.runtime {
        sqlx::query(
            "UPDATE media_items SET duration_seconds = COALESCE(duration_seconds, ?) WHERE id = ?",
        )
        .bind(runtime * 60)
        .bind(ep_item.id)
        .execute(db)
        .await?;
    }

    // Episode thumbnail
    if let Some(ref still) = tmdb_ep.still_path {
        save_images(
            db,
            client,
            image_cache_dir,
            ep_item.id,
            None,
            Some(still.as_str()),
            mode,
        )
        .await?;
    }

    Ok(())
}

// ── Video Thumbnail Generation ──

/// Generate a thumbnail from a video file using FFmpeg if the item has a
/// file_path but no backdrop/thumbnail image yet.
async fn generate_video_thumbnail(db: &DbPool, cache_dir: &str, item: &MediaItem) {
    let file_path = match item.file_path.as_ref() {
        Some(p) if !p.is_empty() => p,
        _ => return,
    };

    // Only generate for items with actual video files (episodes, movies)
    if item.media_type != "episode" && item.media_type != "movie" {
        return;
    }

    // Check if already has a backdrop/thumbnail
    let (count,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM images WHERE media_item_id = ? AND image_type IN ('backdrop', 'thumbnail')",
    )
    .bind(item.id)
    .fetch_one(db)
    .await
    .unwrap_or((0,));

    if count > 0 {
        return;
    }

    let ffmpeg = std::env::var("FFMPEG_PATH").unwrap_or_else(|_| "ffmpeg".to_string());
    let ffprobe = std::env::var("FFPROBE_PATH").unwrap_or_else(|_| "ffprobe".to_string());

    let filename = format!("{}_thumbnail.jpg", item.id);
    let output_path = format!("{}/{}", cache_dir, filename);

    // Get video duration with ffprobe, then seek to 10% (same as Jellyfin)
    let seek_seconds = match tokio::process::Command::new(&ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            file_path,
        ])
        .output()
        .await
    {
        Ok(output) => {
            let s = String::from_utf8_lossy(&output.stdout);
            s.trim()
                .parse::<f64>()
                .ok()
                .map(|d| (d * 0.1) as u64)
                .unwrap_or(30)
        }
        Err(_) => 30,
    };

    let seek_str = seek_seconds.to_string();
    let result = tokio::process::Command::new(&ffmpeg)
        .args([
            "-ss",
            &seek_str,
            "-i",
            file_path,
            "-vframes",
            "1",
            "-q:v",
            "5",
            "-vf",
            "scale=320:-1",
            "-y",
            &output_path,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await;

    match result {
        Ok(status) if status.success() => {
            // Insert image record
            let _ = sqlx::query(
                "INSERT INTO images (media_item_id, image_type, file_path, is_primary) VALUES (?, 'thumbnail', ?, 1)",
            )
            .bind(item.id)
            .bind(&filename)
            .execute(db)
            .await;
            debug!("Generated video thumbnail for item {}", item.id);
        }
        Ok(status) => {
            debug!("FFmpeg exited with {} for item {}", status, item.id);
        }
        Err(e) => {
            // FFmpeg not found or failed — silently skip
            debug!("FFmpeg not available for thumbnail generation: {}", e);
        }
    }
}

// ── Helpers ──

async fn save_genres(
    db: &DbPool,
    media_id: i64,
    genres: &[tmdb::Genre],
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if mode == RefreshMode::Standard {
        // Check if already has genres
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM media_genres WHERE media_item_id = ?")
                .bind(media_id)
                .fetch_one(db)
                .await?;
        if count > 0 {
            return Ok(());
        }
    }

    sqlx::query("DELETE FROM media_genres WHERE media_item_id = ?")
        .bind(media_id)
        .execute(db)
        .await?;

    for genre in genres {
        // Upsert genre
        sqlx::query("INSERT OR IGNORE INTO genres (name) VALUES (?)")
            .bind(&genre.name)
            .execute(db)
            .await?;
        let (genre_id,): (i64,) = sqlx::query_as("SELECT id FROM genres WHERE name = ?")
            .bind(&genre.name)
            .fetch_one(db)
            .await?;
        sqlx::query("INSERT OR IGNORE INTO media_genres (media_item_id, genre_id) VALUES (?, ?)")
            .bind(media_id)
            .bind(genre_id)
            .execute(db)
            .await?;
    }
    Ok(())
}

async fn save_credits(
    db: &DbPool,
    media_id: i64,
    credits: &tmdb::Credits,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if mode == RefreshMode::Standard {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM media_credits WHERE media_item_id = ?")
                .bind(media_id)
                .fetch_one(db)
                .await?;
        if count > 0 {
            return Ok(());
        }
    }

    sqlx::query("DELETE FROM media_credits WHERE media_item_id = ?")
        .bind(media_id)
        .execute(db)
        .await?;

    // Top 20 cast
    for member in credits.cast.iter().take(20) {
        let person_id = upsert_person(db, member.id, &member.name).await?;
        sqlx::query(
            "INSERT INTO media_credits (media_item_id, person_id, role_type, character_name, credit_order) VALUES (?, ?, 'actor', ?, ?)",
        )
        .bind(media_id)
        .bind(person_id)
        .bind(&member.character)
        .bind(member.order)
        .execute(db)
        .await?;
    }

    // Key crew: directors, writers
    for member in &credits.crew {
        let role = match member.job.to_lowercase().as_str() {
            "director" => "director",
            "screenplay" | "writer" | "story" => "writer",
            "producer" | "executive producer" => "producer",
            "original music composer" | "music" => "composer",
            _ => continue,
        };
        let person_id = upsert_person(db, member.id, &member.name).await?;
        sqlx::query(
            "INSERT INTO media_credits (media_item_id, person_id, role_type) VALUES (?, ?, ?)",
        )
        .bind(media_id)
        .bind(person_id)
        .bind(role)
        .execute(db)
        .await?;
    }

    Ok(())
}

async fn upsert_person(db: &DbPool, tmdb_id: u64, name: &str) -> Result<i64, anyhow::Error> {
    let tmdb_str = tmdb_id.to_string();
    let existing: Option<(i64,)> = sqlx::query_as("SELECT id FROM people WHERE tmdb_id = ?")
        .bind(&tmdb_str)
        .fetch_optional(db)
        .await?;
    if let Some((id,)) = existing {
        return Ok(id);
    }

    let result = sqlx::query("INSERT INTO people (name, tmdb_id) VALUES (?, ?)")
        .bind(name)
        .bind(&tmdb_str)
        .execute(db)
        .await?;
    Ok(result.last_insert_rowid())
}

async fn save_images(
    db: &DbPool,
    client: &TmdbClient,
    cache_dir: &str,
    media_id: i64,
    poster_path: Option<&str>,
    backdrop_path: Option<&str>,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if let Some(path) = poster_path {
        save_single_image(
            db, client, cache_dir, media_id, path, "poster", "w500", mode,
        )
        .await?;
    }
    if let Some(path) = backdrop_path {
        save_single_image(
            db, client, cache_dir, media_id, path, "backdrop", "w1280", mode,
        )
        .await?;
    }
    Ok(())
}

async fn save_single_image(
    db: &DbPool,
    client: &TmdbClient,
    cache_dir: &str,
    media_id: i64,
    tmdb_path: &str,
    image_type: &str,
    size: &str,
    mode: RefreshMode,
) -> Result<(), anyhow::Error> {
    if mode == RefreshMode::Standard {
        // Check if already has this type of image
        let (count,): (i64,) = sqlx::query_as(
            "SELECT COUNT(*) FROM images WHERE media_item_id = ? AND image_type = ?",
        )
        .bind(media_id)
        .bind(image_type)
        .fetch_one(db)
        .await?;
        if count > 0 {
            return Ok(());
        }
    }

    // Download image
    let bytes = match client.download_image(tmdb_path, size).await {
        Ok(b) => b,
        Err(e) => {
            debug!("Failed to download image {}: {}", tmdb_path, e);
            return Ok(());
        }
    };

    // Determine extension from path
    let ext = std::path::Path::new(tmdb_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("jpg");

    // Save to disk
    let filename = format!("{}_{}.{}", media_id, image_type, ext);
    let local_path = format!("{}/{}", cache_dir, filename);

    tokio::fs::write(&local_path, &bytes).await?;

    // Remove old image of same type
    sqlx::query("DELETE FROM images WHERE media_item_id = ? AND image_type = ?")
        .bind(media_id)
        .bind(image_type)
        .execute(db)
        .await?;

    // Insert image record
    let url = TmdbClient::image_url(tmdb_path, "original");
    sqlx::query(
        "INSERT INTO images (media_item_id, image_type, url, file_path, is_primary) VALUES (?, ?, ?, ?, 1)",
    )
    .bind(media_id)
    .bind(image_type)
    .bind(&url)
    .bind(&filename)
    .execute(db)
    .await?;

    Ok(())
}
