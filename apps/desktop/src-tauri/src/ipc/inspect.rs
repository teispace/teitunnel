//! The inspector: taps, captured requests (masked unless revealed), live updates,
//! replay, exports, tap settings, webhook secrets and route inspection. The logic is
//! `teitunnel_core::inspect`.

use std::sync::atomic::{AtomicU32, Ordering};

use tauri::{AppHandle, State, ipc::Channel};
use tauri_specta::Event;
use teitunnel_core::{
    Secret,
    engine::{Approval, Context, Outcome, Progress},
    inspect::{
        ExchangeDetail, ExchangePage, ExchangeQuery, ExchangeRow, InspectError, InspectorSettings,
        InspectorSettingsPatch, KnownTap, LiveBatch, ProtectionInput, ProtectionResult,
        ReplayInput, TapPatch, TapView, TrafficFormat, WebhookCheck, WebhookSender as Provider,
        lens::{ExchangeId, MetricsSnapshot, TapId},
        routes::{self, InspectPlan, InspectedRoute},
        secrets,
    },
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn err(error: InspectError) -> AppError {
    teitunnel_core::Error::from(error).into()
}

fn changed(app: &AppHandle, kinds: &[EntityKind]) {
    for kind in kinds {
        let _ = EntityChanged {
            kind: *kind,
            id: None,
        }
        .emit(app);
    }
}

/// The inspector's settings.
#[tauri::command]
#[specta::specta]
pub fn inspect_settings_get(state: State<'_, AppState>) -> InspectorSettings {
    state.inspector.settings()
}

/// Changes the inspector's settings.
#[tauri::command]
#[specta::specta]
pub async fn inspect_settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: InspectorSettingsPatch,
) -> Result<InspectorSettings, AppError> {
    let settings = state.inspector.update_settings(patch).await.map_err(err)?;
    changed(&app, &[EntityKind::Inspector]);
    Ok(settings)
}

/// Running taps (inspected shares and routes).
#[tauri::command]
#[specta::specta]
pub fn inspect_taps(state: State<'_, AppState>) -> Vec<TapView> {
    state.inspector.taps()
}

/// Every tap captures refer to: running, stopped, or from the history.
#[tauri::command]
#[specta::specta]
pub fn inspect_known_taps(state: State<'_, AppState>) -> Vec<KnownTap> {
    state.inspector.known_taps()
}

/// A page of captured requests, newest first, masked.
#[tauri::command]
#[specta::specta]
pub fn inspect_exchanges(state: State<'_, AppState>, query: ExchangeQuery) -> ExchangePage {
    state.inspector.list(&query)
}

/// One captured request in full: masked, or revealed (only when the person clicked to
/// reveal secrets).
#[tauri::command]
#[specta::specta]
pub async fn inspect_exchange(
    state: State<'_, AppState>,
    id: ExchangeId,
    reveal: bool,
) -> Result<ExchangeDetail, AppError> {
    state.inspector.detail(id, reveal).await.map_err(err)
}

static NEXT_SUBSCRIPTION: AtomicU32 = AtomicU32::new(1);

/// Streams changes to captured requests on `on_batch`, at most every 100 ms, until
/// `inspect_unsubscribe` with the returned id (or the window goes away).
#[tauri::command]
#[specta::specta]
// Async: on the runtime, not the main thread (starting Lens spawns its tasks).
pub async fn inspect_subscribe(
    state: State<'_, AppState>,
    on_batch: Channel<LiveBatch>,
) -> Result<u32, AppError> {
    let id = NEXT_SUBSCRIPTION.fetch_add(1, Ordering::Relaxed);
    let stop = tokio_util::sync::CancellationToken::new();
    state
        .inspect_live
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .insert(id, stop.clone());
    let inspector = state.inspector.clone();
    // Fails early if Lens can't start.
    drop(inspector.live().map_err(err)?);
    tauri::async_runtime::spawn(async move {
        let _ = teitunnel_core::inspect::follow(&inspector, stop, move |batch| {
            on_batch.send(batch).is_ok()
        })
        .await;
    });
    Ok(id)
}

/// Stops a live subscription.
#[tauri::command]
#[specta::specta]
pub fn inspect_unsubscribe(state: State<'_, AppState>, id: u32) {
    if let Some(stop) = state
        .inspect_live
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&id)
    {
        stop.cancel();
    }
}

/// Sends a captured request to its service again: edited, repeated, re-signed.
#[tauri::command]
#[specta::specta]
pub async fn inspect_replay(
    state: State<'_, AppState>,
    id: ExchangeId,
    input: ReplayInput,
) -> Result<Vec<ExchangeRow>, AppError> {
    state.inspector.replay(id, input).await.map_err(err)
}

/// Captured requests as cURL, HTTPie, fetch, raw HTTP, HAR, JSON or Markdown; secrets
/// are masked unless `redact` is false (an explicit choice).
#[tauri::command]
#[specta::specta]
pub async fn inspect_export(
    state: State<'_, AppState>,
    ids: Vec<ExchangeId>,
    format: TrafficFormat,
    redact: bool,
) -> Result<String, AppError> {
    // Bodies may be read back from disk: off the main thread.
    let inspector = state.inspector.clone();
    super::off_main(move || inspector.export(&ids, format.into(), redact).map_err(err)).await?
}

/// Saves an export to Downloads and shows it in the file manager. Returns its path.
#[tauri::command]
#[specta::specta]
pub async fn inspect_export_save(
    app: AppHandle,
    state: State<'_, AppState>,
    ids: Vec<ExchangeId>,
    format: TrafficFormat,
    redact: bool,
) -> Result<String, AppError> {
    let contents = state
        .inspector
        .export(&ids, format.into(), redact)
        .map_err(err)?;
    let extension = match format {
        TrafficFormat::Har => "har",
        TrafficFormat::Json => "json",
        TrafficFormat::Markdown => "md",
        TrafficFormat::Fetch => "js",
        TrafficFormat::Curl | TrafficFormat::Httpie => "sh",
        TrafficFormat::Raw => "http",
    };
    let name = format!("teitunnel-requests.{extension}");
    crate::ipc::app::save_to_downloads(&app, &name, move |path| std::fs::write(path, contents))
        .await
}

/// Describes the API the captured requests show as OpenAPI 3.1 (to `host`, or every
/// host), saves it to Downloads as JSON and shows it in the file manager. Returns what
/// went into it and where it is.
#[tauri::command]
#[specta::specta]
pub async fn inspect_openapi_save(
    app: AppHandle,
    state: State<'_, AppState>,
    host: Option<String>,
) -> Result<OpenApiSaved, AppError> {
    let options = teitunnel_core::openapi::Options {
        host: host.filter(|h| !h.trim().is_empty()),
        title: None,
    };
    let (document, summary) =
        teitunnel_core::openapi::describe(Some(&state.inspector), Some(&state.store), &options)
            .await?;
    let text = teitunnel_core::openapi::render(&document, false)
        .map_err(|e| AppError::internal(teitunnel_core::text::msg::raw(e)))?;
    let name = teitunnel_core::diagnostics::timestamped_name("teitunnel-openapi", "json");
    let path =
        crate::ipc::app::save_to_downloads(&app, &name, move |path| std::fs::write(path, text))
            .await?;
    Ok(OpenApiSaved { path, summary })
}

/// An OpenAPI description saved to Downloads.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct OpenApiSaved {
    /// Where it is.
    pub path: String,
    /// What went into it.
    pub summary: teitunnel_core::openapi::Summary,
}

/// Forgets captured requests of one tap, or all (in memory and on disk).
#[tauri::command]
#[specta::specta]
pub async fn inspect_clear(
    app: AppHandle,
    state: State<'_, AppState>,
    tap: Option<TapId>,
) -> Result<(), AppError> {
    state.inspector.clear(tap.as_ref()).await.map_err(err)?;
    changed(&app, &[EntityKind::Inspector]);
    Ok(())
}

/// Changes a tap's settings at once: capturing, the paused page, stubs, header rules,
/// network simulation, faults, stream keep-alive, the Host header, watched paths, idle
/// stop.
#[tauri::command]
#[specta::specta]
pub fn inspect_configure(
    state: State<'_, AppState>,
    tap: TapId,
    patch: TapPatch,
) -> Result<TapView, AppError> {
    state.inspector.configure(&tap, &patch).map_err(err)
}

/// Changes a tap's protection. A generated secret link key or bearer token is in the
/// answer once; passwords go in and never come back.
#[tauri::command]
#[specta::specta]
pub async fn inspect_protect(
    state: State<'_, AppState>,
    tap: TapId,
    input: ProtectionInput,
) -> Result<ProtectionResult, AppError> {
    state.inspector.protect(&tap, input).await.map_err(err)
}

/// A tap's counters and latency percentiles.
#[tauri::command]
#[specta::specta]
pub fn inspect_metrics(
    state: State<'_, AppState>,
    tap: TapId,
) -> Result<MetricsSnapshot, AppError> {
    state.inspector.metrics(&tap).map_err(err)
}

fn webhook_scope(state: &AppState, tap: &TapId) -> Result<String, AppError> {
    state
        .inspector
        .webhook_scope(tap)
        .ok_or_else(|| err(InspectError::UnknownTap))
}

fn keychain(state: &AppState) -> Result<&teitunnel_core::secrets::Secrets, AppError> {
    state
        .inspector
        .secrets()
        .ok_or_else(|| err(InspectError::UnknownTap))
}

/// Webhook senders with a signing secret saved for a tap's share or route (never the
/// secrets).
#[tauri::command]
#[specta::specta]
pub async fn inspect_webhook_secrets(
    state: State<'_, AppState>,
    tap: TapId,
) -> Result<Vec<Provider>, AppError> {
    let scope = webhook_scope(&state, &tap)?;
    Ok(secrets::webhook_providers(keychain(&state)?, &scope)
        .await
        .map_err(|e| err(e.into()))?
        .into_iter()
        .map(Provider::from)
        .collect())
}

/// Saves a webhook signing secret in the keychain for a tap's share or route.
#[tauri::command]
#[specta::specta]
pub async fn inspect_webhook_secret_set(
    state: State<'_, AppState>,
    tap: TapId,
    provider: Provider,
    secret: String,
) -> Result<(), AppError> {
    let scope = webhook_scope(&state, &tap)?;
    secrets::set_webhook_secret(
        keychain(&state)?,
        &scope,
        provider.into(),
        Secret::new(secret),
    )
    .await
    .map_err(|e| err(e.into()))
}

/// Removes a saved webhook signing secret.
#[tauri::command]
#[specta::specta]
pub async fn inspect_webhook_secret_remove(
    state: State<'_, AppState>,
    tap: TapId,
    provider: Provider,
) -> Result<(), AppError> {
    let scope = webhook_scope(&state, &tap)?;
    secrets::remove_webhook_secret(keychain(&state)?, &scope, provider.into())
        .await
        .map_err(|e| err(e.into()))
}

/// Checks a captured webhook's signature with the saved secret (`null`: not a webhook).
#[tauri::command]
#[specta::specta]
pub async fn inspect_webhook_verify(
    state: State<'_, AppState>,
    id: ExchangeId,
) -> Result<Option<WebhookCheck>, AppError> {
    Ok(state
        .inspector
        .detail(id, false)
        .await
        .map_err(err)?
        .webhook)
}

/// Routes pointed at an inspector, in every account.
#[tauri::command]
#[specta::specta]
pub async fn inspect_routes(state: State<'_, AppState>) -> Result<Vec<InspectedRoute>, AppError> {
    Ok(routes::list(&state.store, None).await?)
}

fn context<'a>(state: &'a AppState, account_id: &'a str) -> Context<'a> {
    Context {
        account: account_id,
        machine_name: &state.machine_name,
        tunnel: None,
    }
}

/// Plans inspecting a route (`on`: point it at the inspector) or ending it (back to its
/// own service), for review. `null`: nothing to change (ending an inspection whose
/// route is gone or was changed since).
#[tauri::command]
#[specta::specta]
pub async fn inspect_route_preview(
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    path: Option<String>,
    on: bool,
) -> Result<Option<InspectPlan>, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let ctx = context(&state, &account_id);
    let path = path.as_deref();
    let plan = if on {
        Some(
            routes::plan_on(
                &state.engine,
                &api,
                &state.machine,
                ctx,
                &state.inspector,
                &hostname,
                path,
            )
            .await
            .map_err(err)?,
        )
    } else {
        routes::plan_off(&state.engine, &api, &state.machine, ctx, &hostname, path)
            .await
            .map_err(err)?
    };
    Ok(plan)
}

/// Applies a reviewed [`inspect_route_preview`] plan. Inspection lasts while the app
/// runs: it's reverted on quit (and after a crash, at the next launch).
#[tauri::command]
#[specta::specta]
// A command's arguments are its IPC parameters.
#[allow(clippy::too_many_arguments)]
pub async fn inspect_route_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    path: Option<String>,
    on: bool,
    fingerprint: String,
    confirmed: bool,
    on_progress: Channel<Progress>,
) -> Result<Option<Outcome>, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let ctx = context(&state, &account_id);
    let approval = Approval {
        fingerprint: &fingerprint,
        confirmed,
    };
    let progress = move |p: Progress| {
        let _ = on_progress.send(p);
    };
    let path = path.as_deref();
    let outcome = if on {
        routes::apply_on(
            &state.engine,
            &api,
            &state.machine,
            ctx,
            &state.inspector,
            &hostname,
            path,
            approval,
            progress,
        )
        .await
        .map(Some)
    } else {
        routes::apply_off(
            &state.engine,
            &api,
            &state.machine,
            ctx,
            Some(&state.inspector),
            &hostname,
            path,
            approval,
            progress,
        )
        .await
    };
    changed(
        &app,
        &[
            EntityKind::Routes,
            EntityKind::Inspector,
            EntityKind::QuickShares,
        ],
    );
    crate::bootstrap::refresh_tray_routes(&app);
    outcome.map_err(err)
}
