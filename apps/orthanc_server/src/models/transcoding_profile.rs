use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct TranscodingProfile {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub container_format: String,
    pub video_codec: Option<String>,
    pub video_bitrate_kbps: Option<i32>,
    pub video_width: Option<i32>,
    pub video_height: Option<i32>,
    pub video_frame_rate: Option<f64>,
    pub audio_codec: Option<String>,
    pub audio_bitrate_kbps: Option<i32>,
    pub audio_channels: Option<i32>,
    pub audio_sample_rate: Option<i32>,
    pub is_default: bool,
}
