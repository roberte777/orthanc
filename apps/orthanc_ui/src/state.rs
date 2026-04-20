use crate::api::UserResponse;
use dioxus::prelude::*;

#[derive(Debug, Clone, PartialEq)]
pub struct AuthState {
    pub user: Option<UserResponse>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
}

impl AuthState {
    pub fn logged_in(&self) -> bool {
        self.access_token.is_some() && self.user.is_some()
    }

    pub fn is_admin(&self) -> bool {
        self.user.as_ref().map(|u| u.is_admin).unwrap_or(false)
    }
}

impl Default for AuthState {
    fn default() -> Self {
        // Try to load from localStorage on WASM
        #[cfg(target_arch = "wasm32")]
        {
            let access_token = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .and_then(|s| s.get_item("access_token").ok().flatten());
            let refresh_token = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .and_then(|s| s.get_item("refresh_token").ok().flatten());
            return Self {
                user: None,
                access_token,
                refresh_token,
            };
        }
        #[cfg(not(target_arch = "wasm32"))]
        Self {
            user: None,
            access_token: None,
            refresh_token: None,
        }
    }
}

pub fn save_auth(access_token: &str, refresh_token: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(storage) = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
        {
            let _ = storage.set_item("access_token", access_token);
            let _ = storage.set_item("refresh_token", refresh_token);
        }
    }
    // Suppress unused variable warnings on non-WASM
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = access_token;
        let _ = refresh_token;
    }
}

pub fn clear_auth() {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(storage) = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
        {
            let _ = storage.remove_item("access_token");
            let _ = storage.remove_item("refresh_token");
        }
    }
}

/// Read a string from browser localStorage (WASM-only; returns None elsewhere).
pub fn storage_get(key: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        return web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
            .and_then(|s| s.get_item(key).ok().flatten());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = key;
        None
    }
}

/// Write a string to browser localStorage. No-op off WASM.
pub fn storage_set(key: &str, value: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(storage) = web_sys::window()
            .and_then(|w| w.local_storage().ok().flatten())
        {
            let _ = storage.set_item(key, value);
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (key, value);
    }
}

/// Wraps an authenticated API call with automatic token refresh on 401.
///
/// If the call fails with a 401, attempts to refresh the access token using
/// the stored refresh token, updates the auth state and localStorage, then
/// retries the original call once.
pub async fn with_refresh<T, F, Fut>(
    mut auth: Signal<AuthState>,
    f: F,
) -> Result<T, String>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let token = auth.read().access_token.clone().unwrap_or_default();
    let result = f(token).await;

    let is_401 = matches!(&result, Err(e) if e.starts_with("Error 401"));
    if !is_401 {
        return result;
    }

    let refresh_token = auth.read().refresh_token.clone();
    let refresh_token = match refresh_token {
        Some(rt) => rt,
        None => {
            clear_auth();
            let mut w = auth.write();
            w.access_token = None;
            w.refresh_token = None;
            w.user = None;
            return Err("Session expired. Please log in again.".to_string());
        }
    };

    match crate::api::refresh_tokens(&refresh_token).await {
        Ok(tokens) => {
            save_auth(&tokens.access_token, &tokens.refresh_token);
            let new_token = tokens.access_token.clone();
            let mut w = auth.write();
            w.access_token = Some(tokens.access_token);
            w.refresh_token = Some(tokens.refresh_token);
            drop(w);
            f(new_token).await
        }
        Err(_) => {
            clear_auth();
            let mut w = auth.write();
            w.access_token = None;
            w.refresh_token = None;
            w.user = None;
            Err("Session expired. Please log in again.".to_string())
        }
    }
}
