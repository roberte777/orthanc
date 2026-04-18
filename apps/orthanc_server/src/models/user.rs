use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub email: String,
    pub password_hash: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
    pub last_login_at: Option<String>,
}

#[derive(Debug, Serialize)]
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

impl From<User> for UserResponse {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            username: u.username,
            email: u.email,
            display_name: u.display_name,
            is_admin: u.is_admin,
            is_active: u.is_active,
            created_at: u.created_at,
            last_login_at: u.last_login_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CreateUserRequest {
    pub username: String,
    pub email: String,
    pub password: String,
    pub display_name: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateUserRequest {
    pub email: Option<String>,
    pub display_name: Option<String>,
    pub is_admin: Option<bool>,
    pub is_active: Option<bool>,
}
