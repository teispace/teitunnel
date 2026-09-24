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
    /// The routes engine (observe → plan → apply → verify), shared with the control
    /// connection so applies stay serialized.
    pub engine: std::sync::Arc<Engine>,
    /// This Mac's tunnel connectors.
    pub machine: MachineTunnels,
    /// Live logs of connectors on other machines.
    pub remote_logs: teitunnel_core::remote_logs::RemoteLogs,
    /// This Mac's name for new tunnels.
    pub machine_name: String,
    /// Where route checks connect (Cloudflare's edge; a fake in E2E builds).
    pub edge: teitunnel_core::engine::Edge,
    /// Cancels the OAuth sign-in in progress, if any.
    pub oauth_cancel: std::sync::Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    /// Doctor runs, for background scheduling and new-problem notifications.
    pub doctor: teitunnel_core::doctor_monitor::DoctorMonitor,
    /// Tunnels whose connector the user stopped (not reported as down).
    pub paused: std::sync::Mutex<std::collections::HashSet<String>>,
    /// The user confirmed quitting while routes were running.
    pub quit_confirmed: AtomicBool,
    /// Set once shutdown has started, so the exit hook runs only once.
    pub shutting_down: AtomicBool,
    /// Registries of `teitunnel` processes (their shares show in Quick Share).
    pub cli_runs: std::path::PathBuf,
    /// Edge analytics (cached GraphQL answers).
    pub analytics: teitunnel_core::analytics::Analytics,
    /// Uptime checks and alerts.
    pub monitor: teitunnel_core::uptime::Monitor,
    /// Snapshot files prepared for review.
    pub snapshots: teitunnel_core::snapshot::Preparations,
    /// Where crawled sites are captured before publishing.
    pub snapshot_dir: std::path::PathBuf,
    /// The control connection and `teitunnel://` links.
    pub control: crate::shell::control::Control,
    /// New service token secrets, kept in memory briefly so they can be copied.
    pub issued_secrets: teitunnel_core::protection::IssuedSecrets,
    /// The keychain (a project's secret references are read from it).
    pub secrets: teitunnel_core::secrets::Secrets,
    /// A backup read and shown to the user, waiting to be restored (its id, its contents).
    pub pending_restore: std::sync::Mutex<Option<(String, teitunnel_core::backup::Contents)>>,
    /// The inspector (Lens) in front of Quick Shares and inspected routes.
    pub inspector: teitunnel_core::inspect::Inspector,
    /// Local HTTPS domains, served through the inspector's Lens.
    pub local_domains: teitunnel_core::local_domains::LocalDomains,
    /// Live inspector subscriptions of the webview, by id (cancelled to stop).
    pub inspect_live:
        std::sync::Mutex<std::collections::HashMap<u32, tokio_util::sync::CancellationToken>>,
}
