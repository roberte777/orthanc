// API client for communicating with the orthanc_server REST API

use serde::{de::DeserializeOwned, Deserialize, Serialize};

pub const API_BASE_URL: &str = "http://localhost:8081";
const API_BASE: &str = API_BASE_URL;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UserResponse {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub is_active: bool,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub user: UserResponse,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RefreshResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupStatus {
    pub needs_setup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaginatedUsers {
    pub users: Vec<UserResponse>,
    pub total: i64,
    pub page: u32,
    pub per_page: u32,
}

#[derive(Debug, Serialize)]
pub struct SetupRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Serialize, Default)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct UpdateProfileRequest {
    pub display_name: Option<String>,
    pub email: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChangePasswordRequest {
    pub current_password: String,
    pub new_password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Setting {
    pub key: String,
    pub value: String,
    pub value_type: String,
    pub description: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateSettingRequest {
    pub key: String,
    pub value: String,
}

// ── Libraries ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LibraryPathResponse {
    pub id: i64,
    pub path: String,
    pub is_enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct CreateLibraryRequest {
    pub name: String,
    pub library_type: String,
    pub description: Option<String>,
    pub paths: Vec<String>,
    pub scan_interval_minutes: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct UpdateLibraryRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub is_enabled: Option<bool>,
    pub scan_interval_minutes: Option<Option<i32>>,
}

#[derive(Debug, Serialize)]
pub struct AddLibraryPathRequest {
    pub path: String,
}

pub async fn list_libraries(token: &str) -> Result<Vec<LibraryResponse>, String> {
    get_json("/api/admin/libraries", Some(token)).await
}

#[allow(dead_code)]
pub async fn get_library(token: &str, id: i64) -> Result<LibraryResponse, String> {
    get_json(&format!("/api/admin/libraries/{}", id), Some(token)).await
}

pub async fn create_library(token: &str, req: CreateLibraryRequest) -> Result<LibraryResponse, String> {
    post_json("/api/admin/libraries", &req, Some(token)).await
}

pub async fn update_library(token: &str, id: i64, req: UpdateLibraryRequest) -> Result<LibraryResponse, String> {
    put_json(&format!("/api/admin/libraries/{}", id), &req, Some(token)).await
}

pub async fn delete_library(token: &str, id: i64) -> Result<(), String> {
    delete_req(&format!("/api/admin/libraries/{}", id), Some(token)).await
}

pub async fn add_library_path(token: &str, id: i64, path: &str) -> Result<LibraryPathResponse, String> {
    let req = AddLibraryPathRequest { path: path.to_string() };
    post_json(&format!("/api/admin/libraries/{}/paths", id), &req, Some(token)).await
}

pub async fn remove_library_path(token: &str, library_id: i64, path_id: i64) -> Result<(), String> {
    delete_req(&format!("/api/admin/libraries/{}/paths/{}", library_id, path_id), Some(token)).await
}

pub async fn scan_library(token: &str, id: i64) -> Result<ScanResult, String> {
    post_json(&format!("/api/admin/libraries/{}/scan", id), &serde_json::json!({}), Some(token)).await
}

pub async fn list_library_media(token: &str, id: i64) -> Result<Vec<MediaItemResponse>, String> {
    get_json(&format!("/api/admin/libraries/{}/media", id), Some(token)).await
}

// ── Media ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
    pub poster_url: Option<String>,
    pub backdrop_url: Option<String>,
    pub genres: Option<Vec<String>>,
    pub children: Option<Vec<MediaItemResponse>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub added: u32,
    pub unchanged: u32,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecentMedia {
    pub movies: Vec<MediaItemResponse>,
    pub shows: Vec<MediaItemResponse>,
}

pub async fn get_recent_media(token: &str) -> Result<RecentMedia, String> {
    get_json("/api/media/recent", Some(token)).await
}

pub async fn get_all_movies(token: &str) -> Result<Vec<MediaItemResponse>, String> {
    get_json("/api/media/movies", Some(token)).await
}

pub async fn get_movie(token: &str, id: i64) -> Result<MediaItemResponse, String> {
    get_json(&format!("/api/media/movies/{}", id), Some(token)).await
}

pub async fn get_all_shows(token: &str) -> Result<Vec<MediaItemResponse>, String> {
    get_json("/api/media/shows", Some(token)).await
}

pub async fn get_show(token: &str, id: i64) -> Result<MediaItemResponse, String> {
    get_json(&format!("/api/media/shows/{}", id), Some(token)).await
}

pub async fn refresh_metadata(token: &str, id: i64, mode: &str) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({"mode": mode});
    post_json(&format!("/api/admin/metadata/refresh/{}", id), &body, Some(token)).await
}

pub async fn refresh_library_metadata(token: &str, id: i64, mode: &str) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({"mode": mode});
    post_json(&format!("/api/admin/metadata/refresh-library/{}", id), &body, Some(token)).await
}

#[derive(Debug, Serialize)]
pub struct MetadataOverride {
    pub title: Option<String>,
    pub description: Option<String>,
    pub rating: Option<f64>,
    pub content_rating: Option<String>,
    pub tagline: Option<String>,
    pub release_date: Option<String>,
}

pub async fn override_metadata(token: &str, id: i64, req: MetadataOverride) -> Result<serde_json::Value, String> {
    put_json(&format!("/api/admin/metadata/override/{}", id), &req, Some(token)).await
}

// ── Metadata Providers ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetadataProviderResponse {
    pub id: i64,
    pub library_id: i64,
    pub provider: String,
    pub is_enabled: bool,
    pub priority: i32,
}

pub async fn list_providers(token: &str, library_id: i64) -> Result<Vec<MetadataProviderResponse>, String> {
    get_json(&format!("/api/admin/libraries/{}/providers", library_id), Some(token)).await
}

pub async fn update_provider(token: &str, library_id: i64, provider: &str, is_enabled: bool) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "provider": provider,
        "is_enabled": is_enabled,
    });
    put_json(&format!("/api/admin/libraries/{}/providers", library_id), &body, Some(token)).await
}

pub async fn swap_providers(token: &str, library_id: i64, provider_a: &str, provider_b: &str) -> Result<serde_json::Value, String> {
    let body = serde_json::json!({
        "provider_a": provider_a,
        "provider_b": provider_b,
    });
    post_json(&format!("/api/admin/libraries/{}/providers/swap", library_id), &body, Some(token)).await
}

pub async fn get_setup_status() -> Result<SetupStatus, String> {
    get_json("/api/setup-status", None).await
}

pub async fn setup(req: SetupRequest) -> Result<AuthResponse, String> {
    post_json("/api/auth/setup", &req, None).await
}

pub async fn login(req: LoginRequest) -> Result<AuthResponse, String> {
    post_json("/api/auth/login", &req, None).await
}

pub async fn get_me(token: &str) -> Result<UserResponse, String> {
    get_json("/api/auth/me", Some(token)).await
}

pub async fn refresh_tokens(refresh_token: &str) -> Result<RefreshResponse, String> {
    let body = serde_json::json!({"refresh_token": refresh_token});
    post_json("/api/auth/refresh", &body, None).await
}

pub async fn logout(token: &str, refresh_token: &str) -> Result<(), String> {
    let body = serde_json::json!({"refresh_token": refresh_token});
    let _ = post_json::<serde_json::Value>("/api/auth/logout", &body, Some(token)).await?;
    Ok(())
}

pub async fn list_users(token: &str, page: u32) -> Result<PaginatedUsers, String> {
    get_json(
        &format!("/api/admin/users?page={}&per_page=20", page),
        Some(token),
    )
    .await
}

pub async fn create_user(token: &str, req: CreateUserRequest) -> Result<UserResponse, String> {
    post_json("/api/admin/users", &req, Some(token)).await
}

pub async fn update_user(
    token: &str,
    id: i64,
    req: UpdateUserRequest,
) -> Result<UserResponse, String> {
    put_json(&format!("/api/admin/users/{}", id), &req, Some(token)).await
}

pub async fn delete_user(token: &str, id: i64) -> Result<(), String> {
    delete_req(&format!("/api/admin/users/{}", id), Some(token)).await
}

pub async fn update_profile(
    token: &str,
    req: UpdateProfileRequest,
) -> Result<UserResponse, String> {
    put_json("/api/settings/profile", &req, Some(token)).await
}

pub async fn get_server_settings(token: &str) -> Result<Vec<Setting>, String> {
    get_json("/api/admin/settings", Some(token)).await
}

pub async fn update_server_setting(token: &str, key: &str, value: &str) -> Result<(), String> {
    let req = UpdateSettingRequest { key: key.to_string(), value: value.to_string() };
    let _ = put_json::<serde_json::Value>("/api/admin/settings", &req, Some(token)).await?;
    Ok(())
}

pub async fn change_password(token: &str, req: ChangePasswordRequest) -> Result<(), String> {
    let _ =
        put_json::<serde_json::Value>("/api/settings/password", &req, Some(token)).await?;
    Ok(())
}

// HTTP helpers using reqwest
async fn get_json<T: DeserializeOwned>(path: &str, token: Option<&str>) -> Result<T, String> {
    let url = format!("{}{}", API_BASE, path);
    let client = reqwest::Client::new();
    let mut req = client.get(&url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        resp.json::<T>().await.map_err(|e| e.to_string())
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(format!("Error {}: {}", status, text))
    }
}

async fn post_json<T: DeserializeOwned>(
    path: &str,
    body: &impl Serialize,
    token: Option<&str>,
) -> Result<T, String> {
    let url = format!("{}{}", API_BASE, path);
    let client = reqwest::Client::new();
    let mut req = client.post(&url).json(body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        resp.json::<T>().await.map_err(|e| e.to_string())
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(format!("Error {}: {}", status, text))
    }
}

async fn put_json<T: DeserializeOwned>(
    path: &str,
    body: &impl Serialize,
    token: Option<&str>,
) -> Result<T, String> {
    let url = format!("{}{}", API_BASE, path);
    let client = reqwest::Client::new();
    let mut req = client.put(&url).json(body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        resp.json::<T>().await.map_err(|e| e.to_string())
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(format!("Error {}: {}", status, text))
    }
}

async fn delete_req(path: &str, token: Option<&str>) -> Result<(), String> {
    let url = format!("{}{}", API_BASE, path);
    let client = reqwest::Client::new();
    let mut req = client.delete(&url);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    if resp.status().is_success() {
        Ok(())
    } else {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        Err(format!("Error {}: {}", status, text))
    }
}
