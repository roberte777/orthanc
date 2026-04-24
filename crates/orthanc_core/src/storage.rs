use std::sync::Arc;

/// Persistent key-value storage for auth tokens and small config values.
///
/// Each platform supplies its own implementation:
/// - web: localStorage via web_sys
/// - mobile: keyring (iOS Keychain / Android Keystore)
/// - desktop: TBD (likely keyring too)
///
/// Implementations are shared across components via Dioxus context as
/// `Arc<dyn TokenStorage>` (alias `Storage`).
pub trait TokenStorage: Send + Sync {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str);
    fn remove(&self, key: &str);
}

pub type Storage = Arc<dyn TokenStorage>;

pub const KEY_ACCESS_TOKEN: &str = "access_token";
pub const KEY_REFRESH_TOKEN: &str = "refresh_token";
pub const KEY_SERVER_URL: &str = "server_url";
