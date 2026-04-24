use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaType {
    Movie,
    TvShow,
    Season,
    Episode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExternalIdKind {
    Imdb,
    Tmdb,
    Tvdb,
    Anidb,
}

#[derive(Debug, Clone)]
pub struct MediaItem {
    pub id: i64,
    pub library_id: Option<i64>,
    pub media_type: MediaType,
    pub title: String,
    pub sort_title: Option<String>,
    pub original_title: Option<String>,
    pub description: Option<String>,
    pub release_date: Option<NaiveDate>,
    pub duration_seconds: Option<i64>,
    pub file_path: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub container_format: Option<String>,
    pub rating: Option<f64>,
    pub content_rating: Option<String>,
    pub tagline: Option<String>,
    pub imdb_id: Option<String>,
    pub anidb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub tvdb_id: Option<String>,
    pub parent_id: Option<i64>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub file_hash: Option<String>,
    pub date_added: DateTime<Utc>,
    pub date_modified: Option<DateTime<Utc>>,
    pub last_scanned_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewMediaItem {
    pub library_id: Option<i64>,
    pub media_type: MediaType,
    pub title: String,
    pub sort_title: Option<String>,
    pub original_title: Option<String>,
    pub description: Option<String>,
    pub release_date: Option<NaiveDate>,
    pub duration_seconds: Option<i64>,
    pub file_path: Option<String>,
    pub file_size_bytes: Option<i64>,
    pub mime_type: Option<String>,
    pub container_format: Option<String>,
    pub rating: Option<f64>,
    pub content_rating: Option<String>,
    pub tagline: Option<String>,
    pub imdb_id: Option<String>,
    pub anidb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub tvdb_id: Option<String>,
    pub parent_id: Option<i64>,
    pub season_number: Option<i64>,
    pub episode_number: Option<i64>,
    pub file_hash: Option<String>,
    pub date_modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateMediaItem {
    pub title: Option<String>,
    pub sort_title: Option<Option<String>>,
    pub original_title: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub release_date: Option<Option<NaiveDate>>,
    pub duration_seconds: Option<Option<i64>>,
    pub file_path: Option<Option<String>>,
    pub file_size_bytes: Option<Option<i64>>,
    pub mime_type: Option<Option<String>>,
    pub container_format: Option<Option<String>>,
    pub rating: Option<Option<f64>>,
    pub content_rating: Option<Option<String>>,
    pub tagline: Option<Option<String>>,
    pub imdb_id: Option<Option<String>>,
    pub anidb_id: Option<Option<String>>,
    pub tmdb_id: Option<Option<String>>,
    pub tvdb_id: Option<Option<String>>,
    pub season_number: Option<Option<i64>>,
    pub episode_number: Option<Option<i64>>,
    pub file_hash: Option<Option<String>>,
    pub date_modified: Option<Option<DateTime<Utc>>>,
}

#[derive(Debug, Clone, Default)]
pub struct MediaItemListFilter {
    pub library_id: Option<i64>,
    pub media_type: Option<MediaType>,
    pub parent_id: Option<i64>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[async_trait]
pub trait MediaItemRepository: Send + Sync {
    async fn create(&self, input: NewMediaItem) -> Result<MediaItem>;
    async fn find_by_id(&self, id: i64) -> Result<Option<MediaItem>>;
    async fn find_by_file_path(&self, file_path: &str) -> Result<Option<MediaItem>>;
    async fn find_by_external_id(
        &self,
        kind: ExternalIdKind,
        value: &str,
    ) -> Result<Option<MediaItem>>;
    async fn list(&self, filter: MediaItemListFilter) -> Result<Vec<MediaItem>>;
    async fn list_children(&self, parent_id: i64) -> Result<Vec<MediaItem>>;
    async fn search(
        &self,
        query: &str,
        library_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<MediaItem>>;
    async fn update(&self, id: i64, input: UpdateMediaItem) -> Result<MediaItem>;
    async fn record_scan(&self, id: i64) -> Result<()>;
    async fn delete(&self, id: i64) -> Result<bool>;
    async fn count_by_library(&self, library_id: i64) -> Result<i64>;
}
