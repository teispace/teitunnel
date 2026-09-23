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
    let paths = teitunnel_core::machine::ServicePaths {
        tokens: data_dir.join("tokens"),
        logs: data_dir.join("logs").join("connectors"),
    };
    let machine = MachineTunnels::new(
        supervisor.clone(),
        binary.clone(),
        PortAllocator::new(TUNNEL_PORTS),
        secrets,
        local.clone(),
    );
    let machine = match service_manager() {
        Some(manager) => machine.with_services(manager, paths),
        None => machine,
    };
    resume_machine_tunnels(app.clone(), accounts.clone(), machine.clone());
    watch_tray_routes(app.clone());
    watch_connector_health(app.clone());
    tauri::async_runtime::spawn(machine.clone().sample_forever());

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
        paused: std::sync::Mutex::default(),
        quit_confirmed: false.into(),
        shutting_down: false.into(),
    })
}

/// Where Always-on connectors run: launchd on macOS. E2E builds use child processes, so
/// tests never install real launch agents.
fn service_manager() -> Option<Arc<dyn teitunnel_core::service::ServiceManager>> {
    if cfg!(feature = "e2e") {
        return Some(Arc::new(teitunnel_core::service::ProcessServices::default()));
    }
    if cfg!(target_os = "macos") {
        return teitunnel_core::service::Launchd::for_current_user()
            .map(|l| Arc::new(l) as Arc<dyn teitunnel_core::service::ServiceManager>);
    }
    None
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

/// Updates the routes in the menu bar menu (every account's routes and status).
pub fn refresh_tray_routes<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let mut routes = Vec::new();
        for account in state.accounts.list().await.unwrap_or_default() {
            let Ok(api) = state.accounts.client(&account.id).await else {
                continue;
            };
            let ctx = teitunnel_core::engine::Context {
                account: &account.id,
                machine_name: &state.machine_name,
            };
            if let Ok(overview) = state.engine.overview(&api, &state.machine, ctx).await {
                routes.extend(overview.statuses().into_iter().map(|(hostname, status)| {
                    shell::tray::TrayRoute {
                        hostname,
                        status: status.to_owned(),
                    }
                }));
            }
        }
        shell::tray::set_routes(&app, routes);
    });
}

/// Keeps the menu bar's route statuses current (connector state changes on its own).
fn watch_tray_routes<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            refresh_tray_routes(&app);
        }
    });
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

/// Whether any Teitunnel window has focus (then the user sees changes already).
fn any_window_focused<R: Runtime>(app: &AppHandle<R>) -> bool {
    app.webview_windows()
        .values()
        .any(|w| w.is_focused().unwrap_or(false))
}

fn notify<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    if any_window_focused(app) {
        return;
    }
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(error = %err, "failed to show notification");
    }
}

/// Notifies when this Mac's connector goes down, comes back, or crash-loops (the policy
/// is `core::health`: brief blips stay quiet). Connectors the user stopped are skipped.
fn watch_connector_health<R: Runtime>(app: AppHandle<R>) {
    use teitunnel_core::{
        engine::Connectors,
        health::{HealthWatch, Notice},
    };
    tauri::async_runtime::spawn(async move {
        let mut watch = HealthWatch::default();
        let mut tick = tokio::time::interval(Duration::from_secs(10));
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let enabled = settings::load(&state.store)
                .await
                .map_or(true, |s| s.notify_connectors);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            for account in state.accounts.list().await.unwrap_or_default() {
                let Ok(Some(tunnel)) = state.engine.local().machine_tunnel(&account.id).await
                else {
                    continue;
                };
                let id = tunnel.tunnel_id;
                let paused = state
                    .paused
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .contains(&id);
                if paused {
                    watch.forget(&id);
                    continue;
                }
                let notice = watch.observe(&id, state.machine.state(&id).as_ref(), now);
                if !enabled {
                    continue;
                }
                match notice {
                    Some(Notice::Down) => notify(
                        &app,
                        "Routes on this Mac are down",
                        "The connector lost its connection to Cloudflare. Teitunnel keeps retrying.",
                    ),
                    Some(Notice::Back) => {
                        notify(&app, "Routes are back", "The connector is connected again.");
                    }
                    Some(Notice::CrashLoop) => notify(
                        &app,
                        "The connector keeps stopping",
                        "Open Teitunnel's Doctor to see why.",
                    ),
                    None => {}
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
                    let enabled = match app.try_state::<AppState>() {
                        Some(state) => settings::load(&state.store)
                            .await
                            .map_or(true, |s| s.notify_quick_shares),
                        None => true,
                    };
                    if enabled {
                        notify_transition(&app, last.get(&id), current);
                    }
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
    if any_window_focused(app) || before == Some(&share.status) {
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
    // Routes run through the app's connectors: ask first (they can keep running as a
    // service instead).
    let routes_running = state
        .supervisor
        .ids()
        .iter()
        .any(|id| id.0.starts_with("tunnel-"));
    if routes_running
        && state.machine.supports_always_on()
        && !state.quit_confirmed.load(Ordering::SeqCst)
        && !state.shutting_down.load(Ordering::SeqCst)
    {
        api.prevent_exit();
        shell::windows::focus_main(app);
        let _ = crate::ipc::MenuAction {
            command: crate::ipc::MenuCommand::ConfirmQuit,
        }
        .emit(app);
        return;
    }
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
