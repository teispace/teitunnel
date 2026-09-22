//! Startup and shutdown of the core services.

use std::{sync::atomic::Ordering, time::Duration};

use tauri::{AppHandle, Manager, Runtime};
use tauri_specta::Event;
use teitunnel_core::{
    binary::{BinaryManager, Locator},
    quick_share::QuickShares,
    runtime::{PidRegistry, PortAllocator, QUICK_SHARE_PORTS, Supervisor},
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
    let data_dir = app.path().app_data_dir()?;
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

    Ok(AppState {
        store,
        binary,
        supervisor,
        quick_shares,
        shutting_down: false.into(),
    })
}

/// Turns Quick Share changes into `EntityChanged` events for the webviews.
fn forward_quick_share_changes<R: Runtime>(app: AppHandle<R>, quick_shares: &QuickShares) {
    let mut changes = quick_shares.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match changes.recv().await {
                Ok(id) => {
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
