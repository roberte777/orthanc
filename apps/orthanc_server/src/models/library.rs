use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct Library {
    pub id: i64,
    pub name: String,
    pub library_type: String,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub scan_interval_minutes: Option<i32>,
    pub last_scan_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, FromRow)]
pub struct LibraryPath {
    pub id: i64,
    pub library_id: i64,
    pub path: String,
    pub is_enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct LibraryResponse {
    pub id: i64,
    pub name: String,
    pub library_type: String,
    pub description: Option<String>,
    pub is_enabled: bool,
    pub scan_interval_minutes: Option<i32>,
    pub last_scan_at: Option<String>,
    pub paths: Vec<LibraryPathResponse>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct LibraryPathResponse {
    pub id: i64,
    pub path: String,
    pub is_enabled: bool,
}

impl From<&LibraryPath> for LibraryPathResponse {
    fn from(p: &LibraryPath) -> Self {
        Self {
            id: p.id,
            path: p.path.clone(),
            is_enabled: p.is_enabled,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateLibraryRequest {
    pub name: String,
    pub library_type: String,
    pub description: Option<String>,
    pub paths: Vec<String>,
    pub scan_interval_minutes: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateLibraryRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
    pub scan_interval_minutes: Option<Option<i32>>,
}

#[derive(Debug, Deserialize)]
pub struct AddLibraryPathRequest {
    pub path: String,
}
