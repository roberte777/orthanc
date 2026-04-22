use crate::{
    api::{
        error::{ApiError, ApiResult},
        state::AppState,
    },
    auth::{
        jwt::create_access_token,
        middleware::AuthUser,
        password::{hash_password, verify_password},
        tokens::{generate_refresh_token, hash_token},
    },
    models::{session::SessionResponse, user::UserResponse},
};
use axum::{
    Json, Router,
    extract::{Path, State},
    routing::{delete, get, post},
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/setup", post(setup))
        .route("/login", post(login))
        .route("/refresh", post(refresh))
        .route("/logout", post(logout))
        .route("/me", get(me))
        .route("/sessions", get(sessions))
        .route("/sessions/{id}", delete(revoke_session))
}

#[derive(Deserialize)]
struct SetupRequest {
    username: String,
    email: String,
    password: String,
    display_name: Option<String>,
}

#[derive(Deserialize)]
struct LoginRequest {
    username: String,
    password: String,
    device_name: Option<String>,
}

#[derive(Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[derive(Deserialize)]
struct LogoutRequest {
    refresh_token: String,
}

#[derive(Serialize)]
struct AuthResponse {
    access_token: String,
    refresh_token: String,
    user: UserResponse,
}

async fn setup(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetupRequest>,
) -> ApiResult<Json<AuthResponse>> {
    // Check if any users exist
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    if count > 0 {
        return Err(ApiError::Conflict("Setup already complete".to_string()));
    }

    let password_hash = hash_password(&req.password)?;

    let user = sqlx::query_as::<_, crate::models::user::User>(
        "INSERT INTO users (username, email, password_hash, display_name, is_admin)
         VALUES (?, ?, ?, ?, 1)
         RETURNING *",
    )
    .bind(&req.username)
    .bind(&req.email)
    .bind(&password_hash)
    .bind(&req.display_name)
    .fetch_one(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    let (access_token, refresh_token) = create_tokens(&user, &state, None).await?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token,
        user: user.into(),
    }))
}

async fn login(
    State(state): State<Arc<AppState>>,
    Json(req): Json<LoginRequest>,
) -> ApiResult<Json<AuthResponse>> {
    let user = sqlx::query_as::<_, crate::models::user::User>(
        "SELECT * FROM users WHERE username = ? AND is_active = 1",
    )
    .bind(&req.username)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(ApiError::Unauthorized)?;

    if !verify_password(&req.password, &user.password_hash)? {
        return Err(ApiError::Unauthorized);
    }

    // Update last_login_at
    sqlx::query("UPDATE users SET last_login_at = datetime('now') WHERE id = ?")
        .bind(user.id)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    let (access_token, refresh_token) =
        create_tokens(&user, &state, req.device_name.as_deref()).await?;

    Ok(Json(AuthResponse {
        access_token,
        refresh_token,
        user: user.into(),
    }))
}

async fn refresh(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RefreshRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let token_hash = hash_token(&req.refresh_token);

    let session = sqlx::query_as::<_, crate::models::session::UserSession>(
        "SELECT * FROM user_sessions WHERE refresh_token_hash = ? AND is_revoked = 0 AND expires_at > datetime('now')",
    )
    .bind(&token_hash)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(ApiError::Unauthorized)?;

    let user = sqlx::query_as::<_, crate::models::user::User>(
        "SELECT * FROM users WHERE id = ? AND is_active = 1",
    )
    .bind(session.user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(ApiError::Unauthorized)?;

    // Revoke old token
    sqlx::query("UPDATE user_sessions SET is_revoked = 1 WHERE id = ?")
        .bind(session.id)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    // Issue new tokens
    let (access_token, new_refresh_token) =
        create_tokens(&user, &state, session.device_name.as_deref()).await?;

    Ok(Json(serde_json::json!({
        "access_token": access_token,
        "refresh_token": new_refresh_token,
    })))
}

async fn logout(
    AuthUser(_claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Json(req): Json<LogoutRequest>,
) -> ApiResult<Json<serde_json::Value>> {
    let token_hash = hash_token(&req.refresh_token);
    sqlx::query("UPDATE user_sessions SET is_revoked = 1 WHERE refresh_token_hash = ?")
        .bind(&token_hash)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;
    Ok(Json(serde_json::json!({"message": "Logged out"})))
}

async fn me(
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

async fn sessions(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
) -> ApiResult<Json<Vec<SessionResponse>>> {
    let user_id: i64 = claims.sub.parse().map_err(|_| ApiError::Unauthorized)?;
    let sessions = sqlx::query_as::<_, crate::models::session::UserSession>(
        "SELECT * FROM user_sessions WHERE user_id = ? AND is_revoked = 0 AND expires_at > datetime('now') ORDER BY last_used_at DESC",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .map_err(anyhow::Error::from)?;
    Ok(Json(sessions.into_iter().map(Into::into).collect()))
}

async fn revoke_session(
    AuthUser(claims): AuthUser,
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<i64>,
) -> ApiResult<Json<serde_json::Value>> {
    let user_id: i64 = claims.sub.parse().map_err(|_| ApiError::Unauthorized)?;

    let session = sqlx::query_as::<_, crate::models::session::UserSession>(
        "SELECT * FROM user_sessions WHERE id = ?",
    )
    .bind(session_id)
    .fetch_optional(&state.db)
    .await
    .map_err(anyhow::Error::from)?
    .ok_or(ApiError::NotFound("Session not found".to_string()))?;

    // Only allow revoking own sessions (or admin can revoke any)
    if session.user_id != user_id && !claims.is_admin {
        return Err(ApiError::Forbidden);
    }

    sqlx::query("UPDATE user_sessions SET is_revoked = 1 WHERE id = ?")
        .bind(session_id)
        .execute(&state.db)
        .await
        .map_err(anyhow::Error::from)?;

    Ok(Json(serde_json::json!({"message": "Session revoked"})))
}

async fn create_tokens(
    user: &crate::models::user::User,
    state: &AppState,
    device_name: Option<&str>,
) -> ApiResult<(String, String)> {
    let access_token = create_access_token(
        user.id,
        &user.username,
        &user.email,
        user.is_admin,
        &state.jwt_secret,
        state.access_token_expiry,
    )?;

    let refresh_token = generate_refresh_token();
    let token_hash = hash_token(&refresh_token);

    let expires_at =
        chrono::Utc::now() + chrono::Duration::seconds(state.refresh_token_expiry as i64);

    sqlx::query(
        "INSERT INTO user_sessions (user_id, refresh_token_hash, device_name, expires_at)
         VALUES (?, ?, ?, ?)",
    )
    .bind(user.id)
    .bind(&token_hash)
    .bind(device_name)
    .bind(expires_at.format("%Y-%m-%d %H:%M:%S").to_string())
    .execute(&state.db)
    .await
    .map_err(anyhow::Error::from)?;

    Ok((access_token, refresh_token))
}
