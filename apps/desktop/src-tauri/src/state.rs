//! Shared state managed by Tauri and injected into commands.

use teitunnel_core::store::Store;

/// Long-lived handles owned by the app.
#[derive(Debug)]
pub struct AppState {
    /// The local database.
    pub store: Store,
}
