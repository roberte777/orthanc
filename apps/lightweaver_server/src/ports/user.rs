use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_login_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateUser {
    pub email: Option<String>,
    pub display_name: Option<Option<String>>,
    pub is_admin: Option<bool>,
    pub is_active: Option<bool>,
}

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn create(&self, input: NewUser) -> Result<User>;
    async fn find_by_id(&self, id: i64) -> Result<Option<User>>;
    async fn find_by_username(&self, username: &str) -> Result<Option<User>>;
    async fn find_by_email(&self, email: &str) -> Result<Option<User>>;
    async fn list(&self, limit: i64, offset: i64) -> Result<Vec<User>>;
    async fn update(&self, id: i64, input: UpdateUser) -> Result<User>;
    async fn update_password(&self, id: i64, password_hash: &str) -> Result<()>;
    async fn record_login(&self, id: i64) -> Result<()>;
    async fn delete(&self, id: i64) -> Result<bool>;
    async fn count(&self) -> Result<i64>;
}
