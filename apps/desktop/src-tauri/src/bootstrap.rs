//! Startup and shutdown of the core services.

use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;
use tauri_specta::Event;
use teitunnel_core::{
    accounts::Accounts,
    binary::{BinaryManager, Locator},
    engine::{Edge, Engine, Local},
    machine::{MachineTunnels, machine_name},
    quick_share::{QuickShare, QuickShares, ShareStatus},
    runtime::{PidRegistry, PortAllocator, QUICK_SHARE_PORTS, Supervisor, TUNNEL_PORTS},
    secrets::Secrets,
    settings,
    store::Store,
};

use crate::{
    ipc::{EntityChanged, EntityKind},
    shell,
    state::AppState,
};

/// Upper bound for stopping every connector when the app quits.
const SHUTDOWN_DEADLINE: Duration = Duration::from_secs(8);

/// Opens the database, reaps connectors orphaned by a previous crash, and starts the
/// services the commands use.
pub fn init<R: Runtime>(app: &AppHandle<R>) -> Result<AppState, Box<dyn std::error::Error>> {
    // `TEITUNNEL_DATA_DIR` isolates test runs (E2E) from the user's real data.
    let data_dir = match std::env::var_os("TEITUNNEL_DATA_DIR") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => app.path().app_data_dir()?,
    };
    let store = Store::open(&data_dir.join("teitunnel.db"))?;
    let prefs = tauri::async_runtime::block_on(settings::load(&store))?;
    shell::tray::install(app, prefs.show_in_menu_bar)?;

    let registry = PidRegistry::new(data_dir.join("run"));
    let reaped = tauri::async_runtime::block_on(registry.reap_orphans());
    if !reaped.is_empty() {
        tracing::info!(
            count = reaped.len(),
            "stopped connectors left over from a previous run"
        );
    }

    // E2E builds must never run the real cloudflared (it would open public tunnels).
    #[cfg(feature = "e2e")]
    if std::env::var_os("TEITUNNEL_CLOUDFLARED").is_none() {
        return Err("E2E builds require TEITUNNEL_CLOUDFLARED to point at fake-cloudflared".into());
    }

    let runtime = tauri::async_runtime::handle().inner().clone();
    let supervisor = Supervisor::new(registry, runtime);
    let binary = BinaryManager::new(Locator::from_env(data_dir.join("bin")));
    let quick_shares = QuickShares::new(
        supervisor.clone(),
        binary.clone(),
        PortAllocator::new(QUICK_SHARE_PORTS),
        store.clone(),
    );
    tauri::async_runtime::spawn(quick_shares.clone().watch_runtime());
    forward_quick_share_changes(app.clone(), &quick_shares);

    let (secrets, accounts, edge) = services(&store);
    let local = Local::new(store.clone());
    let machine = MachineTunnels::new(
        supervisor.clone(),
        binary.clone(),
        PortAllocator::new(TUNNEL_PORTS),
        secrets,
        local.clone(),
    );
    resume_machine_tunnels(app.clone(), accounts.clone(), machine.clone());

    Ok(AppState {
        accounts,
        engine: Engine::new(local),
        machine,
        machine_name: machine_name(),
        edge,
        store,
        binary,
        supervisor,
        quick_shares,
        oauth_cancel: std::sync::Mutex::default(),
        shutting_down: false.into(),
    })
}

/// The keychain, the Cloudflare API and the edge the verifier probes.
#[cfg(not(feature = "e2e"))]
fn services(store: &Store) -> (Secrets, Accounts, Edge) {
    let secrets: Secrets = Arc::new(teitunnel_core::secrets::KeychainStore);
    let accounts = Accounts::new(store.clone(), secrets.clone());
    (secrets, accounts, Edge::Cloudflare)
}

/// E2E builds never touch the login keychain or the real Cloudflare: secrets stay in
/// memory, and `TEITUNNEL_API_BASE` / `TEITUNNEL_EDGE` point at `fake-cloudflare`.
#[cfg(feature = "e2e")]
fn services(store: &Store) -> (Secrets, Accounts, Edge) {
    let secrets: Secrets = Arc::new(teitunnel_core::secrets::MemoryStore::default());
    let base = std::env::var("TEITUNNEL_API_BASE").unwrap_or_else(|_| "http://127.0.0.1:9".into());
    let accounts = Accounts::with_api_base(store.clone(), secrets.clone(), &base, None);
    let edge = std::env::var("TEITUNNEL_EDGE")
        .ok()
        .and_then(|addr| addr.parse().ok())
        .map_or(Edge::Test(([127, 0, 0, 1], 9).into()), Edge::Test);
    (secrets, accounts, edge)
}

/// Starts every account's machine tunnel connector (Session mode runs while the app
/// does). Failures are logged; the Routes view shows the connector as stopped.
fn resume_machine_tunnels<R: Runtime>(
    app: AppHandle<R>,
    accounts: Accounts,
    machine: MachineTunnels,
) {
    tauri::async_runtime::spawn(async move {
        let Ok(list) = accounts.list().await else {
            return;
        };
        for account in list {
            let Ok(client) = accounts.client(&account.id).await else {
                continue;
            };
            match machine.resume(&client, &account.id).await {
                Ok(true) => {
                    let _ = EntityChanged {
                        kind: EntityKind::Routes,
                        id: Some(account.id.clone()),
                    }
                    .emit(&app);
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!(account = %account.id, %err, "couldn't start the tunnel connector");
                }
            }
        }
    });
}

/// Turns Quick Share changes into `EntityChanged` events, refreshes the menu bar menu,
/// and posts notifications for events the user might not see (M1-11).
fn forward_quick_share_changes<R: Runtime>(app: AppHandle<R>, quick_shares: &QuickShares) {
    let mut changes = quick_shares.subscribe();
    let quick_shares = quick_shares.clone();
    tauri::async_runtime::spawn(async move {
        let mut last: HashMap<String, ShareStatus> = HashMap::new();
        loop {
            match changes.recv().await {
                Ok(id) => {
                    let shares = quick_shares.list();
                    shell::tray::refresh(&app, &shares);
                    let current = shares.iter().find(|share| share.id == id);
                    notify_transition(&app, last.get(&id), current);
                    match current {
                        Some(share) => last.insert(id.clone(), share.status.clone()),
                        None => last.remove(&id),
                    };
                    let event = EntityChanged {
                        kind: EntityKind::QuickShares,
                        id: Some(id),
                    };
                    if let Err(err) = event.emit(&app) {
                        tracing::warn!(error = %err, "failed to emit quick share change");
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

/// Notifies when a share goes live or fails, but only if no Teitunnel window has focus:
/// otherwise the user is already looking at it.
fn notify_transition<R: Runtime>(
    app: &AppHandle<R>,
    before: Option<&ShareStatus>,
    after: Option<&QuickShare>,
) {
    let Some(share) = after else { return };
    let focused = app
        .webview_windows()
        .values()
        .any(|w| w.is_focused().unwrap_or(false));
    if focused || before == Some(&share.status) {
        return;
    }
    let (title, body) = match (&share.status, before) {
        (ShareStatus::Live, Some(ShareStatus::Starting)) => {
            ("Quick Share is live", share.url.clone().unwrap_or_default())
        }
        (ShareStatus::Failed { message }, _) => ("Quick Share stopped working", message.clone()),
        _ => return,
    };
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(error = %err, "failed to show notification");
    }
}

/// Handles an exit request: the first one is deferred while every Session connector
/// stops (bounded by [`SHUTDOWN_DEADLINE`]); then the app exits for real.
pub fn on_exit_requested<R: Runtime>(app: &AppHandle<R>, api: &tauri::ExitRequestApi) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if state.shutting_down.swap(true, Ordering::SeqCst) {
        return; // second pass: let it exit
    }
    if state.supervisor.ids().is_empty() {
        return;
    }
    api.prevent_exit();
    let quick_shares = state.quick_shares.clone();
    let supervisor = state.supervisor.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let stop = async {
            quick_shares.stop_all().await;
            supervisor.stop_all().await;
        };
        if tokio::time::timeout(SHUTDOWN_DEADLINE, stop).await.is_err() {
            tracing::warn!("connectors didn't stop in time; exiting anyway");
        }
        app.exit(0);
    });
}
