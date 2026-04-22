use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::AppState,
    },
    auth::{middleware::AdminUser, password::hash_password},
    models::user::{CreateUserRequest, UpdateUserRequest, UserResponse},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::get,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(list_users).post(create_user))
        .route("/{id}", get(get_user).put(update_user).delete(delete_user))
}

#[derive(Deserialize)]
struct Pagination {
    #[serde(default = "default_page")]
    page: u32,
    #[serde(default = "default_per_page")]
    per_page: u32,
}

fn default_page() -> u32 {
    1
}
fn default_per_page() -> u32 {
    20
}

#[derive(Serialize)]
struct PaginatedUsers {
    users: Vec<UserResponse>,
    total: i64,
    page: u32,
    per_page: u32,
}

async fn list_users(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Query(pagination): Query<Pagination>,
) -> ApiResult<Json<PaginatedUsers>> {
    let offset = (pagination.page - 1) * pagination.per_page;

    let (total,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    let users = sqlx::query_as::<_, crate::models::user::User>(
        "SELECT * FROM users ORDER BY created_at DESC LIMIT ? OFFSET ?",
    )
    .bind(pagination.per_page)
    .bind(offset)
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(PaginatedUsers {
        users: users.into_iter().map(Into::into).collect(),
        total,
        page: pagination.page,
        per_page: pagination.per_page,
    }))
}

async fn create_user(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateUserRequest>,
) -> ApiResult<Json<UserResponse>> {
    let password_hash = hash_password(&req.password).map_err(anyhow::Error::from)?;

    let user = sqlx::query_as::<_, crate::models::user::User>(
        "INSERT INTO users (username, email, password_hash, display_name, is_admin)
         VALUES (?, ?, ?, ?, ?)
         RETURNING *",
    )
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
    .bind(&req.display_name)
    .bind(req.is_admin)
    .fetch_one(&state.db)
    .await
    .map_err(|e| {
        if e.to_string().contains("UNIQUE constraint failed") {
            ApiError::Conflict("Username or email already exists".to_string())
        } else {
            ApiError::Internal(anyhow::Error::from(e))
        }
    })?;

    Ok(Json(user.into()))
}

async fn get_user(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<UserResponse>> {
    let user = sqlx::query_as::<_, crate::models::user::User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;
    Ok(Json(user.into()))
}

async fn update_user(
    AdminUser(admin_claims): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<UpdateUserRequest>,
) -> ApiResult<Json<UserResponse>> {
    let admin_id: i64 = admin_claims
        .sub
        .parse()
        .map_err(|_| ApiError::Unauthorized)?;

    // Prevent demoting self from admin or deactivating own account
    if id == admin_id {
        if let Some(false) = req.is_admin {
            return Err(ApiError::BadRequest(
                "Cannot demote your own admin role".to_string(),
            ));
        }
        if let Some(false) = req.is_active {
            return Err(ApiError::BadRequest(
                "Cannot deactivate your own account".to_string(),
            ));
        }
    }

    let user = sqlx::query_as::<_, crate::models::user::User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;

    let email = req.email.as_deref().unwrap_or(&user.email);
    let display_name = req.display_name.or(user.display_name);
    let is_admin = req.is_admin.unwrap_or(user.is_admin);
    let is_active = req.is_active.unwrap_or(user.is_active);

    // If deactivating, revoke all sessions
    if !is_active && user.is_active {
        sqlx::query("UPDATE user_sessions SET is_revoked = 1 WHERE user_id = ?")
            .bind(id)
            .execute(&state.db)
            .await
            .map_err(anyhow::Error::from)?;
    }

    let updated = sqlx::query_as::<_, crate::models::user::User>(
        "UPDATE users SET email = ?, display_name = ?, is_admin = ?, is_active = ?, updated_at = datetime('now')
         WHERE id = ? RETURNING *",
    )
    .bind(email)
    .bind(&display_name)
    .bind(is_admin)
    .bind(is_active)
    .bind(id)
    .fetch_one(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(updated.into()))
}

async fn delete_user(
    AdminUser(admin_claims): AdminUser,
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let admin_id: i64 = admin_claims
        .sub
        .parse()
        .map_err(|_| ApiError::Unauthorized)?;

    if id == admin_id {
        return Err(ApiError::BadRequest(
            "Cannot delete your own account".to_string(),
        ));
    }

    sqlx::query_as::<_, crate::models::user::User>("SELECT * FROM users WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;

    // Revoke sessions first (CASCADE should handle it, but be explicit)
    sqlx::query("DELETE FROM user_sessions WHERE user_id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    sqlx::query("DELETE FROM users WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(serde_json::json!({"message": "User deleted"})))
}
