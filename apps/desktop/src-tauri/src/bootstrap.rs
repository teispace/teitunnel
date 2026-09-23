//! Startup and shutdown of the core services.

use std::{
    collections::HashMap,
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use teitunnel_core::text::{Text, msg::notify as n};

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
    let machine = match service_manager(&data_dir) {
        Some(manager) => machine.with_services(manager, paths),
        None => machine,
    };
    resume_machine_tunnels(app.clone(), accounts.clone(), machine.clone());
    watch_tray_routes(app.clone());
    watch_connector_health(app.clone());
    watch_doctor(app.clone());
    tauri::async_runtime::spawn(machine.clone().sample_forever());

    Ok(AppState {
        accounts,
        engine: Engine::new(local),
        machine,
        remote_logs: teitunnel_core::remote_logs::RemoteLogs::default(),
        machine_name: machine_name(),
        edge,
        store,
        binary,
        supervisor,
        quick_shares,
        oauth_cancel: std::sync::Mutex::default(),
        doctor: teitunnel_core::doctor_monitor::DoctorMonitor::default(),
        paused: std::sync::Mutex::default(),
        quit_confirmed: false.into(),
        shutting_down: false.into(),
    })
}

/// Where Always-on connectors run: launchd on macOS, systemd user units on Linux (when
/// there's a user session), scheduled tasks on Windows. E2E builds use child processes,
/// so tests never install real services.
fn service_manager(
    data_dir: &std::path::Path,
) -> Option<Arc<dyn teitunnel_core::service::ServiceManager>> {
    use teitunnel_core::service::{
        Launchd, ProcessServices, ServiceManager, Systemd, TaskScheduler,
    };
    fn shared(manager: impl ServiceManager + 'static) -> Arc<dyn ServiceManager> {
        Arc::new(manager)
    }
    if cfg!(feature = "e2e") {
        return Some(shared(ProcessServices::default()));
    }
    if cfg!(target_os = "macos") {
        Launchd::for_current_user().map(shared)
    } else if cfg!(target_os = "linux") {
        Systemd::for_current_user().map(shared)
    } else if cfg!(windows) {
        TaskScheduler::for_current_user(data_dir.join("tasks")).map(shared)
    } else {
        None
    }
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
        let (mut tunnels, mut running, mut paused) = (0, false, 0);
        for account in state.accounts.list().await.unwrap_or_default() {
            if let Ok(Some(tunnel)) = state.engine.local().machine_tunnel(&account.id).await {
                use teitunnel_core::{engine::Connectors, runtime::ConnectorState};
                tunnels += 1;
                running |= !matches!(
                    state.machine.state(&tunnel.tunnel_id),
                    None | Some(ConnectorState::Stopped)
                );
                paused += usize::from(is_paused(&state, &tunnel.tunnel_id));
            }
            let Ok(api) = state.accounts.client(&account.id).await else {
                continue;
            };
            let ctx = teitunnel_core::engine::Context {
                account: &account.id,
                machine_name: &state.machine_name,
            };
            if let Ok(overview) = state.engine.overview(&api, &state.machine, ctx).await {
                routes.extend(
                    overview
                        .statuses()
                        .into_iter()
                        .map(|(hostname, status)| shell::tray::TrayRoute { hostname, status }),
                );
            }
        }
        let connectors = match (tunnels, running) {
            (0, _) => shell::tray::TrayConnectors::None,
            (_, true) => shell::tray::TrayConnectors::Running,
            (_, false) => shell::tray::TrayConnectors::Stopped {
                on_purpose: paused == tunnels,
            },
        };
        shell::tray::set_routes(&app, shell::tray::TrayRoutes { routes, connectors });
    });
}

fn is_paused(state: &AppState, tunnel_id: &str) -> bool {
    state
        .paused
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .contains(tunnel_id)
}

/// After a cloudflared update: restarts this Mac's connectors on the new binary, one
/// account at a time, each checked healthy before the next (`restart_on_current_binary`).
pub(crate) fn move_connectors_to_current_binary<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        for account in state.accounts.list().await.unwrap_or_default() {
            let Ok(api) = state.accounts.client(&account.id).await else {
                continue;
            };
            match state
                .machine
                .restart_on_current_binary(&api, &account.id)
                .await
            {
                Ok(true) => {
                    tracing::info!(account = %account.id, "connector moved to the new cloudflared");
                }
                Ok(false) => {}
                Err(err) => {
                    tracing::warn!(account = %account.id, %err, "couldn't move the connector to the new cloudflared");
                    notify(&app, &n::old_binary(), &err);
                }
            }
        }
        refresh_tray_routes(&app);
    });
}

/// The menu bar's Start/Stop Routes: stops every connector on this Mac if any runs,
/// otherwise starts them all (the same actions as the Tunnels view).
pub(crate) fn toggle_machine_routes<R: Runtime>(app: &AppHandle<R>) {
    use teitunnel_core::{engine::Connectors, runtime::ConnectorState};
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let mut tunnels = Vec::new();
        for account in state.accounts.list().await.unwrap_or_default() {
            if let Ok(Some(tunnel)) = state.engine.local().machine_tunnel(&account.id).await {
                tunnels.push((account.id, tunnel.tunnel_id));
            }
        }
        let any_running = tunnels.iter().any(|(_, id)| {
            !matches!(
                state.machine.state(id),
                None | Some(ConnectorState::Stopped)
            )
        });
        for (account, tunnel) in &tunnels {
            let result = if any_running {
                crate::ipc::stop_machine(&app, &state, account, tunnel).await
            } else {
                crate::ipc::start_machine(&app, &state, account).await
            };
            if let Err(err) = result {
                tracing::warn!(%err, "couldn't switch this Mac's routes from the menu bar");
                notify(&app, &n::switch_failed(), &err.message);
            }
        }
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

/// Shows a notification in the user's language, unless a Teitunnel window is in front.
fn notify<R: Runtime>(app: &AppHandle<R>, title: &Text, body: &Text) {
    if any_window_focused(app) {
        return;
    }
    if let Err(err) = app
        .notification()
        .builder()
        .title(title.to_string())
        .body(body.to_string())
        .show()
    {
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
                    Some(Notice::Down) => notify(&app, &n::routes_down(), &n::routes_down_body()),
                    Some(Notice::Back) => notify(&app, &n::routes_back(), &n::routes_back_body()),
                    Some(Notice::CrashLoop) => {
                        notify(&app, &n::crash_loop(), &n::crash_loop_body());
                    }
                    None => {}
                }
            }
        }
    });
}

/// Records a Doctor run and notifies about new errors (if the user wants that).
pub(crate) async fn doctor_ran<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    issues: &[teitunnel_core::doctor::Issue],
) {
    let settings = settings::load(&state.store).await.unwrap_or_default();
    let ignored = settings.ignored_issues.into_iter().collect();
    let notice = state
        .doctor
        .record(issues, &ignored, std::time::Instant::now());
    if let (Some(notice), true) = (notice, settings.notify_doctor) {
        notify(app, &notice.title, &notice.body);
    }
}

/// Runs the Doctor in the background when nothing else has for a while (the window
/// runs it while open), so problems are noticed with the window closed.
fn watch_doctor<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60));
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            if !state.doctor.due(std::time::Instant::now()) {
                continue;
            }
            let issues = teitunnel_core::doctor::run(
                &state.accounts,
                &state.engine,
                &state.machine,
                &state.binary,
                &state.machine_name,
            )
            .await;
            doctor_ran(&app, &state, &issues).await;
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
            (n::share_live(), share.url.clone().unwrap_or_default())
        }
        (ShareStatus::Failed { message }, _) => (n::share_failed(), message.to_string()),
        _ => return,
    };
    if let Err(err) = app
        .notification()
        .builder()
        .title(title.to_string())
        .body(body)
        .show()
    {
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
