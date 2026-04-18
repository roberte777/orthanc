use crate::api::{error::ApiError, state::AppState};
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct AuthUser(pub crate::auth::jwt::Claims);

#[derive(Debug, Clone)]
pub struct AdminUser(pub crate::auth::jwt::Claims);

impl FromRequestParts<Arc<AppState>> for AuthUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get("Authorization")
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized)?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized)?;

        let claims = crate::auth::jwt::validate_access_token(token, &state.jwt_secret)
            .map_err(|_| ApiError::Unauthorized)?;

        Ok(AuthUser(claims))
    }
}

impl FromRequestParts<Arc<AppState>> for AdminUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let AuthUser(claims) = AuthUser::from_request_parts(parts, state).await?;
        if !claims.is_admin {
            return Err(ApiError::Forbidden);
        }
        Ok(AdminUser(claims))
    }
}
