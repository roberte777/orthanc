use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StreamType {
    Video,
    Audio,
    Subtitle,
}

#[derive(Debug, Clone)]
pub struct MediaStream {
    pub id: i64,
    pub media_item_id: i64,
    pub stream_index: i64,
    pub stream_type: StreamType,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub aspect_ratio: Option<String>,
    pub frame_rate: Option<f64>,
    pub bit_depth: Option<i64>,
    pub color_space: Option<String>,
    pub channels: Option<i64>,
    pub sample_rate: Option<i64>,
    pub bit_rate: Option<i64>,
    pub is_external: bool,
    pub external_file_path: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewMediaStream {
    pub media_item_id: i64,
    pub stream_index: i64,
    pub stream_type: StreamType,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub aspect_ratio: Option<String>,
    pub frame_rate: Option<f64>,
    pub bit_depth: Option<i64>,
    pub color_space: Option<String>,
    pub channels: Option<i64>,
    pub sample_rate: Option<i64>,
    pub bit_rate: Option<i64>,
    pub is_external: bool,
    pub external_file_path: Option<String>,
}

#[async_trait]
pub trait MediaStreamRepository: Send + Sync {
    async fn create(&self, input: NewMediaStream) -> Result<MediaStream>;
    async fn find_by_id(&self, id: i64) -> Result<Option<MediaStream>>;
    async fn list_for_media_item(&self, media_item_id: i64) -> Result<Vec<MediaStream>>;
    async fn replace_for_media_item(
        &self,
        media_item_id: i64,
        streams: Vec<NewMediaStream>,
    ) -> Result<Vec<MediaStream>>;
    async fn delete(&self, id: i64) -> Result<bool>;
    async fn delete_for_media_item(&self, media_item_id: i64) -> Result<u64>;
}
