use crate::api::{self, UserResponse};
use crate::storage::{KEY_ACCESS_TOKEN, KEY_REFRESH_TOKEN, Storage};
use dioxus::prelude::*;

#[derive(Debug, Clone, Default, PartialEq)]
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

pub fn save_auth(storage: &Storage, access_token: &str, refresh_token: &str) {
    storage.set(KEY_ACCESS_TOKEN, access_token);
    storage.set(KEY_REFRESH_TOKEN, refresh_token);
}

pub fn clear_auth(storage: &Storage) {
    storage.remove(KEY_ACCESS_TOKEN);
    storage.remove(KEY_REFRESH_TOKEN);
}

/// Wraps an authenticated API call with automatic token refresh on 401.
///
/// Reads the `Storage` impl from Dioxus context via `consume_context`. The
/// caller's component (or its ancestor) must have provided one with
/// `use_context_provider`. Panics if no Storage was provided — that's a
/// programming error that should be caught in dev, not a runtime failure mode.
///
/// If the inner call fails with a 401, attempts to refresh the access token
/// using the stored refresh token, updates auth state and persistent storage,
/// then retries the original call once.
pub async fn with_refresh<T, F, Fut>(mut auth: Signal<AuthState>, f: F) -> Result<T, String>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let storage: Storage = consume_context();

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
            clear_auth(&storage);
            let mut w = auth.write();
            w.access_token = None;
            w.refresh_token = None;
            w.user = None;
            return Err("Session expired. Please log in again.".to_string());
        }
    };

    match api::refresh_tokens(&refresh_token).await {
        Ok(tokens) => {
            save_auth(&storage, &tokens.access_token, &tokens.refresh_token);
            let new_token = tokens.access_token.clone();
            let mut w = auth.write();
            w.access_token = Some(tokens.access_token);
            w.refresh_token = Some(tokens.refresh_token);
            drop(w);
            f(new_token).await
        }
        Err(_) => {
            clear_auth(&storage);
            let mut w = auth.write();
            w.access_token = None;
            w.refresh_token = None;
            w.user = None;
            Err("Session expired. Please log in again.".to_string())
        }
    }
}
