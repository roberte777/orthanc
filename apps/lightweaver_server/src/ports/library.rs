use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LibraryType {
    Movies,
    TvShows,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetadataProvider {
    Tmdb,
    Anidb,
    Tvdb,
}

#[derive(Debug, Clone)]
pub struct Library {
    pub id: i64,
    pub name: String,
    pub library_type: LibraryType,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub last_scan_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewLibrary {
    pub name: String,
    pub library_type: LibraryType,
    pub description: Option<String>,
    pub is_enabled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateLibrary {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub is_enabled: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct LibraryPath {
    pub id: i64,
    pub library_id: i64,
    pub path: String,
    pub is_enabled: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewLibraryPath {
    pub library_id: i64,
    pub path: String,
    pub is_enabled: bool,
}

#[derive(Debug, Clone)]
pub struct LibraryMetadataProvider {
    pub id: i64,
    pub library_id: i64,
    pub provider: MetadataProvider,
    pub is_enabled: bool,
    pub priority: i64,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewLibraryMetadataProvider {
    pub library_id: i64,
    pub provider: MetadataProvider,
    pub is_enabled: bool,
    pub priority: i64,
}

#[async_trait]
pub trait LibraryRepository: Send + Sync {
    async fn create(&self, input: NewLibrary) -> Result<Library>;
    async fn find_by_id(&self, id: i64) -> Result<Option<Library>>;
    async fn list(&self) -> Result<Vec<Library>>;
    async fn list_for_user(&self, user_id: i64) -> Result<Vec<Library>>;
    async fn update(&self, id: i64, input: UpdateLibrary) -> Result<Library>;
    async fn delete(&self, id: i64) -> Result<bool>;
    async fn record_scan(&self, id: i64) -> Result<()>;

    async fn grant_user_access(&self, library_id: i64, user_id: i64) -> Result<()>;
    async fn revoke_user_access(&self, library_id: i64, user_id: i64) -> Result<bool>;
    async fn list_users(&self, library_id: i64) -> Result<Vec<i64>>;

    async fn add_path(&self, input: NewLibraryPath) -> Result<LibraryPath>;
    async fn list_paths(&self, library_id: i64) -> Result<Vec<LibraryPath>>;
    async fn set_path_enabled(&self, path_id: i64, enabled: bool) -> Result<()>;
    async fn delete_path(&self, path_id: i64) -> Result<bool>;

    async fn add_provider(
        &self,
        input: NewLibraryMetadataProvider,
    ) -> Result<LibraryMetadataProvider>;
    async fn list_providers(&self, library_id: i64) -> Result<Vec<LibraryMetadataProvider>>;
    async fn set_provider_enabled(&self, provider_id: i64, enabled: bool) -> Result<()>;
    async fn update_provider_priority(&self, provider_id: i64, priority: i64) -> Result<()>;
    async fn delete_provider(&self, provider_id: i64) -> Result<bool>;
}
