use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Session {
    pub id: i64,
    pub user_id: i64,
    pub refresh_token_hash: String,
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
    pub is_revoked: bool,
}

#[derive(Debug, Clone)]
pub struct NewSession {
    pub user_id: i64,
    pub refresh_token_hash: String,
    pub device_name: Option<String>,
    pub device_id: Option<String>,
    pub ip_address: Option<String>,
    pub user_agent: Option<String>,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait SessionRepository: Send + Sync {
    async fn create(&self, input: NewSession) -> Result<Session>;
    async fn find_by_id(&self, id: i64) -> Result<Option<Session>>;
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<Session>>;
    async fn list_active_for_user(&self, user_id: i64) -> Result<Vec<Session>>;
    async fn touch_last_used(&self, id: i64) -> Result<()>;
    async fn revoke(&self, id: i64) -> Result<bool>;
    async fn revoke_all_for_user(&self, user_id: i64) -> Result<u64>;
    async fn delete(&self, id: i64) -> Result<bool>;
    async fn delete_expired(&self) -> Result<u64>;
}
