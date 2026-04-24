use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct Genre {
    pub id: i64,
    pub name: String,
    pub created_at: DateTime<Utc>,
}

#[async_trait]
pub trait GenreRepository: Send + Sync {
    async fn find_by_id(&self, id: i64) -> Result<Option<Genre>>;
    async fn find_by_name(&self, name: &str) -> Result<Option<Genre>>;
    async fn find_or_create(&self, name: &str) -> Result<Genre>;
    async fn list(&self) -> Result<Vec<Genre>>;
    async fn delete(&self, id: i64) -> Result<bool>;

    async fn attach_to_media(&self, media_item_id: i64, genre_id: i64) -> Result<()>;
    async fn detach_from_media(&self, media_item_id: i64, genre_id: i64) -> Result<bool>;
    async fn set_media_genres(&self, media_item_id: i64, genre_ids: Vec<i64>) -> Result<()>;
    async fn list_for_media(&self, media_item_id: i64) -> Result<Vec<Genre>>;
}
