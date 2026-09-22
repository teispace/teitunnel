//! Shared state managed by Tauri and injected into commands.

use std::sync::atomic::AtomicBool;

use teitunnel_core::{
    accounts::Accounts, binary::BinaryManager, quick_share::QuickShares, runtime::Supervisor,
    store::Store,
};

/// Long-lived handles owned by the app.
#[derive(Debug)]
pub struct AppState {
    /// The local database.
    pub store: Store,
    /// The cloudflared binary in use.
    pub binary: BinaryManager,
    /// All Session-mode connectors.
    pub supervisor: Supervisor,
    /// Quick Shares.
    pub quick_shares: QuickShares,
    /// Connected Cloudflare accounts.
    pub accounts: Accounts,
    /// Set once shutdown has started, so the exit hook runs only once.
    pub shutting_down: AtomicBool,
}
