use anyhow::Result;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String, // user id as string
    pub username: String,
    pub email: String,
    pub is_admin: bool,
    pub iat: u64,
    pub exp: u64,
}

pub fn create_access_token(
    user_id: i64,
    username: &str,
    email: &str,
    is_admin: bool,
    secret: &str,
    expiry_secs: u64,
) -> Result<String> {
    let now = chrono::Utc::now().timestamp() as u64;
    let claims = Claims {
        sub: user_id.to_string(),
        username: username.to_string(),
        email: email.to_string(),
        is_admin,
        iat: now,
        exp: now + expiry_secs,
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|e| anyhow::anyhow!("Failed to create token: {}", e))
}

pub fn validate_access_token(token: &str, secret: &str) -> Result<Claims> {
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map_err(|e| anyhow::anyhow!("Invalid token: {}", e))?;
    Ok(data.claims)
}
