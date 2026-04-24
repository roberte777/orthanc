use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct UserTrackPreferences {
    pub user_id: i64,
    pub scope_media_item_id: i64,
    pub audio_language: Option<String>,
    pub subtitle_language: Option<String>,
    pub subtitles_enabled: bool,
    pub audio_normalize: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpsertTrackPreferences {
    pub audio_language: Option<String>,
    pub subtitle_language: Option<String>,
    pub subtitles_enabled: bool,
    pub audio_normalize: bool,
}

#[async_trait]
pub trait TrackPreferencesRepository: Send + Sync {
    async fn upsert(
        &self,
        user_id: i64,
        scope_media_item_id: i64,
        input: UpsertTrackPreferences,
    ) -> Result<UserTrackPreferences>;
    async fn find(
        &self,
        user_id: i64,
        scope_media_item_id: i64,
    ) -> Result<Option<UserTrackPreferences>>;
    async fn list_for_user(&self, user_id: i64) -> Result<Vec<UserTrackPreferences>>;
    async fn delete(&self, user_id: i64, scope_media_item_id: i64) -> Result<bool>;
    async fn delete_for_user(&self, user_id: i64) -> Result<u64>;
}
