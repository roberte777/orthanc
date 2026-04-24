use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct TranscodingProfile {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub container_format: String,
    pub video_codec: Option<String>,
    pub video_bitrate_kbps: Option<i64>,
    pub video_width: Option<i64>,
    pub video_height: Option<i64>,
    pub video_frame_rate: Option<f64>,
    pub audio_codec: Option<String>,
    pub audio_bitrate_kbps: Option<i64>,
    pub audio_channels: Option<i64>,
    pub audio_sample_rate: Option<i64>,
    pub is_default: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewTranscodingProfile {
    pub name: String,
    pub description: Option<String>,
    pub container_format: String,
    pub video_codec: Option<String>,
    pub video_bitrate_kbps: Option<i64>,
    pub video_width: Option<i64>,
    pub video_height: Option<i64>,
    pub video_frame_rate: Option<f64>,
    pub audio_codec: Option<String>,
    pub audio_bitrate_kbps: Option<i64>,
    pub audio_channels: Option<i64>,
    pub audio_sample_rate: Option<i64>,
    pub is_default: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateTranscodingProfile {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub container_format: Option<String>,
    pub video_codec: Option<Option<String>>,
    pub video_bitrate_kbps: Option<Option<i64>>,
    pub video_width: Option<Option<i64>>,
    pub video_height: Option<Option<i64>>,
    pub video_frame_rate: Option<Option<f64>>,
    pub audio_codec: Option<Option<String>>,
    pub audio_bitrate_kbps: Option<Option<i64>>,
    pub audio_channels: Option<Option<i64>>,
    pub audio_sample_rate: Option<Option<i64>>,
}

#[async_trait]
pub trait TranscodingProfileRepository: Send + Sync {
    async fn create(&self, input: NewTranscodingProfile) -> Result<TranscodingProfile>;
    async fn find_by_id(&self, id: i64) -> Result<Option<TranscodingProfile>>;
    async fn find_by_name(&self, name: &str) -> Result<Option<TranscodingProfile>>;
    async fn get_default(&self) -> Result<Option<TranscodingProfile>>;
    async fn list(&self) -> Result<Vec<TranscodingProfile>>;
    async fn update(&self, id: i64, input: UpdateTranscodingProfile) -> Result<TranscodingProfile>;
    async fn set_default(&self, id: i64) -> Result<()>;
    async fn delete(&self, id: i64) -> Result<bool>;
}
