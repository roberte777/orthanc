use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RoleType {
    Actor,
    Director,
    Writer,
    Producer,
    Composer,
    Cinematographer,
    Editor,
}

#[derive(Debug, Clone)]
pub struct MediaCredit {
    pub id: i64,
    pub media_item_id: i64,
    pub person_id: i64,
    pub role_type: RoleType,
    pub character_name: Option<String>,
    pub credit_order: Option<i64>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewMediaCredit {
    pub media_item_id: i64,
    pub person_id: i64,
    pub role_type: RoleType,
    pub character_name: Option<String>,
    pub credit_order: Option<i64>,
}

#[async_trait]
pub trait MediaCreditRepository: Send + Sync {
    async fn create(&self, input: NewMediaCredit) -> Result<MediaCredit>;
    async fn find_by_id(&self, id: i64) -> Result<Option<MediaCredit>>;
    async fn list_for_media(&self, media_item_id: i64) -> Result<Vec<MediaCredit>>;
    async fn list_for_person(&self, person_id: i64) -> Result<Vec<MediaCredit>>;
    async fn delete(&self, id: i64) -> Result<bool>;
    async fn delete_for_media(&self, media_item_id: i64) -> Result<u64>;
}
