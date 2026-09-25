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
    let (secrets, accounts, edge) = services(&store);
    let inspector = teitunnel_core::inspect::Inspector::new(
        Some(store.clone()),
        Some(secrets.clone()),
        teitunnel_core::domain_shares::APP_OWNER,
    );
    {
        let inspector = inspector.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(err) = inspector.load().await {
                tracing::warn!(%err, "couldn't read the inspector's settings and history");
            }
        });
    }
    let quick_shares = QuickShares::new(
        supervisor.clone(),
        binary.clone(),
        PortAllocator::new(QUICK_SHARE_PORTS),
        store.clone(),
        data_dir.join("quick-share.yml"),
    )
    .with_edge(edge)
    .with_inspector(inspector.clone());
    tauri::async_runtime::spawn(quick_shares.clone().watch_runtime());
    tauri::async_runtime::spawn(quick_shares.clone().watch_idle());
    watch_inspector(app.clone(), &inspector);
    watch_comments(app.clone(), &inspector);
    watch_snapshot_comments(app.clone());
    watch_inboxes(app.clone());
    watch_inspected_routes(app.clone());
    let local_domains = teitunnel_core::local_domains::LocalDomains::new(
        store.clone(),
        inspector.clone(),
        teitunnel_core::local_domains::LocalDomainsConfig::detect(&data_dir, Some(secrets.clone())),
    );
    start_local_domains(app.clone(), &local_domains);
    let pauses = Arc::new(teitunnel_core::pause::Enforcer::new());
    let schedules_changed = Arc::new(tokio::sync::Notify::new());
    watch_pauses(app.clone());
    forward_quick_share_changes(app.clone(), &quick_shares);

    let local = Local::new(store.clone());
    let paths = teitunnel_core::machine::ServicePaths {
        tokens: data_dir.join("tokens"),
        logs: data_dir.join("logs").join("connectors"),
    };
    let machine = MachineTunnels::new(
        supervisor.clone(),
        binary.clone(),
        PortAllocator::new(TUNNEL_PORTS),
        secrets.clone(),
        local.clone(),
    );
    let machine = match service_manager(&data_dir) {
        Some(manager) => machine.with_services(manager, paths),
        None => machine,
    };
    resume_machine_tunnels(app.clone(), accounts.clone(), machine.clone());
    watch_tray_routes(app.clone());
    watch_connector_health(app.clone());
    forward_connector_states(app.clone(), &supervisor);
    watch_doctor(app.clone());
    watch_domain_shares(app.clone());
    watch_uptime(app.clone());
    watch_snapshot_expiry(app.clone());
    tauri::async_runtime::spawn(machine.clone().sample_forever());
    let analytics = teitunnel_core::analytics::Analytics::default();
    let monitor = teitunnel_core::uptime::Monitor::new(
        store.clone(),
        accounts.clone(),
        analytics.clone(),
        edge,
        "app",
    );
    let engine = Arc::new(Engine::new(local));
    let control = control(
        app,
        &data_dir,
        ControlParts {
            store: &store,
            accounts: &accounts,
            engine: &engine,
            machine: &machine,
            quick_shares: &quick_shares,
            binary: &binary,
            inspector: &inspector,
            pauses: &pauses,
            local_domains: &local_domains,
        },
    );
    tauri::async_runtime::spawn(Arc::clone(&control.host).forward_requests(inspector.clone()));

    Ok(AppState {
        cli_runs: data_dir.join("run-cli"),
        snapshots: teitunnel_core::snapshot::Preparations::default(),
        snapshot_dir: data_dir.join("snapshots"),
        issued_secrets: teitunnel_core::protection::IssuedSecrets::default(),
        secrets,
        pending_restore: std::sync::Mutex::default(),
        accounts,
        engine,
        control,
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
        analytics,
        monitor,
        inspector,
        local_domains,
        pauses,
        schedules_changed,
        inspect_live: std::sync::Mutex::default(),
    })
}

/// Serves the local domains saved earlier (nothing when there are none), keeps them
/// healthy across sleep and network changes, and tells the webview when they change.
fn start_local_domains<R: Runtime>(
    app: AppHandle<R>,
    local_domains: &teitunnel_core::local_domains::LocalDomains,
) {
    let local = local_domains.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(err) = local.sync().await {
            tracing::warn!(%err, "local domains couldn't be served at launch");
        }
        local.run().await;
    });
    let mut changes = local_domains.subscribe();
    tauri::async_runtime::spawn(async move {
        use tokio::sync::broadcast::error::RecvError;
        while let Ok(()) | Err(RecvError::Lagged(_)) = changes.recv().await {
            let _ = EntityChanged {
                kind: EntityKind::LocalDomains,
                id: None,
            }
            .emit(&app);
        }
    });
}

/// What the control connection's host is made of.
#[derive(Clone, Copy)]
struct ControlParts<'a> {
    store: &'a Store,
    accounts: &'a Accounts,
    engine: &'a Arc<Engine>,
    machine: &'a MachineTunnels,
    quick_shares: &'a QuickShares,
    binary: &'a BinaryManager,
    inspector: &'a teitunnel_core::inspect::Inspector,
    pauses: &'a Arc<teitunnel_core::pause::Enforcer>,
    local_domains: &'a teitunnel_core::local_domains::LocalDomains,
}

/// The control connection's host over the app's services, listening unless it's turned
/// off in Settings ▸ Integrations.
fn control<R: Runtime>(
    app: &AppHandle<R>,
    data_dir: &std::path::Path,
    parts: ControlParts<'_>,
) -> shell::control::Control {
    use teitunnel_core::control::{CoreHost, HostParts, integrations};
    let ControlParts {
        store,
        accounts,
        engine,
        machine,
        quick_shares,
        binary,
        inspector,
        pauses,
        local_domains,
    } = parts;
    let host = CoreHost::new(
        HostParts {
            version: app.package_info().version.to_string(),
            store: store.clone(),
            accounts: accounts.clone(),
            engine: Arc::clone(engine),
            machine: machine.clone(),
            quick_shares: quick_shares.clone(),
            binary: binary.clone(),
            runs: data_dir.join("run-cli"),
            machine_name: machine_name(),
            local_domains: Some(local_domains.clone()),
            inspector: inspector.clone(),
            pauses: Arc::clone(pauses),
        },
        shell::control::ui(app),
    );
    shell::control::forward_changes(app, Arc::clone(&host));
    let control = shell::control::Control::new(host, data_dir);
    let enabled = tauri::async_runtime::block_on(integrations::load(store))
        .map_or(true, |s| s.control_enabled);
    if enabled {
        control.start();
    }
    control
}

/// Tells the webview when comments change and notifies about new ones on live shares
/// (outside quiet hours, unless the window is in front).
fn watch_comments<R: Runtime>(app: AppHandle<R>, inspector: &teitunnel_core::inspect::Inspector) {
    use teitunnel_core::{comments::CommentsEvent, text::msg::comments::notify as c};
    let Some(comments) = inspector.comments() else {
        return;
    };
    let mut events = comments.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            let key = match &event {
                CommentsEvent::New { subject, .. } => subject.key.clone(),
                CommentsEvent::Changed { subject } => subject.clone(),
            };
            let _ = EntityChanged {
                kind: EntityKind::Comments,
                id: Some(key),
            }
            .emit(&app);
            if let CommentsEvent::New {
                subject,
                author,
                excerpt,
                ..
            } = event
                && let Some(state) = app.try_state::<AppState>()
            {
                let prefs = settings::load(&state.store).await.unwrap_or_default();
                if !quiet_now(&state, &prefs).await {
                    notify(&app, &c::title(&subject.label), &c::body(&author, &excerpt));
                }
            }
        }
    });
}

/// Reads Snapshot comment counts from Cloudflare every two minutes (one D1 query per
/// account with commented Snapshots) and notifies about new ones.
fn watch_snapshot_comments<R: Runtime>(app: AppHandle<R>) {
    use teitunnel_core::{comments::SubjectKind, text::msg::comments::notify as c};
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(30)).await;
        let mut tick = tokio::time::interval(Duration::from_secs(120));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let Some(comments) = state.inspector.comments().cloned() else {
                return;
            };
            let subjects = comments.subjects().await.unwrap_or_default();
            let mut accounts: Vec<String> = subjects
                .iter()
                .filter(|s| s.subject.kind == SubjectKind::Snapshot)
                .filter_map(|s| s.subject.account_id.clone())
                .collect();
            accounts.sort();
            accounts.dedup();
            let prefs = settings::load(&state.store).await.unwrap_or_default();
            let quiet = quiet_now(&state, &prefs).await;
            for account in accounts {
                let Ok(api) = state.accounts.client(&account).await else {
                    continue;
                };
                match comments.poll_snapshots(&api, &account).await {
                    Ok(news) => {
                        for (subject, count) in news {
                            let _ = EntityChanged {
                                kind: EntityKind::Comments,
                                id: Some(subject.key.clone()),
                            }
                            .emit(&app);
                            if !quiet {
                                notify(
                                    &app,
                                    &c::title(&subject.label),
                                    &c::snapshot_body(u64::from(count)),
                                );
                            }
                        }
                    }
                    Err(err) => tracing::debug!(%err, "couldn't read Snapshot comments"),
                }
            }
        }
    });
}

/// Delivers webhooks the inboxes kept while this computer was off, every 30 seconds
/// (only accounts with an inbox on a route this computer serves; `core::inbox`).
fn watch_inboxes<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let Ok(http) = teitunnel_core::inbox::client() else {
            return;
        };
        // Connectors and local services take a moment at launch.
        tokio::time::sleep(Duration::from_secs(20)).await;
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let Ok(inboxes) = teitunnel_core::fronts::list(&state.engine, None).await else {
                continue;
            };
            let mut accounts: Vec<String> = inboxes
                .iter()
                .filter(|f| f.kind == teitunnel_core::engine::front::FrontKind::Inbox && f.routed)
                .map(|f| f.account_id.clone())
                .collect();
            accounts.sort();
            accounts.dedup();
            for account in accounts {
                let Ok(api) = state.accounts.client(&account).await else {
                    continue;
                };
                let Ok(reports) =
                    teitunnel_core::inbox::drain_account(&state.engine, &api, &http, &account)
                        .await
                else {
                    continue;
                };
                if reports.iter().any(|r| r.delivered > 0) {
                    let _ = EntityChanged {
                        kind: EntityKind::Fronts,
                        id: Some(account.clone()),
                    }
                    .emit(&app);
                }
            }
        }
    });
}

/// Tells the person a `teitunnel://` link couldn't be followed.
pub(crate) fn notify_link_failed<R: Runtime>(app: &AppHandle<R>, message: &str) {
    notify(
        app,
        &teitunnel_core::text::msg::control::link_failed(),
        &teitunnel_core::text::msg::raw(message),
    );
}

/// Tells the webview when taps change, notifies about requests to watched paths, and
/// stops shares on your domain that were idle for their limit (Quick Shares stop in
/// `QuickShares::watch_idle`).
fn watch_inspector<R: Runtime>(app: AppHandle<R>, inspector: &teitunnel_core::inspect::Inspector) {
    use teitunnel_core::inspect::{InspectEvent, TapScope};
    let mut events = inspector.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let event = match events.recv().await {
                Ok(event) => event,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            };
            match event {
                InspectEvent::Taps => {
                    let _ = EntityChanged {
                        kind: EntityKind::Inspector,
                        id: None,
                    }
                    .emit(&app);
                }
                InspectEvent::Watched {
                    name, method, path, ..
                } => notify(
                    &app,
                    &n::watched_path(&path),
                    &n::watched_path_body(&method, &path, &name),
                ),
                InspectEvent::Idle {
                    scope,
                    name,
                    minutes,
                    ..
                } => {
                    if let TapScope::Route {
                        account_id,
                        hostname,
                        path: None,
                    } = &scope
                        && let Some(state) = app.try_state::<AppState>()
                    {
                        let shared = state
                            .engine
                            .local()
                            .shares(Some(account_id))
                            .await
                            .unwrap_or_default()
                            .iter()
                            .any(|s| s.hostname.eq_ignore_ascii_case(hostname));
                        if !shared {
                            continue;
                        }
                        if let Ok(api) = state.accounts.client(account_id).await {
                            let ctx = teitunnel_core::engine::Context {
                                account: account_id,
                                machine_name: &state.machine_name,
                                tunnel: None,
                            };
                            let _ = teitunnel_core::domain_shares::stop(
                                &state.engine,
                                &api,
                                &state.machine,
                                ctx,
                                hostname,
                            )
                            .await;
                            if let Some(tap) = state.inspector.tap_for(&scope) {
                                state.inspector.stop(&tap).await;
                            }
                        }
                        let _ = EntityChanged {
                            kind: EntityKind::QuickShares,
                            id: None,
                        }
                        .emit(&app);
                    }
                    notify(
                        &app,
                        &n::idle_stopped(),
                        &n::idle_stopped_body(u64::from(minutes), &name),
                    );
                }
            }
        }
    });
}

/// Ends inspections of routes that are over: on launch also the app's own from its last
/// run (they end when it quits; this catches a crash), then every 30 s those of CLI
/// processes that exited.
fn watch_inspected_routes<R: Runtime>(app: AppHandle<R>) {
    use teitunnel_core::{domain_shares::APP_OWNER, inspect::routes};
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        let mut launch = true;
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let from_last_run = std::mem::take(&mut launch);
            let before = routes::list(&state.store, None).await.unwrap_or_default();
            if before.is_empty() {
                continue;
            }
            routes::sweep(
                &state.accounts,
                &state.engine,
                &state.machine,
                &state.machine_name,
                Some(&state.inspector),
                // The app's own rows from before this launch point at a Lens that's gone.
                |route| {
                    route.is_over()
                        || (from_last_run
                            && route.owner == APP_OWNER
                            && state
                                .inspector
                                .tap_for(&teitunnel_core::inspect::TapScope::route(
                                    &route.account_id,
                                    &route.hostname,
                                    route.path.as_deref(),
                                ))
                                .is_none())
                },
            )
            .await;
            let after = routes::list(&state.store, None).await.unwrap_or_default();
            if after.len() != before.len() {
                for kind in [EntityKind::Routes, EntityKind::Inspector] {
                    let _ = EntityChanged { kind, id: None }.emit(&app);
                }
            }
        }
    });
}

/// Serves paused pages and runs schedules (M12-06): the app holds the route host lease
/// (renewed every 30 s), evaluates schedules every 30 s (or at once when one changes),
/// and applies pauses to its inspector's taps every 3 s, so a pause asked for by
/// another process (`teitunnel shares --pause`, an agent) shows within seconds. Starts
/// after the first sweep of routes left inspected by a previous run.
fn watch_pauses<R: Runtime>(app: AppHandle<R>) {
    use teitunnel_core::{domain_shares::APP_OWNER, pause, schedule};
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(5)).await;
        let scheduler = schedule::Scheduler::new();
        let mut tick = tokio::time::interval(Duration::from_secs(3));
        let mut count = 0u32;
        let mut before: Vec<pause::PausedRoute> = Vec::new();
        loop {
            let Some(state) = app.try_state::<AppState>() else {
                tick.tick().await;
                continue;
            };
            let wake = Arc::clone(&state.schedules_changed);
            let woken = tokio::select! {
                _ = tick.tick() => false,
                () = wake.notified() => true,
            };
            if woken || count.is_multiple_of(10) {
                let _ = pause::claim_host(&state.store, APP_OWNER, true).await;
                for failure in schedule::run_tick(
                    &state.store,
                    &scheduler,
                    APP_OWNER,
                    true,
                    jiff::Timestamp::now(),
                )
                .await
                {
                    tracing::warn!("schedule: {}", failure.english());
                }
            }
            count = count.wrapping_add(1);
            let failures = state
                .pauses
                .sync(
                    &state.accounts,
                    &state.engine,
                    &state.machine,
                    &state.machine_name,
                    &state.inspector,
                )
                .await;
            for failure in failures {
                tracing::warn!("pause: {}", failure.english());
            }
            let after = pause::list(&state.store, None).await.unwrap_or_default();
            if after != before {
                for kind in [
                    EntityKind::QuickShares,
                    EntityKind::Routes,
                    EntityKind::Inspector,
                ] {
                    let _ = EntityChanged { kind, id: None }.emit(&app);
                }
                before = after;
            }
        }
    });
}

/// Evaluates schedules now (one was set or removed).
pub(crate) fn apply_schedules_soon<R: Runtime>(app: &AppHandle<R>) {
    if let Some(state) = app.try_state::<AppState>() {
        state.schedules_changed.notify_one();
    }
}

/// Where Always-on connectors run: launchd on macOS, systemd user units on Linux (when
/// there's a user session), scheduled tasks on Windows. E2E builds use child processes,
/// so tests never install real services.
fn service_manager(
    data_dir: &std::path::Path,
) -> Option<Arc<dyn teitunnel_core::service::ServiceManager>> {
    if cfg!(feature = "e2e") {
        return Some(Arc::new(teitunnel_core::service::ProcessServices::default()));
    }
    // The app runs in a user's session: their own services, never the system's.
    teitunnel_core::service::for_this_platform(data_dir, false)
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
            for tunnel in state
                .engine
                .local()
                .tunnels(&account.id)
                .await
                .unwrap_or_default()
            {
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
                tunnel: None,
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
            for tunnel in state
                .engine
                .local()
                .tunnels(&account.id)
                .await
                .unwrap_or_default()
            {
                tunnels.push((account.id.clone(), tunnel.tunnel_id));
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

/// Refreshes the Routes view and the menu bar when a connector changes state (starts,
/// connects, drops, stops), so neither waits for a poll. Bursts (a connector registering
/// its four connections) become one refresh.
fn forward_connector_states<R: Runtime>(app: AppHandle<R>, supervisor: &Supervisor) {
    use teitunnel_core::runtime::RuntimeEvent;
    use tokio::sync::broadcast::error::RecvError;
    let mut events = supervisor.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match events.recv().await {
                Ok(RuntimeEvent::State { .. }) | Err(RecvError::Lagged(_)) => {}
                Ok(RuntimeEvent::Log { .. }) => continue,
                Err(RecvError::Closed) => return,
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
            while let Ok(_) | Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) =
                events.try_recv()
            {}
            let _ = EntityChanged {
                kind: EntityKind::Routes,
                id: None,
            }
            .emit(&app);
            refresh_tray_routes(&app);
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
pub(crate) fn notify<R: Runtime>(app: &AppHandle<R>, title: &Text, body: &Text) {
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

/// Whether it's quiet hours now (alerts and connector notices are recorded, not shown).
async fn quiet_now(state: &AppState, settings: &settings::Settings) -> bool {
    settings.quiet_hours.enabled
        && teitunnel_core::alerts::local_minute(&state.store)
            .await
            .is_ok_and(|minute| settings.quiet_hours.contains(minute))
}

/// Checks every route through the edge once a minute and delivers alerts (the checks,
/// incidents and rules are `core::uptime` and `core::alerts`). Tunnels the user stopped
/// aren't checked.
fn watch_uptime<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        // Connectors take a moment to connect at launch.
        tokio::time::sleep(std::time::Duration::from_secs(20)).await;
        let mut tick = tokio::time::interval(teitunnel_core::uptime::INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let paused = state
                .paused
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone();
            let now = teitunnel_core::domain_shares::now_ms();
            let report = state
                .monitor
                .tick(i64::try_from(now).unwrap_or(i64::MAX), &paused)
                .await;
            if report.alerts.is_empty() {
                continue;
            }
            let _ = EntityChanged {
                kind: EntityKind::Routes,
                id: None,
            }
            .emit(&app);
            let settings = settings::load(&state.store).await.unwrap_or_default();
            if !settings.notify_alerts || quiet_now(&state, &settings).await {
                continue;
            }
            let shown: Vec<&teitunnel_core::alerts::Alert> =
                report.alerts.iter().filter(|a| a.notify).collect();
            if let Some((title, body)) = teitunnel_core::alerts::notice(&shown) {
                notify(&app, &title, &body);
            }
        }
    });
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
            let prefs = settings::load(&state.store).await.unwrap_or_default();
            let enabled = prefs.notify_connectors && !quiet_now(&state, &prefs).await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            let mut tunnels = Vec::new();
            for account in state.accounts.list().await.unwrap_or_default() {
                tunnels.extend(
                    state
                        .engine
                        .local()
                        .tunnels(&account.id)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .map(|tunnel| (account.id.clone(), tunnel)),
                );
            }
            for (account, tunnel) in tunnels {
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
                // Recorded in Activity as an alert, whatever the notification settings.
                if let Some(down) = match notice {
                    Some(Notice::Down | Notice::CrashLoop) => Some(true),
                    Some(Notice::Back) => Some(false),
                    None => None,
                } {
                    let at = i64::try_from(now).unwrap_or(i64::MAX);
                    state
                        .monitor
                        .connector_changed(&account, &tunnel.name, down, at)
                        .await;
                }
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
/// Ends shares on your domain that are over: on launch also the app's own from its last
/// run (they end when it quits; this catches a crash), then every 30 s the expired ones
/// and those of CLI processes that exited.
fn watch_domain_shares<R: Runtime>(app: AppHandle<R>) {
    use teitunnel_core::domain_shares::{self, APP_OWNER};
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        let mut launch = true;
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let now = domain_shares::now_ms();
            let before = state.engine.local().shares(None).await.unwrap_or_default();
            if before.is_empty() {
                launch = false;
                continue;
            }
            let from_last_run = std::mem::take(&mut launch);
            domain_shares::sweep(
                &state.accounts,
                &state.engine,
                &state.machine,
                &state.machine_name,
                |share| share.is_over(now) || (from_last_run && share.owner == APP_OWNER),
            )
            .await;
            let after = state.engine.local().shares(None).await.unwrap_or_default();
            if after.len() != before.len() {
                let _ = crate::ipc::EntityChanged {
                    kind: crate::ipc::EntityKind::QuickShares,
                    id: None,
                }
                .emit(&app);
                refresh_tray_routes(&app);
            }
        }
    });
}

/// Deletes Snapshots whose expiry passed: at launch, then hourly.
fn watch_snapshot_expiry<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(60 * 60));
        loop {
            tick.tick().await;
            let Some(state) = app.try_state::<AppState>() else {
                continue;
            };
            let now = teitunnel_core::domain_shares::now_ms();
            let due = state
                .engine
                .local()
                .sites(None)
                .await
                .unwrap_or_default()
                .into_iter()
                .any(|s| s.expires_at.is_some_and(|at| at <= now));
            if !due {
                continue;
            }
            teitunnel_core::snapshot::sweep_expired(
                &state.accounts,
                &state.engine,
                &state.machine,
                &state.machine_name,
            )
            .await;
            let _ = crate::ipc::EntityChanged {
                kind: crate::ipc::EntityKind::Snapshots,
                id: None,
            }
            .emit(&app);
        }
    });
}

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
            let mut issues = teitunnel_core::doctor::run(
                &state.accounts,
                &state.engine,
                &state.machine,
                &state.binary,
                &state.machine_name,
            )
            .await;
            issues.extend(state.local_domains.doctor().await);
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
    // Shares on your domain end with the app; the check is quick when there are none.
    api.prevent_exit();
    let quick_shares = state.quick_shares.clone();
    let supervisor = state.supervisor.clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let stop = async {
            if let Some(state) = app.try_state::<AppState>() {
                use teitunnel_core::domain_shares::{self, APP_OWNER};
                // Routes pointed at this app's inspector go back to their own service
                // first (Lens ends with the app; an Always-on connector keeps running).
                teitunnel_core::inspect::routes::sweep(
                    &state.accounts,
                    &state.engine,
                    &state.machine,
                    &state.machine_name,
                    Some(&state.inspector),
                    |route| route.owner == APP_OWNER,
                )
                .await;
                domain_shares::sweep(
                    &state.accounts,
                    &state.engine,
                    &state.machine,
                    &state.machine_name,
                    |share| share.owner == APP_OWNER,
                )
                .await;
            }
            quick_shares.stop_all().await;
            supervisor.stop_all().await;
            if let Some(state) = app.try_state::<AppState>() {
                state.monitor.release().await;
                state.local_domains.stop().await;
                let _ = teitunnel_core::pause::release_host(
                    &state.store,
                    teitunnel_core::domain_shares::APP_OWNER,
                )
                .await;
                state.inspector.shutdown().await;
            }
        };
        if tokio::time::timeout(SHUTDOWN_DEADLINE, stop).await.is_err() {
            tracing::warn!("connectors didn't stop in time; exiting anyway");
        }
        // Last: a downloaded update installs now (on Windows its installer takes over).
        let restart = app
            .try_state::<crate::shell::updates::Updates>()
            .is_some_and(|updates| updates.finish_on_exit());
        if restart {
            app.restart();
        }
        app.exit(0);
    });
}
