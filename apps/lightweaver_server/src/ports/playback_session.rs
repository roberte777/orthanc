use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct PlaybackSession {
    pub id: i64,
    pub user_id: i64,
    pub media_item_id: i64,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub duration_watched_seconds: Option<i64>,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewPlaybackSession {
    pub user_id: i64,
    pub media_item_id: i64,
    pub client_name: Option<String>,
    pub client_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EndPlaybackSession {
    pub ended_at: DateTime<Utc>,
    pub duration_watched_seconds: i64,
}

#[async_trait]
pub trait PlaybackSessionRepository: Send + Sync {
    async fn start(&self, input: NewPlaybackSession) -> Result<PlaybackSession>;
    async fn end(&self, id: i64, input: EndPlaybackSession) -> Result<PlaybackSession>;
    async fn find_by_id(&self, id: i64) -> Result<Option<PlaybackSession>>;
    async fn list_for_user(&self, user_id: i64, limit: i64) -> Result<Vec<PlaybackSession>>;
    async fn list_for_media(&self, media_item_id: i64, limit: i64) -> Result<Vec<PlaybackSession>>;
    async fn list_active_for_user(&self, user_id: i64) -> Result<Vec<PlaybackSession>>;
    async fn delete(&self, id: i64) -> Result<bool>;
}
