use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, NaiveDate, Utc};

#[derive(Debug, Clone)]
pub struct Person {
    pub id: i64,
    pub name: String,
    pub biography: Option<String>,
    pub birth_date: Option<NaiveDate>,
    pub death_date: Option<NaiveDate>,
    pub profile_image_url: Option<String>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewPerson {
    pub name: String,
    pub biography: Option<String>,
    pub birth_date: Option<NaiveDate>,
    pub death_date: Option<NaiveDate>,
    pub profile_image_url: Option<String>,
    pub imdb_id: Option<String>,
    pub tmdb_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct UpdatePerson {
    pub name: Option<String>,
    pub biography: Option<Option<String>>,
    pub birth_date: Option<Option<NaiveDate>>,
    pub death_date: Option<Option<NaiveDate>>,
    pub profile_image_url: Option<Option<String>>,
    pub imdb_id: Option<Option<String>>,
    pub tmdb_id: Option<Option<String>>,
}

#[async_trait]
pub trait PersonRepository: Send + Sync {
    async fn create(&self, input: NewPerson) -> Result<Person>;
    async fn find_by_id(&self, id: i64) -> Result<Option<Person>>;
    async fn find_by_imdb_id(&self, imdb_id: &str) -> Result<Option<Person>>;
    async fn find_by_tmdb_id(&self, tmdb_id: &str) -> Result<Option<Person>>;
    async fn search_by_name(&self, query: &str, limit: i64) -> Result<Vec<Person>>;
    async fn update(&self, id: i64, input: UpdatePerson) -> Result<Person>;
    async fn delete(&self, id: i64) -> Result<bool>;
}
