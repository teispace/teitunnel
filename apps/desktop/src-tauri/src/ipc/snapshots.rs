//! Snapshots: static copies of a site hosted on the user's own Cloudflare account (the
//! logic is `teitunnel_core::snapshot`). Publishing goes through the engine's plan →
//! apply like every other Cloudflare change.

use tauri::{AppHandle, Runtime, State, ipc::Channel};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;
use teitunnel_core::{
    engine::{Approval, Context, Outcome, PlanView, Progress},
    snapshot::{
        self, PreparedView, SnapshotChange, SnapshotVersionView, SnapshotView,
        build::{Project, detect},
        crawl::Limits,
    },
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn changed<R: Runtime>(app: &AppHandle<R>, account_id: &str) {
    for kind in [EntityKind::Snapshots, EntityKind::Routes] {
        let _ = EntityChanged {
            kind,
            id: Some(account_id.to_owned()),
        }
        .emit(app);
    }
}

fn context<'a>(state: &'a AppState, account_id: &'a str) -> Context<'a> {
    Context {
        account: account_id,
        machine_name: &state.machine_name,
        tunnel: None,
    }
}

/// Snapshots in every account, by name.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_list(state: State<'_, AppState>) -> Result<Vec<SnapshotView>, AppError> {
    Ok(snapshot::list(&state.engine, None).await?)
}

/// A Snapshot's kept versions, newest first.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_versions(
    state: State<'_, AppState>,
    snapshot_id: String,
) -> Result<Vec<SnapshotVersionView>, AppError> {
    Ok(snapshot::versions(&state.engine, &snapshot_id).await?)
}

/// Asks for a folder with the system's open panel. `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_choose_folder(app: AppHandle) -> Result<Option<String>, AppError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder.and_then(|f| f.into_path().ok()));
    });
    Ok(rx
        .await
        .ok()
        .flatten()
        .map(|path| path.display().to_string()))
}

/// Recognises the web project in a folder: framework, build command, output folder.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_detect_project(dir: String) -> Result<Project, AppError> {
    let found = tokio::task::spawn_blocking(move || detect(std::path::Path::new(&dir)))
        .await
        .map_err(|e| AppError::internal(teitunnel_core::text::msg::raw(e.to_string())))?;
    Ok(found?)
}

/// Collects a folder's files for publishing (nothing is sent anywhere).
#[tauri::command]
#[specta::specta]
pub async fn snapshots_prepare_folder(
    state: State<'_, AppState>,
    path: String,
) -> Result<PreparedView, AppError> {
    Ok(state.snapshots.folder(std::path::Path::new(&path)).await?)
}

/// Builds a project with its package manager (after the user confirmed the command),
/// streaming its output, then collects the files it produced.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_prepare_build(
    state: State<'_, AppState>,
    dir: String,
    on_output: Channel<String>,
) -> Result<PreparedView, AppError> {
    let dir2 = dir.clone();
    let project = tokio::task::spawn_blocking(move || detect(std::path::Path::new(&dir2)))
        .await
        .map_err(|e| AppError::internal(teitunnel_core::text::msg::raw(e.to_string())))??;
    Ok(state
        .snapshots
        .build(&project, |line| {
            let _ = on_output.send(line.to_owned());
        })
        .await?)
}

/// Captures a site running on this computer (e.g. a dev server) by crawling it.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_prepare_crawl(
    state: State<'_, AppState>,
    url: String,
) -> Result<PreparedView, AppError> {
    let dest = snapshot::capture_dir(&state.snapshot_dir);
    Ok(state
        .snapshots
        .crawl(&url, &dest, Limits::default())
        .await?)
}

/// Plans a Snapshot change for review. Nothing is changed.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_preview(
    state: State<'_, AppState>,
    account_id: String,
    change: SnapshotChange,
) -> Result<PlanView, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(snapshot::preview(
        &state.engine,
        &api,
        &state.snapshots,
        context(&state, &account_id),
        &change,
    )
    .await?)
}

/// Applies a reviewed Snapshot change; step progress (and upload progress) streams on
/// `on_progress`.
#[tauri::command]
#[specta::specta]
pub async fn snapshots_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    change: SnapshotChange,
    fingerprint: String,
    confirmed: bool,
    on_progress: Channel<Progress>,
) -> Result<Outcome, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let outcome = snapshot::apply(
        &state.engine,
        &api,
        &state.machine,
        &state.snapshots,
        context(&state, &account_id),
        "app",
        &change,
        Approval {
            fingerprint: &fingerprint,
            confirmed,
        },
        |p| {
            let _ = on_progress.send(p);
        },
    )
    .await;
    changed(&app, &account_id);
    Ok(outcome?)
}
