use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::AppState,
    },
    auth::middleware::AuthUser,
    models::media::{MediaItem, MediaItemResponse},
};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
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

#[derive(Serialize)]
struct RecentMedia {
    movies: Vec<MediaItemResponse>,
    shows: Vec<MediaItemResponse>,
}

async fn recent_media(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<RecentMedia>> {
    let movies = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE media_type = 'movie' ORDER BY date_added DESC LIMIT 20",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let shows = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE media_type = 'tv_show' ORDER BY date_added DESC LIMIT 20",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(RecentMedia {
        movies: movies.into_iter().map(Into::into).collect(),
        shows: shows.into_iter().map(Into::into).collect(),
    }))
}

async fn list_movies(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<MediaItemResponse>>> {
    let movies = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE media_type = 'movie' ORDER BY sort_title",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(movies.into_iter().map(Into::into).collect()))
}

async fn list_shows(
    AuthUser(_): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<MediaItemResponse>>> {
    let shows = sqlx::query_as::<_, MediaItem>(
        "SELECT * FROM media_items WHERE media_type = 'tv_show' ORDER BY sort_title",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(shows.into_iter().map(Into::into).collect()))
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

    Ok(Json(movie.into()))
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

    Ok(Json(resp))
}
