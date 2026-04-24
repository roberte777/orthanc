use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct PasswordResetToken {
    pub id: i64,
    pub user_id: i64,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub used_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewPasswordResetToken {
    pub user_id: i64,
    pub token_hash: String,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait PasswordResetRepository: Send + Sync {
    async fn create(&self, input: NewPasswordResetToken) -> Result<PasswordResetToken>;
    async fn find_by_token_hash(&self, token_hash: &str) -> Result<Option<PasswordResetToken>>;
    async fn mark_used(&self, id: i64) -> Result<()>;
    async fn delete_expired(&self) -> Result<u64>;
    async fn delete_all_for_user(&self, user_id: i64) -> Result<u64>;
}
