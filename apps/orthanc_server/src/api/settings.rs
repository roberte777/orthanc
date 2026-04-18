use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::AppState,
    },
    auth::{
        middleware::{AdminUser, AuthUser},
        password::{hash_password, verify_password},
    },
    models::user::UserResponse,
};
use axum::{
    extract::State,
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub fn user_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/profile", get(get_profile).put(update_profile))
        .route("/password", put(change_password))
}

pub fn admin_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/", get(get_server_settings).put(update_server_settings))
}

async fn get_profile(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<UserResponse>> {
    let user_id: i64 = claims.sub.parse().map_err(|_| ApiError::Unauthorized)?;
    let user = sqlx::query_as::<_, crate::models::user::User>("SELECT * FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;
    Ok(Json(user.into()))
}

#[derive(Deserialize)]
struct UpdateProfileRequest {
    display_name: Option<String>,
    email: Option<String>,
}

async fn update_profile(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateProfileRequest>,
) -> ApiResult<Json<UserResponse>> {
    let user_id: i64 = claims.sub.parse().map_err(|_| ApiError::Unauthorized)?;

    let user = sqlx::query_as::<_, crate::models::user::User>("SELECT * FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;

    let email = req.email.as_deref().unwrap_or(&user.email);
    let display_name = req.display_name.or(user.display_name);

    let updated = sqlx::query_as::<_, crate::models::user::User>(
        "UPDATE users SET email = ?, display_name = ?, updated_at = datetime('now') WHERE id = ? RETURNING *",
    )
    .bind(email)
    .bind(&display_name)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(updated.into()))
}

#[derive(Deserialize)]
struct ChangePasswordRequest {
    current_password: String,
    new_password: String,
}

async fn change_password(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<ChangePasswordRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let user_id: i64 = claims.sub.parse().map_err(|_| ApiError::Unauthorized)?;

    let user = sqlx::query_as::<_, crate::models::user::User>("SELECT * FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_optional(&state.db)
        .await
        .map_err(anyhow::Error::from)?
        .ok_or(ApiError::NotFound("User not found".to_string()))?;

    if !verify_password(&req.current_password, &user.password_hash)
        .map_err(anyhow::Error::from)?
    {
        return Err(ApiError::BadRequest(
            "Current password is incorrect".to_string(),
        ));
    }

    let new_hash = hash_password(&req.new_password).map_err(anyhow::Error::from)?;

    sqlx::query("UPDATE users SET password_hash = ?, updated_at = datetime('now') WHERE id = ?")
        .bind(&new_hash)
        .bind(user_id)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(serde_json::json!({"message": "Password updated"})))
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Setting {
    pub key: String,
    pub value: String,
    pub value_type: String,
    pub description: Option<String>,
}

/// Known server settings with their defaults.
/// These are returned even if not yet stored in the DB.
fn default_settings() -> Vec<Setting> {
    vec![
        Setting {
            key: "server_name".into(),
            value: "Orthanc".into(),
            value_type: "string".into(),
            description: Some("Display name for this server".into()),
        },
        Setting {
            key: "transcoding_enabled".into(),
            value: "false".into(),
            value_type: "boolean".into(),
            description: Some("Enable on-the-fly video transcoding".into()),
        },
        Setting {
            key: "default_quality".into(),
            value: "1080p".into(),
            value_type: "string".into(),
            description: Some("Default streaming quality (480p, 720p, 1080p, 4k)".into()),
        },
        Setting {
            key: "library_scan_interval_minutes".into(),
            value: "60".into(),
            value_type: "integer".into(),
            description: Some("How often to auto-scan media libraries (minutes)".into()),
        },
        Setting {
            key: "allow_guest_access".into(),
            value: "false".into(),
            value_type: "boolean".into(),
            description: Some("Allow unauthenticated users to browse media".into()),
        },
    ]
}

async fn get_server_settings(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<Setting>>> {
    let rows = sqlx::query_as::<_, (String, String, String, Option<String>)>(
        "SELECT key, value, value_type, description FROM settings",
    )
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    // Start with defaults, then overlay with whatever is stored in the DB
    let mut result = default_settings();
    for (key, value, value_type, description) in rows {
        if let Some(s) = result.iter_mut().find(|s| s.key == key) {
            s.value = value;
            s.value_type = value_type;
            if description.is_some() {
                s.description = description;
            }
        } else {
            result.push(Setting { key, value, value_type, description });
        }
    }

    Ok(Json(result))
}

#[derive(Deserialize)]
struct UpdateSettingRequest {
    key: String,
    value: String,
}

async fn update_server_settings(
    AdminUser(_): AdminUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateSettingRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    // Look up value_type from defaults, fall back to "string"
    let value_type = default_settings()
        .into_iter()
        .find(|s| s.key == req.key)
        .map(|s| s.value_type)
        .unwrap_or_else(|| "string".into());

    sqlx::query(
        "INSERT INTO settings (key, value, value_type, updated_at) VALUES (?, ?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(&req.key)
    .bind(&req.value)
    .bind(&value_type)
    .execute(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(serde_json::json!({"message": "Setting updated"})))
}
