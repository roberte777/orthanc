use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::AppState,
    },
    auth::middleware::AuthUser,
    db::DbPool,
    models::media::{ImageRecord, MediaItem, MediaItemResponse},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::get,
};
use serde::Serialize;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/recent", get(recent_media))
        .route("/movies", get(list_movies))
        .route("/movies/{id}", get(get_movie))
        .route("/shows", get(list_shows))
        .route("/shows/{id}", get(get_show))
}

/// Enrich a MediaItemResponse with poster/backdrop URLs and genres from the DB.
async fn enrich(db: &DbPool, resp: &mut MediaItemResponse) -> Result<(), anyhow::Error> {
    // Images
    let images = sqlx::query_as::<_, ImageRecord>(
        "SELECT id, media_item_id, image_type, file_path FROM images WHERE media_item_id = ? AND is_primary = 1",
    )
    .bind(resp.id)
    .fetch_all(db)
    .await?;

    for img in &images {
        let url = img.file_path.as_ref().map(|p| format!("/api/images/{}", p));
        match img.image_type.as_str() {
            "poster" => resp.poster_url = url,
            "backdrop" | "thumbnail" => resp.backdrop_url = url.or(resp.backdrop_url.take()),
            _ => {}
        }
    }

    // Genres
    let genres: Vec<(String,)> = sqlx::query_as(
        "SELECT g.name FROM genres g JOIN media_genres mg ON g.id = mg.genre_id WHERE mg.media_item_id = ?",
    )
    .bind(resp.id)
    .fetch_all(db)
    .await?;

    if !genres.is_empty() {
        resp.genres = Some(genres.into_iter().map(|(n,)| n).collect());
    }

    Ok(())
}

/// Enrich a list of responses (light version — images only, no deep tree enrichment).
async fn enrich_list(db: &DbPool, items: &mut [MediaItemResponse]) -> Result<(), anyhow::Error> {
    for item in items.iter_mut() {
        enrich(db, item).await?;
    }
    Ok(())
}

#[derive(Serialize)]
struct RecentMedia {
    movies: Vec<MediaItemResponse>,
    shows: Vec<MediaItemResponse>,
}

async fn recent_media(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<RecentMedia>> {
    let movies_raw = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE media_type = 'movie' ORDER BY date_added DESC LIMIT 20",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let shows_raw = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE media_type = 'tv_show' ORDER BY date_added DESC LIMIT 20",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let mut movies: Vec<MediaItemResponse> = movies_raw.into_iter().map(Into::into).collect();
    let mut shows: Vec<MediaItemResponse> = shows_raw.into_iter().map(Into::into).collect();

    enrich_list(&state.db, &mut movies).await?;
    enrich_list(&state.db, &mut shows).await?;

    Ok(Json(RecentMedia { movies, shows }))
}

async fn list_movies(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<MediaItemResponse>>> {
    let raw = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE media_type = 'movie' ORDER BY sort_title",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let mut items: Vec<MediaItemResponse> = raw.into_iter().map(Into::into).collect();
    enrich_list(&state.db, &mut items).await?;

    Ok(Json(items))
}

async fn list_shows(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<MediaItemResponse>>> {
    let raw = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE media_type = 'tv_show' ORDER BY sort_title",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let mut items: Vec<MediaItemResponse> = raw.into_iter().map(Into::into).collect();
    enrich_list(&state.db, &mut items).await?;

    Ok(Json(items))
}

async fn get_movie(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<MediaItemResponse>> {
    let movie = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE id = ? AND media_type = 'movie'",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(ApiError::NotFound("Movie not found".to_string()))?;

    let mut resp: MediaItemResponse = movie.into();
    enrich(&state.db, &mut resp).await?;

    Ok(Json(resp))
}

async fn get_show(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<MediaItemResponse>> {
    let show = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE id = ? AND media_type = 'tv_show'",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(ApiError::NotFound("Show not found".to_string()))?;

    let mut resp: MediaItemResponse = show.into();
    enrich(&state.db, &mut resp).await?;

    // Load seasons with episodes
    let seasons = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'season' ORDER BY season_number",
    )
    .bind(id)
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let mut season_resps = Vec::new();
    for season in seasons {
        let mut season_resp: MediaItemResponse = season.clone().into();
        enrich(&state.db, &mut season_resp).await?;

        let episodes = sqlx::query_as::<_, MediaItem>(
            "SELECT * FROM media_items WHERE parent_id = ? AND media_type = 'episode' ORDER BY episode_number",
        )
        .bind(season.id)
        .fetch_all(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

        let mut ep_resps: Vec<MediaItemResponse> = episodes.into_iter().map(Into::into).collect();
        enrich_list(&state.db, &mut ep_resps).await?;
        season_resp.children = Some(ep_resps);
        season_resps.push(season_resp);
    }
    resp.children = Some(season_resps);

    Ok(Json(resp))
}
