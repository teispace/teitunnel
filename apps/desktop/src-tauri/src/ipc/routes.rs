//! Routes commands: overview, preview, apply (with progress), verify, drift and
//! activity. Every Cloudflare change goes through the engine's plan → apply path.

use std::time::Duration;

use tauri::{AppHandle, Runtime, State, ipc::Channel};
use tauri_specta::Event;
use teitunnel_core::engine::Connectors;
use teitunnel_core::engine::{
    ActivityEntry, Approval, Change, Context, Drift, Outcome, PlanView, Progress, RoutesOverview,
    TunnelSummary, Verification,
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

/// How long a check right after applying waits for the connector and propagation.
const VERIFY_PATIENCE: Duration = Duration::from_secs(30);

fn context<'a>(state: &'a AppState, account_id: &'a str) -> Context<'a> {
    Context {
        account: account_id,
        machine_name: &state.machine_name,
    }
}

fn changed<R: Runtime>(app: &AppHandle<R>, account_id: &str) {
    crate::bootstrap::refresh_tray_routes(app);
    let _ = EntityChanged {
        kind: EntityKind::Routes,
        id: Some(account_id.to_owned()),
    }
    .emit(app);
}

/// This Mac's tunnel and routes in an account.
#[tauri::command]
#[specta::specta]
pub async fn routes_overview(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<RoutesOverview, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(state
        .engine
        .overview(&api, &state.machine, context(&state, &account_id))
        .await?)
}

/// Plans a change for review. Nothing is changed.
#[tauri::command]
#[specta::specta]
pub async fn routes_preview(
    state: State<'_, AppState>,
    account_id: String,
    change: Change,
) -> Result<PlanView, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let ctx = context(&state, &account_id);
    let intent = state.engine.intent_for(&api, ctx, &change).await?;
    let plan = state.engine.preview(&api, ctx, &intent).await?;
    Ok(plan.view(&account_id))
}

/// Applies a reviewed change. Step progress streams on `on_progress`. Fails with
/// `conflict` if anything changed since the preview (preview again).
#[tauri::command]
#[specta::specta]
pub async fn routes_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    change: Change,
    fingerprint: String,
    confirmed: bool,
    on_progress: Channel<Progress>,
) -> Result<Outcome, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let ctx = context(&state, &account_id);
    let intent = state.engine.intent_for(&api, ctx, &change).await?;
    let approval = Approval {
        fingerprint: &fingerprint,
        confirmed,
    };
    let outcome = state
        .engine
        .apply(&api, &state.machine, ctx, &intent, approval, |p| {
            let _ = on_progress.send(p);
        })
        .await;
    changed(&app, &account_id);
    Ok(outcome?)
}

/// Checks a route end to end. With `wait`, transient failures (connector connecting,
/// propagation) are retried for up to 30 s, as right after applying.
#[tauri::command]
#[specta::specta]
pub async fn routes_verify(
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    wait: bool,
) -> Result<Verification, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let hostname = teitunnel_core::domain::Hostname::parse(&hostname)
        .map_err(|e| AppError::invalid("hostname", e.to_string()))?;
    let patience = if wait {
        VERIFY_PATIENCE
    } else {
        Duration::ZERO
    };
    Ok(state
        .engine
        .verify(
            &api,
            context(&state, &account_id),
            &hostname,
            state.edge,
            patience,
        )
        .await?)
}

/// An outside edit of this Mac's routes, if there is one.
#[tauri::command]
#[specta::specta]
pub async fn routes_drift(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<Drift>, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(state.engine.drift(&api, &account_id).await?)
}

/// Accepts an outside edit as the new baseline ("Keep theirs").
#[tauri::command]
#[specta::specta]
pub async fn routes_keep_theirs(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), AppError> {
    let api = state.accounts.client(&account_id).await?;
    if let Some(drift) = state.engine.drift(&api, &account_id).await? {
        state.engine.keep_theirs(&account_id, &drift).await?;
    }
    changed(&app, &account_id);
    Ok(())
}

/// Recent changes in an account, newest first.
#[tauri::command]
#[specta::specta]
pub async fn routes_activity(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<ActivityEntry>, AppError> {
    Ok(state.engine.local().activity(&account_id, 50).await?)
}

/// Every tunnel in the account, this Mac's first.
#[tauri::command]
#[specta::specta]
pub async fn tunnels_list(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<TunnelSummary>, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(state
        .engine
        .tunnels(&api, &state.machine, &account_id)
        .await?)
}

/// Starts this Mac's connector for the account's tunnel.
#[tauri::command]
#[specta::specta]
pub async fn tunnels_start(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), AppError> {
    start_machine(&app, &state, &account_id).await
}

/// Stops this Mac's connector for a tunnel. Its routes stop answering until it starts.
#[tauri::command]
#[specta::specta]
pub async fn tunnels_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    tunnel_id: String,
) -> Result<(), AppError> {
    stop_machine(&app, &state, &account_id, &tunnel_id).await
}

/// Starts `account_id`'s connector on this Mac and clears its "stopped on purpose" mark
/// (shared by the Tunnels view and the menu bar).
pub(crate) async fn start_machine<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    account_id: &str,
) -> Result<(), AppError> {
    let api = state.accounts.client(account_id).await?;
    if let Ok(Some(tunnel)) = state.engine.local().machine_tunnel(account_id).await {
        state
            .paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&tunnel.tunnel_id);
    }
    state
        .machine
        .resume(&api, account_id)
        .await
        .map_err(AppError::internal)?;
    changed(app, account_id);
    Ok(())
}

/// Stops a tunnel's connector on this Mac, marked as stopped on purpose so it isn't
/// reported as down.
pub(crate) async fn stop_machine<R: Runtime>(
    app: &AppHandle<R>,
    state: &AppState,
    account_id: &str,
    tunnel_id: &str,
) -> Result<(), AppError> {
    state
        .paused
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(tunnel_id.to_owned());
    state
        .machine
        .stop(tunnel_id)
        .await
        .map_err(AppError::internal)?;
    changed(app, account_id);
    Ok(())
}

/// Removes a tunnel's stale connections (left by connectors that went away uncleanly).
#[tauri::command]
#[specta::specta]
pub async fn tunnels_clean(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    tunnel_id: String,
) -> Result<(), AppError> {
    let api = state.accounts.client(&account_id).await?;
    api.clean_connections(&account_id, &tunnel_id)
        .await
        .map_err(teitunnel_core::Error::from)?;
    changed(&app, &account_id);
    Ok(())
}

/// cloudflared configurations found on this Mac (`~/.cloudflared/config.yml`, …). Only
/// reads; credentials' secrets are never read.
#[tauri::command]
#[specta::specta]
pub async fn import_scan() -> Result<Vec<teitunnel_core::import::LocalSetup>, AppError> {
    tauri::async_runtime::spawn_blocking(teitunnel_core::import::scan)
        .await
        .map_err(|err| AppError::internal(format!("Couldn't look for cloudflared setups: {err}")))
}

/// cloudflared processes on this Mac that Teitunnel didn't start.
#[tauri::command]
#[specta::specta]
pub async fn foreign_list()
-> Result<Vec<teitunnel_core::discovery::cloudflared::ForeignConnector>, AppError> {
    Ok(teitunnel_core::discovery::cloudflared::foreign().await)
}

/// Stops a cloudflared process Teitunnel didn't start (after re-checking it's one).
#[tauri::command]
#[specta::specta]
pub async fn foreign_stop(pid: u32) -> Result<(), AppError> {
    if teitunnel_core::discovery::cloudflared::stop(pid).await {
        Ok(())
    } else {
        Err(AppError::internal(
            "That cloudflared isn't running any more.",
        ))
    }
}

/// The newest log lines of this Mac's connector for a tunnel.
#[tauri::command]
#[specta::specta]
pub fn tunnels_logs(
    state: State<'_, AppState>,
    tunnel_id: String,
    limit: u32,
) -> Vec<crate::ipc::quick_share::LogLine> {
    crate::ipc::quick_share::log_lines(
        &state
            .machine
            .logs(&tunnel_id, usize::try_from(limit).unwrap_or(usize::MAX)),
    )
}

/// A connector's live logs, when it runs on another machine.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct RemoteLogsView {
    /// Where the stream is.
    pub state: teitunnel_core::remote_logs::RemoteLogState,
    /// The newest lines, oldest first.
    pub lines: Vec<crate::ipc::quick_share::LogLine>,
}

/// The newest log lines of a connector anywhere, streamed through Cloudflare. The first
/// call starts the stream; it stops by itself once nobody asks for 30 s.
#[tauri::command]
#[specta::specta]
pub async fn tunnels_remote_logs(
    state: State<'_, AppState>,
    account_id: String,
    tunnel_id: String,
    connector_id: String,
    limit: u32,
) -> Result<RemoteLogsView, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let batch = state.remote_logs.read(
        &api,
        &account_id,
        &tunnel_id,
        &connector_id,
        usize::try_from(limit).unwrap_or(usize::MAX),
    );
    Ok(RemoteLogsView {
        state: batch.state,
        lines: crate::ipc::quick_share::log_lines(&batch.lines),
    })
}

/// Stops a connector's live logs (closing the viewer, or before trying again).
#[tauri::command]
#[specta::specta]
pub fn tunnels_remote_logs_stop(
    state: State<'_, AppState>,
    account_id: String,
    tunnel_id: String,
    connector_id: String,
) {
    state
        .remote_logs
        .stop(&account_id, &tunnel_id, &connector_id);
}

/// This Mac's tunnel and routes as `config.yml`, Docker Compose or Terraform. `None` if
/// this Mac has no tunnel in the account. Never contains a secret.
#[tauri::command]
#[specta::specta]
pub async fn routes_export(
    state: State<'_, AppState>,
    account_id: String,
    format: teitunnel_core::export::ExportFormat,
) -> Result<Option<teitunnel_core::export::ExportFile>, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let version = state
        .binary
        .current()
        .await
        .ok()
        .and_then(|b| b.version)
        .map(|v| v.to_string());
    let input = state
        .engine
        .export_input(&api, context(&state, &account_id), version)
        .await?;
    Ok(input.map(|input| teitunnel_core::export::render(&input, format)))
}

/// Saves an export to Downloads and shows it in Finder. Returns its path.
#[tauri::command]
#[specta::specta]
pub async fn routes_export_save(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    format: teitunnel_core::export::ExportFormat,
) -> Result<String, AppError> {
    let file = routes_export(state, account_id, format)
        .await?
        .ok_or_else(|| {
            AppError::invalid("account", "This Mac has no routes in this account yet.")
        })?;
    let contents = file.contents;
    crate::ipc::app::save_to_downloads(&app, &file.file_name, move |path| {
        std::fs::write(path, contents)
    })
    .await
}

/// The newest log lines about requests for one route (its failed requests, and every
/// request when cloudflared logs at debug level).
#[tauri::command]
#[specta::specta]
pub async fn routes_logs(
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    path: Option<String>,
    limit: u32,
) -> Result<Vec<crate::ipc::quick_share::LogLine>, AppError> {
    let events = state
        .machine
        .route_logs(
            &account_id,
            &hostname,
            path.as_deref(),
            usize::try_from(limit).unwrap_or(usize::MAX),
        )
        .await
        .map_err(AppError::internal)?;
    Ok(crate::ipc::quick_share::log_lines(&events))
}

/// This Mac's connector traffic for a tunnel: samples after `since` (ms; the last hour
/// without it) and the latest numbers. Polling this keeps sampling at 1 s (D-046).
#[tauri::command]
#[specta::specta]
pub fn tunnels_traffic(
    state: State<'_, AppState>,
    tunnel_id: String,
    since: Option<f64>,
) -> Option<teitunnel_core::traffic::Traffic> {
    state.machine.traffic(&tunnel_id, since)
}

/// A tunnel's traffic over the last day or week, from per-minute history.
#[tauri::command]
#[specta::specta]
pub async fn tunnels_traffic_history(
    state: State<'_, AppState>,
    tunnel_id: String,
    range: teitunnel_core::traffic::HistoryRange,
) -> Result<teitunnel_core::traffic::TrafficSeries, AppError> {
    state
        .machine
        .traffic_history(&tunnel_id, range)
        .await
        .map_err(AppError::internal)
}

/// Whether this Mac's connector can run as a service, and whether it does.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AlwaysOn {
    /// This system supports Always-on.
    pub supported: bool,
    /// The connector runs as a service.
    pub enabled: bool,
}

/// Whether this Mac's connector for the account keeps running when Teitunnel quits.
#[tauri::command]
#[specta::specta]
pub async fn tunnels_always_on(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<AlwaysOn, AppError> {
    let tunnel = state.engine.local().machine_tunnel(&account_id).await?;
    Ok(AlwaysOn {
        supported: state.machine.supports_always_on(),
        enabled: tunnel.is_some_and(|t| state.machine.is_always_on(&t.tunnel_id)),
    })
}

/// Switches this Mac's connector between running with the app and running as a
/// service (keeps running after quit and at login), without a gap.
#[tauri::command]
#[specta::specta]
pub async fn tunnels_set_always_on(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    enabled: bool,
) -> Result<(), AppError> {
    let api = state.accounts.client(&account_id).await?;
    state
        .machine
        .set_always_on(&api, &account_id, enabled)
        .await
        .map_err(AppError::internal)?;
    changed(&app, &account_id);
    Ok(())
}
