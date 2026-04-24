use orthanc_core::storage::TokenStorage;

/// Browser localStorage-backed implementation of TokenStorage.
///
/// On non-WASM targets every operation is a no-op. The web crate is only ever
/// compiled for WASM in practice, but the cfg guards keep `cargo check` happy
/// when verifying the workspace from a non-WASM toolchain.
pub struct WebStorage;

impl TokenStorage for WebStorage {
    fn get(&self, key: &str) -> Option<String> {
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

    fn set(&self, key: &str, value: &str) {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                let _ = storage.set_item(key, value);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = (key, value);
        }
    }

    fn remove(&self, key: &str) {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten())
            {
                let _ = storage.remove_item(key);
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = key;
        }
    }
}
