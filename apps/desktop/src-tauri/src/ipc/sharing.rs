//! Sharing extras (M12-06): pausing, schedules, names from the project and folders on
//! your domain. The logic is `teitunnel_core::{pause, schedule, share_names,
//! folder_share}`.

use std::{path::Path, time::Duration};

use tauri::{AppHandle, Runtime, State};
use tauri_specta::Event;
use teitunnel_core::{
    domain_shares,
    engine::{AccessRule, Context, Outcome},
    folder_share::FolderShare,
    pause,
    project::template::{Vars, label},
    schedule::{self, RouteSchedule, Schedule},
    share_names::{self, NameSuggestion},
    text::UserText as _,
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn changed<R: Runtime>(app: &AppHandle<R>, account_id: &str) {
    for kind in [EntityKind::QuickShares, EntityKind::Routes] {
        let _ = EntityChanged {
            kind,
            id: Some(account_id.to_owned()),
        }
        .emit(app);
    }
}

fn invalid(field: &str, text: teitunnel_core::text::Text) -> AppError {
    AppError::invalid(field, text)
}

/// Pauses (`paused`) or resumes a share on your domain or a route: the address stays,
/// and visitors get a "paused" page from this Mac's inspector until it's resumed.
#[tauri::command]
#[specta::specta]
pub async fn sharing_set_paused(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    paused: bool,
) -> Result<(), AppError> {
    let here = pause::Here {
        accounts: &state.accounts,
        engine: &state.engine,
        connectors: &state.machine,
        machine_name: &state.machine_name,
        inspector: &state.inspector,
        enforcer: &state.pauses,
    };
    let result = pause::set_paused(here, &account_id, &hostname, paused).await;
    changed(&app, &account_id);
    result.map_err(|text| invalid("hostname", text))
}

/// Schedules of shares and routes, with whether each is on now and when it changes.
#[tauri::command]
#[specta::specta]
pub async fn sharing_schedules(state: State<'_, AppState>) -> Result<Vec<RouteSchedule>, AppError> {
    Ok(schedule::list(&state.store, None).await?)
}

/// Sets (or, with `null`, removes) when a share on your domain or a route is on. The
/// app applies it within 30 seconds.
#[tauri::command]
#[specta::specta]
pub async fn sharing_set_schedule(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    schedule: Option<Schedule>,
) -> Result<(), AppError> {
    let schedule = schedule
        .map(Schedule::validated)
        .transpose()
        .map_err(|e| invalid("schedule", e.text()))?;
    schedule::set(&state.store, &account_id, &hostname, schedule.as_ref()).await?;
    crate::bootstrap::apply_schedules_soon(&app);
    changed(&app, &account_id);
    Ok(())
}

/// Names to offer for a share on `domain` of a service in `folder` (or known only by
/// its `project` name): the one used there last, `{project}`, `{branch}-{project}`…
#[tauri::command]
#[specta::specta]
pub async fn sharing_name_suggestions(
    state: State<'_, AppState>,
    domain: String,
    folder: Option<String>,
    project: Option<String>,
) -> Result<Vec<NameSuggestion>, AppError> {
    let (vars, remembered) = match folder.as_deref().filter(|f| Path::new(f).is_dir()) {
        Some(folder) => (
            share_names::vars_for(Path::new(folder)),
            share_names::remembered(&state.store, Path::new(folder)).await?,
        ),
        None => {
            let mut vars = Vars::for_dir(Path::new("/"), project.as_deref().unwrap_or_default());
            vars.branch = None;
            vars.project = project.as_deref().map(label).filter(|p| !p.is_empty());
            (vars, None)
        }
    };
    Ok(share_names::suggestions(
        &domain,
        &vars,
        remembered.as_deref(),
    ))
}

/// Fills in a hostname's `{project}`, `{branch}` and `{user}` for a share of a service
/// in `folder` (to show what it becomes).
#[tauri::command]
#[specta::specta]
pub fn sharing_expand_name(hostname: String, folder: Option<String>) -> Result<String, AppError> {
    let vars = match folder.as_deref().filter(|f| Path::new(f).is_dir()) {
        Some(folder) => share_names::vars_for(Path::new(folder)),
        None => Vars::for_dir(Path::new("/"), ""),
    };
    share_names::expand_with(&hostname, &vars).map_err(|e| invalid("hostname", e.text()))
}

/// Asks the person to choose a folder to share (a native panel). `null`: cancelled.
#[tauri::command]
#[specta::specta]
pub async fn sharing_choose_folder(app: AppHandle) -> Result<Option<String>, AppError> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder.and_then(|f| f.into_path().ok()));
    });
    Ok(rx.await.ok().flatten().map(|p| p.display().to_string()))
}

/// Checks a folder chosen or dropped for sharing (it must exist and not be the whole
/// disk or the home folder).
#[tauri::command]
#[specta::specta]
pub fn sharing_folder(
    path: String,
    listing: Option<bool>,
    spa: bool,
) -> Result<FolderShare, AppError> {
    FolderShare::resolve(&path, listing, spa).map_err(|e| invalid("folder", e.text()))
}

/// Shares a folder at a hostname on one of the account's domains: this Mac's inspector
/// serves its files (never secrets or tooling) until it's stopped, `stop_after_minutes`
/// pass, or Teitunnel quits.
#[tauri::command]
#[specta::specta]
pub async fn sharing_start_folder_on_domain(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    folder: FolderShare,
    stop_after_minutes: Option<u32>,
    access: Option<AccessRule>,
) -> Result<Outcome, AppError> {
    let folder = FolderShare::resolve(&folder.path, folder.listing, folder.spa)
        .map_err(|e| invalid("folder", e.text()))?;
    let api = state.accounts.client(&account_id).await?;
    let expires_at = stop_after_minutes.map(|minutes| {
        domain_shares::now_ms()
            + u64::try_from(Duration::from_secs(u64::from(minutes) * 60).as_millis())
                .unwrap_or(u64::MAX)
    });
    let ctx = Context {
        account: &account_id,
        machine_name: &state.machine_name,
        tunnel: None,
    };
    let outcome = domain_shares::start_folder(
        &state.engine,
        &api,
        &state.machine,
        ctx,
        &state.inspector,
        &hostname,
        &folder,
        access,
        expires_at,
    )
    .await;
    changed(&app, &account_id);
    crate::bootstrap::refresh_tray_routes(&app);
    Ok(outcome.map_err(teitunnel_core::Error::from)?)
}
