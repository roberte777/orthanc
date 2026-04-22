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
    models::user_preference,
};
use axum::{
    Json, Router,
    extract::State,
    routing::{get, put},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub fn user_router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/profile", get(get_profile).put(update_profile))
        .route("/password", put(change_password))
        .route(
            "/preferences",
            get(get_user_preferences).put(update_user_preferences),
        )
}

pub fn admin_router() -> Router<Arc<AppState>> {
    Router::new().route("/", get(get_server_settings).put(update_server_settings))
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

    if !verify_password(&req.current_password, &user.password_hash).map_err(anyhow::Error::from)? {
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

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UserPreferencesResponse {
    pub preferred_audio_language: Option<String>,
    pub preferred_subtitle_language: Option<String>,
    pub subtitles_enabled_default: bool,
    pub audio_normalize_default: bool,
}

async fn get_user_preferences(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<UserPreferencesResponse>> {
    let user_id: i64 = claims.sub.parse().map_err(|_| ApiError::Unauthorized)?;
    let pref = user_preference::load_preference(&state.db, user_id)
        .await
        .map_err(ApiError::Internal)?;
    let resp = pref
        .map(|p| UserPreferencesResponse {
            preferred_audio_language: p.preferred_audio_language,
            preferred_subtitle_language: p.preferred_subtitle_language,
            subtitles_enabled_default: p.subtitles_enabled_default,
            audio_normalize_default: p.audio_normalize_default,
        })
        .unwrap_or_default();
    Ok(Json(resp))
}

#[derive(Deserialize)]
struct UpdateUserPreferencesRequest {
    preferred_audio_language: Option<String>,
    preferred_subtitle_language: Option<String>,
    subtitles_enabled_default: bool,
    audio_normalize_default: bool,
}

async fn update_user_preferences(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateUserPreferencesRequest>,
) -> ApiResult<Json<UserPreferencesResponse>> {
    let user_id: i64 = claims.sub.parse().map_err(|_| ApiError::Unauthorized)?;
    let audio = req.preferred_audio_language.as_deref().and_then(|s| {
        let t = s.trim();
        if t.is_empty() { None } else { Some(t) }
    });
    let subs = req.preferred_subtitle_language.as_deref().and_then(|s| {
        let t = s.trim();
        if t.is_empty() { None } else { Some(t) }
    });
    user_preference::upsert_preference(
        &state.db,
        user_id,
        audio,
        subs,
        req.subtitles_enabled_default,
        req.audio_normalize_default,
    )
    .await
    .map_err(ApiError::Internal)?;

    Ok(Json(UserPreferencesResponse {
        preferred_audio_language: audio.map(|s| s.to_string()),
        preferred_subtitle_language: subs.map(|s| s.to_string()),
        subtitles_enabled_default: req.subtitles_enabled_default,
        audio_normalize_default: req.audio_normalize_default,
    }))
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
///
/// For API-key settings, an empty value means "fall back to the built-in/env default"
/// so the server ships with working defaults out of the box.
fn default_settings() -> Vec<Setting> {
    vec![
        Setting {
            key: "server_name".into(),
            value: "Orthanc".into(),
            value_type: "string".into(),
            description: Some("Display name for this server".into()),
        },
        Setting {
            key: "library_scan_interval_minutes".into(),
            value: "60".into(),
            value_type: "integer".into(),
            description: Some(
                "Default auto-scan interval for libraries that don't set their own (minutes). Applies on next scan tick."
                    .into(),
            ),
        },
        Setting {
            key: "max_concurrent_streams".into(),
            value: "3".into(),
            value_type: "integer".into(),
            description: Some(
                "Maximum simultaneous video streams across all users. Requires restart.".into(),
            ),
        },
        Setting {
            key: "max_concurrent_transcodes".into(),
            value: "2".into(),
            value_type: "integer".into(),
            description: Some(
                "Maximum simultaneous FFmpeg transcode jobs. Requires restart.".into(),
            ),
        },
        Setting {
            key: "stream_token_expiry_minutes".into(),
            value: "5".into(),
            value_type: "integer".into(),
            description: Some(
                "How long a stream authorization token stays valid before a client must request a new one."
                    .into(),
            ),
        },
        Setting {
            key: "subtitle_cache_max_mb".into(),
            value: "500".into(),
            value_type: "integer".into(),
            description: Some(
                "Maximum size of the extracted-subtitle cache. Enforced on the next sweep."
                    .into(),
            ),
        },
        Setting {
            key: "tmdb_api_key".into(),
            value: String::new(),
            value_type: "string".into(),
            description: Some(
                "Override the built-in TMDB API key. Leave blank to use the shipped default. Requires restart."
                    .into(),
            ),
        },
        Setting {
            key: "tvdb_api_key".into(),
            value: String::new(),
            value_type: "string".into(),
            description: Some(
                "Override the built-in TVDB API key. Leave blank to use the shipped default. Requires restart."
                    .into(),
            ),
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

    // Start with defaults, then overlay stored values *only* for keys we still
    // recognize. This hides rows left behind by past versions of the schema.
    let mut result = default_settings();
    for (key, value, _db_value_type, description) in rows {
        if let Some(s) = result.iter_mut().find(|s| s.key == key) {
            s.value = value;
            if description.is_some() {
                s.description = description;
            }
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
    // Reject unknown keys so the settings table can't be used as a scratchpad.
    let defaults = default_settings();
    let meta = defaults
        .iter()
        .find(|s| s.key == req.key)
        .ok_or_else(|| ApiError::BadRequest(format!("Unknown setting key: {}", req.key)))?;

    sqlx::query(
        "INSERT INTO settings (key, value, value_type, updated_at) VALUES (?, ?, ?, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(&req.key)
    .bind(&req.value)
    .bind(&meta.value_type)
    .execute(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok(Json(serde_json::json!({"message": "Setting updated"})))
}

/// Fetch a setting from the `settings` table by key, returning its value if present.
/// Used by startup and runtime code to read admin-controlled values with sensible fallbacks.
pub async fn read_setting(pool: &crate::db::DbPool, key: &str) -> Option<String> {
    sqlx::query_as::<_, (String,)>("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten()
        .map(|(v,)| v)
}
