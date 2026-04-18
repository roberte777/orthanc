use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::AppState,
    },
    auth::middleware::AdminUser,
    metadata::{self, RefreshMode, RefreshResult},
};
use axum::{
    extract::{Path, State},
    routing::{post, put},
    Json, Router,
};
use serde::Deserialize;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/refresh/{id}", post(refresh_item))
        .route("/refresh-library/{id}", post(refresh_library))
        .route("/override/{id}", put(override_metadata))
}

#[derive(Deserialize)]
struct RefreshRequest {
    #[serde(default)]
    mode: RefreshModeParam,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "lowercase")]
enum RefreshModeParam {
    #[default]
    Standard,
    Full,
}

impl From<RefreshModeParam> for RefreshMode {
    fn from(p: RefreshModeParam) -> Self {
        match p {
            RefreshModeParam::Standard => RefreshMode::Standard,
            RefreshModeParam::Full => RefreshMode::Full,
        }
    }
}

async fn refresh_item(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<RefreshRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let api_key = state
        .tmdb_api_key
        .as_ref()
        .ok_or(ApiError::BadRequest("TMDB_API_KEY not configured".to_string()))?;

    metadata::refresh_item(&state.db, api_key, &state.image_cache_dir, id, req.mode.into())
        .await
        .map_err(|e| ApiError::Internal(e))?;

    Ok(Json(serde_json::json!({"message": "Metadata refreshed"})))
}

async fn refresh_library(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<RefreshRequest>,
) -> ApiResult<Json<RefreshResult>> {
    let api_key = state
        .tmdb_api_key
        .as_ref()
        .ok_or(ApiError::BadRequest("TMDB_API_KEY not configured".to_string()))?;

    let result = metadata::refresh_library(
        &state.db,
        api_key,
        &state.image_cache_dir,
        id,
        req.mode.into(),
    )
    .await
    .map_err(|e| ApiError::Internal(e))?;

    Ok(Json(result))
}

#[derive(Deserialize)]
struct MetadataOverride {
    title: Option<String>,
    description: Option<String>,
    rating: Option<f64>,
    content_rating: Option<String>,
    tagline: Option<String>,
    release_date: Option<String>,
}

async fn override_metadata(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<MetadataOverride>,
) -> ApiResult<Json<serde_json::Value>> {
    // Verify item exists
    let existing: Option<(i64,)> =
        sqlx::query_as("SELECT id FROM media_items WHERE id = ?")
            .bind(id)
            .fetch_optional(&state.db)
            .await
            .map_err(anyhow::Error::from)?;

    if existing.is_none() {
        return Err(ApiError::NotFound("Media item not found".to_string()));
    }

    if let Some(ref title) = req.title {
        sqlx::query("UPDATE media_items SET title = ? WHERE id = ?")
            .bind(title).bind(id).execute(&state.db).await.map_err(anyhow::Error::from)?;
    }
    if let Some(ref desc) = req.description {
        sqlx::query("UPDATE media_items SET description = ? WHERE id = ?")
            .bind(desc).bind(id).execute(&state.db).await.map_err(anyhow::Error::from)?;
    }
    if let Some(rating) = req.rating {
        sqlx::query("UPDATE media_items SET rating = ? WHERE id = ?")
            .bind(rating).bind(id).execute(&state.db).await.map_err(anyhow::Error::from)?;
    }
    if let Some(ref cr) = req.content_rating {
        sqlx::query("UPDATE media_items SET content_rating = ? WHERE id = ?")
            .bind(cr).bind(id).execute(&state.db).await.map_err(anyhow::Error::from)?;
    }
    if let Some(ref tagline) = req.tagline {
        sqlx::query("UPDATE media_items SET tagline = ? WHERE id = ?")
            .bind(tagline).bind(id).execute(&state.db).await.map_err(anyhow::Error::from)?;
    }
    if let Some(ref rd) = req.release_date {
        sqlx::query("UPDATE media_items SET release_date = ? WHERE id = ?")
            .bind(rd).bind(id).execute(&state.db).await.map_err(anyhow::Error::from)?;
    }

    Ok(Json(serde_json::json!({"message": "Metadata updated"})))
}
