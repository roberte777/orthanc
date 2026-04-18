use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct UserSession {
    pub id: i64,
    pub user_id: i64,
    pub refresh_token_hash: String,
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    pub last_used_at: Option<String>,
    pub is_revoked: bool,
}

#[derive(Debug, Serialize)]
pub struct SessionResponse {
    pub id: i64,
    pub device_name: Option<String>,
    pub ip_address: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

impl From<UserSession> for SessionResponse {
    fn from(s: UserSession) -> Self {
        Self {
            id: s.id,
            device_name: s.device_name,
            ip_address: s.ip_address,
            created_at: s.created_at,
            last_used_at: s.last_used_at,
        }
    }
}
