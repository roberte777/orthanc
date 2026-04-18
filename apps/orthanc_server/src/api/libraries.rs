use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::AppState,
    },
    auth::middleware::AdminUser,
    models::library::{
        AddLibraryPathRequest, CreateLibraryRequest, Library, LibraryPath, LibraryPathResponse,
        LibraryResponse, UpdateLibraryRequest,
    },
};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_libraries).post(create_library))
        .route(
            "/{id}",
            get(get_library).put(update_library).delete(delete_library),
        )
        .route("/{id}/paths", axum::routing::post(add_path))
        .route("/{id}/paths/{path_id}", axum::routing::delete(remove_path))
        .route("/{id}/scan", axum::routing::post(scan_library))
        .route("/{id}/media", get(list_media))
        .route(
            "/{id}/providers",
            get(list_providers).put(update_provider),
        )
}

/// Create default Movies and TV Shows libraries if none exist.
/// Called during initial setup.
pub async fn create_default_libraries(db: &crate::db::DbPool) -> Result<(), anyhow::Error> {
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM libraries")
        .fetch_one(db)
        .await?;

    if count == 0 {
        sqlx::query(
            "INSERT INTO libraries (name, library_type, description) VALUES (?, ?, ?)",
        )
        .bind("Movies")
        .bind("movies")
        .bind("Default movie library")
        .execute(db)
        .await?;

        sqlx::query(
            "INSERT INTO libraries (name, library_type, description) VALUES (?, ?, ?)",
        )
        .bind("TV Shows")
        .bind("tv_shows")
        .bind("Default TV show library")
        .execute(db)
        .await?;

        tracing::info!("Created default Movies and TV Shows libraries");
    }

    Ok(())
}

async fn fetch_library_response(
    db: &crate::db::DbPool,
    library: Library,
) -> Result<LibraryResponse, anyhow::Error> {
    let paths = sqlx::query_as::<_, LibraryPath>(
        "SELECT * FROM library_paths WHERE library_id = ? ORDER BY created_at",
    )
    .bind(library.id)
    .fetch_all(db)
    .await?;

    Ok(LibraryResponse {
        id: library.id,
        name: library.name,
        library_type: library.library_type,
        description: library.description,
        is_enabled: library.is_enabled,
        scan_interval_minutes: library.scan_interval_minutes,
        last_scan_at: library.last_scan_at,
        paths: paths.iter().map(LibraryPathResponse::from).collect(),
        created_at: library.created_at,
        updated_at: library.updated_at,
    })
}

async fn list_libraries(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<LibraryResponse>>> {
    let libraries = sqlx::query_as::<_, Library>(
        "SELECT * FROM libraries ORDER BY created_at",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let mut result = Vec::with_capacity(libraries.len());
    for lib in libraries {
        result.push(
            fetch_library_response(&state.db, lib)
                .await
                .map_err(anyhow::Error::from)?,
        );
    }

    Ok(Json(result))
}

async fn get_library(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<LibraryResponse>> {
    let library = sqlx::query_as::<_, Library>("SELECT * FROM libraries WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("Library not found".to_string()))?;

    let resp = fetch_library_response(&state.db, library)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(resp))
}

async fn create_library(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateLibraryRequest>,
) -> ApiResult<Json<LibraryResponse>> {
    // Validate library_type
    if req.library_type != "movies" && req.library_type != "tv_shows" {
        return Err(ApiError::BadRequest(
            "library_type must be 'movies' or 'tv_shows'".to_string(),
        ));
    }

    if req.name.trim().is_empty() {
        return Err(ApiError::BadRequest("Name is required".to_string()));
    }

    // Validate all paths exist
    for path in &req.paths {
        let p = std::path::Path::new(path);
        if !p.exists() {
            return Err(ApiError::BadRequest(format!(
                "Path does not exist: {}",
                path
            )));
        }
        if !p.is_dir() {
            return Err(ApiError::BadRequest(format!(
                "Path is not a directory: {}",
                path
            )));
        }
    }

    let library = sqlx::query_as::<_, Library>(
        "INSERT INTO libraries (name, library_type, description, scan_interval_minutes)
         VALUES (?, ?, ?, ?)
         RETURNING *",
    )
    .bind(req.name.trim())
    .bind(&req.library_type)
    .bind(&req.description)
    .bind(req.scan_interval_minutes)
    .fetch_one(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    // Insert paths
    for path in &req.paths {
        sqlx::query("INSERT INTO library_paths (library_id, path) VALUES (?, ?)")
            .bind(library.id)
            .bind(path)
            .execute(&state.db)
            .await
            .map_err(anyhow::Error::from)?;
    }

    // Add default metadata providers (TMDB enabled, AniDB disabled)
    sqlx::query(
        "INSERT OR IGNORE INTO library_metadata_providers (library_id, provider, is_enabled, priority) VALUES (?, 'tmdb', 1, 0)",
    )
    .bind(library.id)
    .execute(&state.db)
    .await
    .map_err(anyhow::Error::from)?;
    sqlx::query(
        "INSERT OR IGNORE INTO library_metadata_providers (library_id, provider, is_enabled, priority) VALUES (?, 'anidb', 0, 10)",
    )
    .bind(library.id)
    .execute(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let resp = fetch_library_response(&state.db, library)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(resp))
}

async fn update_library(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateLibraryRequest>,
) -> ApiResult<Json<LibraryResponse>> {
    let library = sqlx::query_as::<_, Library>("SELECT * FROM libraries WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("Library not found".to_string()))?;

    let name = req.name.as_deref().unwrap_or(&library.name);
    if name.trim().is_empty() {
        return Err(ApiError::BadRequest("Name cannot be empty".to_string()));
    }
    let description = if req.description.is_some() {
        req.description
    } else {
        library.description
    };
    let is_enabled = req.is_enabled.unwrap_or(library.is_enabled);
    let scan_interval = if let Some(interval) = req.scan_interval_minutes {
        interval
    } else {
        library.scan_interval_minutes
    };

    let updated = sqlx::query_as::<_, Library>(
        "UPDATE libraries SET name = ?, description = ?, is_enabled = ?, scan_interval_minutes = ?
         WHERE id = ? RETURNING *",
    )
    .bind(name.trim())
    .bind(&description)
    .bind(is_enabled)
    .bind(scan_interval)
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let resp = fetch_library_response(&state.db, updated)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(resp))
}

async fn delete_library(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    sqlx::query_as::<_, Library>("SELECT * FROM libraries WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("Library not found".to_string()))?;

    sqlx::query("DELETE FROM libraries WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(serde_json::json!({"message": "Library deleted"})))
}

async fn add_path(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<AddLibraryPathRequest>,
) -> ApiResult<Json<LibraryPathResponse>> {
    // Verify library exists
    sqlx::query_as::<_, Library>("SELECT * FROM libraries WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("Library not found".to_string()))?;

    // Validate path
    let p = std::path::Path::new(&req.path);
    if !p.exists() {
        return Err(ApiError::BadRequest(format!(
            "Path does not exist: {}",
            req.path
        )));
    }
    if !p.is_dir() {
        return Err(ApiError::BadRequest(format!(
            "Path is not a directory: {}",
            req.path
        )));
    }

    // Check for duplicate
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM library_paths WHERE library_id = ? AND path = ?",
    )
    .bind(id)
    .bind(&req.path)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    if existing.is_some() {
        return Err(ApiError::Conflict(
            "Path already exists in this library".to_string(),
        ));
    }

    let path = sqlx::query_as::<_, LibraryPath>(
        "INSERT INTO library_paths (library_id, path) VALUES (?, ?) RETURNING *",
    )
    .bind(id)
    .bind(&req.path)
    .fetch_one(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(LibraryPathResponse::from(&path)))
}

async fn remove_path(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path((id, path_id)): Path<(i64, i64)>,
) -> ApiResult<Json<serde_json::Value>> {
    let existing: Option<(i64,)> = sqlx::query_as(
        "SELECT id FROM library_paths WHERE id = ? AND library_id = ?",
    )
    .bind(path_id)
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    if existing.is_none() {
        return Err(ApiError::NotFound("Path not found".to_string()));
    }

    sqlx::query("DELETE FROM library_paths WHERE id = ?")
        .bind(path_id)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(serde_json::json!({"message": "Path removed"})))
}

async fn scan_library(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<crate::scanner::ScanResult>> {
    let library = sqlx::query_as::<_, Library>("SELECT * FROM libraries WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("Library not found".to_string()))?;

    let result = crate::scanner::scan_library(&state.db, &library)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(result))
}

async fn list_media(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<crate::models::media::MediaItemResponse>>> {
    use crate::models::media::{MediaItem, MediaItemResponse};

    // Verify library exists
    sqlx::query_as::<_, Library>("SELECT * FROM libraries WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("Library not found".to_string()))?;

    // Get top-level items (movies and tv_shows) for this library
    let top_items = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE library_id = ? AND media_type IN ('movie', 'tv_show') ORDER BY sort_title",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let mut result: Vec<MediaItemResponse> = Vec::new();

    for item in top_items {
        let mut resp: MediaItemResponse = item.clone().into();

        if item.media_type == "tv_show" {
            // Load seasons
            let seasons = sqlx::query_as::<_, MediaItem>(
                "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'season' ORDER BY season_number",
            )
            .bind(item.id)
            .fetch_all(&state.db)
            .await
            .map_err(anyhow::Error::from)?;

            let mut season_resps: Vec<MediaItemResponse> = Vec::new();
            for season in seasons {
                let mut season_resp: MediaItemResponse = season.clone().into();

                // Load episodes
                let episodes = sqlx::query_as::<_, MediaItem>(
                    "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'episode' ORDER BY episode_number",
                )
                .bind(season.id)
                .fetch_all(&state.db)
                .await
                .map_err(anyhow::Error::from)?;

                season_resp.children = Some(episodes.into_iter().map(Into::into).collect());
                season_resps.push(season_resp);
            }
            resp.children = Some(season_resps);
        }

        result.push(resp);
    }

    Ok(Json(result))
}

// ── Provider management ──

#[derive(Debug, serde::Serialize, sqlx::FromRow)]
struct MetadataProvider {
    id: i64,
    library_id: i64,
    provider: String,
    is_enabled: bool,
    priority: i32,
}

async fn list_providers(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<Vec<MetadataProvider>>> {
    let providers = sqlx::query_as::<_, MetadataProvider>(
        "SELECT * FROM library_metadata_providers WHERE library_id = ? ORDER BY priority",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(providers))
}

#[derive(Debug, serde::Deserialize)]
struct UpdateProviderRequest {
    provider: String,
    is_enabled: Option<bool>,
    priority: Option<i32>,
}

async fn update_provider(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateProviderRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    sqlx::query(
        "INSERT INTO library_metadata_providers (library_id, provider, is_enabled, priority)
         VALUES (?, ?, ?, ?)
         ON CONFLICT(library_id, provider)
         DO UPDATE SET is_enabled = COALESCE(excluded.is_enabled, is_enabled),
                       priority = COALESCE(excluded.priority, priority)",
    )
    .bind(id)
    .bind(&req.provider)
    .bind(req.is_enabled.unwrap_or(true))
    .bind(req.priority.unwrap_or(0))
    .execute(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(serde_json::json!({"message": "Provider updated"})))
}
