//! Shared state managed by Tauri and injected into commands.

use std::sync::atomic::AtomicBool;

use teitunnel_core::{
    accounts::Accounts, binary::BinaryManager, engine::Engine, machine::MachineTunnels,
    quick_share::QuickShares, runtime::Supervisor, store::Store,
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
    /// The routes engine (observe → plan → apply → verify).
    pub engine: Engine,
    /// This Mac's tunnel connectors.
    pub machine: MachineTunnels,
    /// This Mac's name for new tunnels.
    pub machine_name: String,
    /// Where route checks connect (Cloudflare's edge; a fake in E2E builds).
    pub edge: teitunnel_core::engine::Edge,
    /// Cancels the OAuth sign-in in progress, if any.
    pub oauth_cancel: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Tunnels whose connector the user stopped (not reported as down).
    pub paused: std::sync::Mutex<std::collections::HashSet<String>>,
    /// Set once shutdown has started, so the exit hook runs only once.
    pub shutting_down: AtomicBool,
}
