use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct MediaItem {
    pub id: i64,
    pub library_id: Option<i64>,
    pub media_type: String,
    pub title: String,
    pub sort_title: Option<String>,
    pub original_title: Option<String>,
    pub description: Option<String>,
    pub release_date: Option<String>,
    pub duration_seconds: Option<i32>,
    pub file_path: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub container_format: Option<String>,
    pub rating: Option<f64>,
    pub content_rating: Option<String>,
    pub tagline: Option<String>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub tvdb_id: Option<String>,
    pub parent_id: Option<i64>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub file_hash: Option<String>,
    pub date_added: String,
    pub date_modified: Option<String>,
    pub last_scanned_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct MediaItemResponse {
    pub id: i64,
    pub library_id: Option<i64>,
    pub media_type: String,
    pub title: String,
    pub sort_title: Option<String>,
    pub description: Option<String>,
    pub release_date: Option<String>,
    pub duration_seconds: Option<i32>,
    pub file_path: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub container_format: Option<String>,
    pub rating: Option<f64>,
    pub content_rating: Option<String>,
    pub tagline: Option<String>,
    pub tmdb_id: Option<String>,
    pub parent_id: Option<i64>,
    pub season_number: Option<i32>,
    pub episode_number: Option<i32>,
    pub date_added: String,
    pub date_modified: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poster_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backdrop_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub genres: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub children: Option<Vec<MediaItemResponse>>,
}

impl From<MediaItem> for MediaItemResponse {
    fn from(m: MediaItem) -> Self {
        Self {
            id: m.id,
            library_id: m.library_id,
            media_type: m.media_type,
            title: m.title,
            sort_title: m.sort_title,
            description: m.description,
            release_date: m.release_date,
            duration_seconds: m.duration_seconds,
            file_path: m.file_path,
            file_size_bytes: m.file_size_bytes,
            container_format: m.container_format,
            rating: m.rating,
            content_rating: m.content_rating,
            tagline: m.tagline,
            tmdb_id: m.tmdb_id,
            parent_id: m.parent_id,
            season_number: m.season_number,
            episode_number: m.episode_number,
            date_added: m.date_added,
            date_modified: m.date_modified,
            poster_url: None,
            backdrop_url: None,
            genres: None,
            children: None,
        }
    }
}

#[derive(Debug, Clone, FromRow)]
pub struct ImageRecord {
    pub id: i64,
    pub media_item_id: Option<i64>,
    pub image_type: String,
    pub file_path: Option<String>,
}
