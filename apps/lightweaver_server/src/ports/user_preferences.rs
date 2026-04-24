use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct UserPreferences {
    pub user_id: i64,
    pub preferred_audio_language: Option<String>,
    pub preferred_subtitle_language: Option<String>,
    pub subtitles_enabled_default: bool,
    pub audio_normalize_default: bool,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct UpsertUserPreferences {
    pub preferred_audio_language: Option<String>,
    pub preferred_subtitle_language: Option<String>,
    pub subtitles_enabled_default: bool,
    pub audio_normalize_default: bool,
}

#[async_trait]
pub trait UserPreferencesRepository: Send + Sync {
    async fn upsert(&self, user_id: i64, input: UpsertUserPreferences) -> Result<UserPreferences>;
    async fn find(&self, user_id: i64) -> Result<Option<UserPreferences>>;
    async fn delete(&self, user_id: i64) -> Result<bool>;
}
