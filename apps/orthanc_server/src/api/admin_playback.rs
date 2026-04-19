use crate::{
    api::{error::ApiResult, state::AppState},
    auth::middleware::AdminUser,
    transcoding::ActiveSessionInfo,
};
use axum::{
    extract::{Path, State},
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/sessions", get(list_sessions))
        .route("/sessions/{id}", axum::routing::delete(stop_session))
}

#[derive(Serialize)]
struct ActiveStreamRow {
    #[serde(flatten)]
    info: ActiveSessionInfo,
    username: Option<String>,
    media_title: Option<String>,
}

async fn list_sessions(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<ActiveStreamRow>>> {
    let sessions = state.transcode_manager.list_sessions().await;
    if sessions.is_empty() {
        return Ok(Json(Vec::new()));
    }

    let user_ids: Vec<i64> = sessions.iter().map(|s| s.user_id).collect();
    let media_ids: Vec<i64> = sessions.iter().map(|s| s.media_item_id).collect();

    let usernames = fetch_usernames(&state.db, &user_ids).await?;
    let titles = fetch_media_titles(&state.db, &media_ids).await?;

    let rows = sessions
        .into_iter()
        .map(|info| ActiveStreamRow {
            username: usernames.get(&info.user_id).cloned(),
            media_title: titles.get(&info.media_item_id).cloned(),
            info,
        })
        .collect();

    Ok(Json(rows))
}

async fn stop_session(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<serde_json::Value>> {
    state.transcode_manager.stop_session(&id).await;
    Ok(Json(serde_json::json!({"message": "Session stopped"})))
}

async fn fetch_usernames(
    db: &crate::db::DbPool,
    ids: &[i64],
) -> ApiResult<HashMap<i64, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!("SELECT id, username FROM users WHERE id IN ({})", placeholders);
    let mut q = sqlx::query_as::<_, (i64, String)>(&sql);
    for id in ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(db).await.map_err(anyhow::Error::from)?;
    Ok(rows.into_iter().collect())
}

async fn fetch_media_titles(
    db: &crate::db::DbPool,
    ids: &[i64],
) -> ApiResult<HashMap<i64, String>> {
    if ids.is_empty() {
        return Ok(HashMap::new());
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    let sql = format!("SELECT id, title FROM media_items WHERE id IN ({})", placeholders);
    let mut q = sqlx::query_as::<_, (i64, String)>(&sql);
    for id in ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(db).await.map_err(anyhow::Error::from)?;
    Ok(rows.into_iter().collect())
}
