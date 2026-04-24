// Re-exports the shared auth state and storage trait from orthanc_core so
// existing view modules can continue to use `crate::state::...` paths.
pub use orthanc_core::auth::{AuthState, clear_auth, save_auth, with_refresh};
pub use orthanc_core::storage::Storage;
