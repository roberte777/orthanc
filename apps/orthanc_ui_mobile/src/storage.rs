use orthanc_core::storage::TokenStorage;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

const SERVICE: &str = "orthanc";

/// Persistent storage for auth tokens and small config values.
///
/// Strategy:
/// - On Apple platforms (iOS, macOS): use the system Keychain via the keyring
///   crate's `apple-native` backend. Encrypted, survives reinstall on iOS,
///   standard practice.
/// - Everywhere else (Android, Linux desktop dev): fall back to a JSON file in
///   the app's per-user data dir. Android Keychain integration via
///   `android-keyring` is a v2 polish item — for v1 we accept plaintext at-rest
///   in the app sandbox, which is similar to what most file-based settings
///   stores do (e.g. Plex's mobile clients).
///
/// The file backend is also what runs during `dx serve --platform desktop`
/// dev-loop sessions on Linux, so dev parity stays close to mobile.
pub struct KeyringStorage {
    fallback_path: PathBuf,
    fallback_cache: Mutex<Option<serde_json::Map<String, serde_json::Value>>>,
}

impl KeyringStorage {
    pub fn new() -> Self {
        let dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("orthanc");
        let _ = fs::create_dir_all(&dir);
        Self {
            fallback_path: dir.join("auth.json"),
            fallback_cache: Mutex::new(None),
        }
    }

    fn load_fallback(&self) -> serde_json::Map<String, serde_json::Value> {
        let mut guard = self.fallback_cache.lock().unwrap();
        if let Some(c) = guard.as_ref() {
            return c.clone();
        }
        let map = fs::read_to_string(&self.fallback_path)
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Map<_, _>>(&s).ok())
            .unwrap_or_default();
        *guard = Some(map.clone());
        map
    }

    fn save_fallback(&self, map: &serde_json::Map<String, serde_json::Value>) {
        if let Ok(s) = serde_json::to_string(map) {
            let _ = fs::write(&self.fallback_path, s);
        }
        *self.fallback_cache.lock().unwrap() = Some(map.clone());
    }
}

impl Default for KeyringStorage {
    fn default() -> Self {
        Self::new()
    }
}

impl TokenStorage for KeyringStorage {
    fn get(&self, key: &str) -> Option<String> {
        if let Ok(entry) = keyring::Entry::new(SERVICE, key)
            && let Ok(v) = entry.get_password()
        {
            return Some(v);
        }
        self.load_fallback()
            .get(key)
            .and_then(|v| v.as_str().map(String::from))
    }

    fn set(&self, key: &str, value: &str) {
        if let Ok(entry) = keyring::Entry::new(SERVICE, key)
            && entry.set_password(value).is_ok()
        {
            return;
        }
        let mut map = self.load_fallback();
        map.insert(
            key.to_string(),
            serde_json::Value::String(value.to_string()),
        );
        self.save_fallback(&map);
    }

    fn remove(&self, key: &str) {
        if let Ok(entry) = keyring::Entry::new(SERVICE, key) {
            let _ = entry.delete_credential();
        }
        let mut map = self.load_fallback();
        if map.remove(key).is_some() {
            self.save_fallback(&map);
        }
    }
}
