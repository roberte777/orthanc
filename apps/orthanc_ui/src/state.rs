use crate::api::UserResponse;

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
