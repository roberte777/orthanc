use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionType {
    Playlist,
    Favorites,
    Watchlist,
}

#[derive(Debug, Clone)]
pub struct Collection {
    pub id: i64,
    pub user_id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    pub collection_type: CollectionType,
    pub is_public: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewCollection {
    pub user_id: Option<i64>,
    pub name: String,
    pub description: Option<String>,
    pub collection_type: CollectionType,
    pub is_public: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateCollection {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub is_public: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct CollectionItem {
    pub collection_id: i64,
    pub media_item_id: i64,
    pub item_order: i64,
    pub added_at: DateTime<Utc>,
}

#[async_trait]
pub trait CollectionRepository: Send + Sync {
    async fn create(&self, input: NewCollection) -> Result<Collection>;
    async fn find_by_id(&self, id: i64) -> Result<Option<Collection>>;
    async fn list_for_user(&self, user_id: i64) -> Result<Vec<Collection>>;
    async fn list_public(&self) -> Result<Vec<Collection>>;
    async fn update(&self, id: i64, input: UpdateCollection) -> Result<Collection>;
    async fn delete(&self, id: i64) -> Result<bool>;

    async fn add_item(
        &self,
        collection_id: i64,
        media_item_id: i64,
        item_order: i64,
    ) -> Result<CollectionItem>;
    async fn remove_item(&self, collection_id: i64, media_item_id: i64) -> Result<bool>;
    async fn list_items(&self, collection_id: i64) -> Result<Vec<CollectionItem>>;
    async fn reorder_items(&self, collection_id: i64, ordered_media_ids: Vec<i64>) -> Result<()>;
    async fn clear_items(&self, collection_id: i64) -> Result<u64>;
}
