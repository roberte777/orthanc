use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ImageType {
    Poster,
    Backdrop,
    Thumbnail,
    Profile,
    Screenshot,
    Logo,
}

#[derive(Debug, Clone)]
pub struct Image {
    pub id: i64,
    pub media_item_id: Option<i64>,
    pub person_id: Option<i64>,
    pub image_type: ImageType,
    pub url: Option<String>,
    pub file_path: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub aspect_ratio: Option<f64>,
    pub is_primary: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewImage {
    pub media_item_id: Option<i64>,
    pub person_id: Option<i64>,
    pub image_type: ImageType,
    pub url: Option<String>,
    pub file_path: Option<String>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub aspect_ratio: Option<f64>,
    pub is_primary: bool,
}

#[async_trait]
pub trait ImageRepository: Send + Sync {
    async fn create(&self, input: NewImage) -> Result<Image>;
    async fn find_by_id(&self, id: i64) -> Result<Option<Image>>;
    async fn list_for_media(
        &self,
        media_item_id: i64,
        image_type: Option<ImageType>,
    ) -> Result<Vec<Image>>;
    async fn list_for_person(
        &self,
        person_id: i64,
        image_type: Option<ImageType>,
    ) -> Result<Vec<Image>>;
    async fn set_primary(&self, id: i64) -> Result<()>;
    async fn delete(&self, id: i64) -> Result<bool>;
    async fn delete_for_media(&self, media_item_id: i64) -> Result<u64>;
    async fn delete_for_person(&self, person_id: i64) -> Result<u64>;
}
