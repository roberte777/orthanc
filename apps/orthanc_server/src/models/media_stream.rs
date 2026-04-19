use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct MediaStream {
    pub id: i64,
    pub media_item_id: i64,
    pub stream_index: i32,
    pub stream_type: String,
    pub codec: Option<String>,
    pub language: Option<String>,
    pub title: Option<String>,
    pub is_default: bool,
    pub is_forced: bool,
    // Video
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub aspect_ratio: Option<String>,
    pub frame_rate: Option<f64>,
    pub bit_depth: Option<i32>,
    pub color_space: Option<String>,
    // Audio
    pub channels: Option<i32>,
    pub sample_rate: Option<i32>,
    pub bit_rate: Option<i32>,
    // Subtitle
    pub is_external: bool,
    pub external_file_path: Option<String>,
}
