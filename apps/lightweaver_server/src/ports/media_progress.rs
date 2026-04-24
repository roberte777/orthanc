use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct UserMediaProgress {
    pub user_id: i64,
    pub media_item_id: i64,
    pub playback_position_seconds: i64,
    pub is_completed: bool,
    pub completed_at: Option<DateTime<Utc>>,
    pub last_updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpsertMediaProgress {
    pub playback_position_seconds: i64,
    pub is_completed: bool,
}

#[async_trait]
pub trait MediaProgressRepository: Send + Sync {
    async fn upsert(
        &self,
        user_id: i64,
        media_item_id: i64,
        input: UpsertMediaProgress,
    ) -> Result<UserMediaProgress>;
    async fn find(&self, user_id: i64, media_item_id: i64) -> Result<Option<UserMediaProgress>>;
    async fn list_recent_for_user(
        &self,
        user_id: i64,
        limit: i64,
    ) -> Result<Vec<UserMediaProgress>>;
    async fn list_in_progress_for_user(
        &self,
        user_id: i64,
        limit: i64,
    ) -> Result<Vec<UserMediaProgress>>;
    async fn mark_completed(&self, user_id: i64, media_item_id: i64) -> Result<()>;
    async fn delete(&self, user_id: i64, media_item_id: i64) -> Result<bool>;
}
