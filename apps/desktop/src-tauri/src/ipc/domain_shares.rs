//! Share on your own domain: temporary routes that go away when stopped, when they
//! expire, or when Teitunnel quits (the logic is `teitunnel_core::domain_shares`).

use std::time::Duration;

use tauri::{AppHandle, Runtime, State};
use tauri_specta::Event;
use teitunnel_core::{
    domain_shares::{self, APP_OWNER, DomainShare, ShareRequest},
    engine::{AccessRule, Context, Outcome},
    quick_share::HostHeaderChoice,
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
    crate::bootstrap::refresh_tray_routes(app);
}

fn context<'a>(state: &'a AppState, account_id: &'a str) -> Context<'a> {
    Context {
        account: account_id,
        machine_name: &state.machine_name,
        tunnel: None,
    }
}

/// Shares on your domains, in every account, oldest first.
#[tauri::command]
#[specta::specta]
pub async fn domain_shares_list(state: State<'_, AppState>) -> Result<Vec<DomainShare>, AppError> {
    Ok(state.engine.local().shares(None).await?)
}

/// Shares a local service at a hostname on one of the account's domains, through this
/// Mac's tunnel, until it's stopped, `stop_after_minutes` pass, or Teitunnel quits. Never
/// replaces a DNS record Teitunnel didn't create. `{project}`, `{branch}` and `{user}` in
/// the hostname are filled in from `folder` (the service's project folder), and the
/// name is remembered for it.
#[tauri::command]
#[specta::specta]
#[allow(clippy::too_many_arguments)] // one per IPC argument
pub async fn domain_shares_start(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    origin: String,
    stop_after_minutes: Option<u32>,
    access: Option<AccessRule>,
    host_header: HostHeaderChoice,
    folder: Option<String>,
) -> Result<Outcome, AppError> {
    use teitunnel_core::{share_names, text::UserText as _};
    let folder = folder.map(std::path::PathBuf::from).filter(|f| f.is_dir());
    let typed = hostname;
    let hostname = match &folder {
        Some(folder) => share_names::expand(&typed, folder),
        None => share_names::expand_with(
            &typed,
            &teitunnel_core::project::template::Vars::for_dir(std::path::Path::new("/"), ""),
        ),
    }
    .map_err(|e| AppError::invalid("hostname", e.text()))?;
    let host_header = host_header.resolve(&origin).await?.map(|h| h.value);
    let api = state.accounts.client(&account_id).await?;
    let expires_at = stop_after_minutes.map(|minutes| {
        domain_shares::now_ms() + Duration::from_secs(u64::from(minutes) * 60).as_millis() as u64
    });
    let outcome = domain_shares::start(
        &state.engine,
        &api,
        &state.machine,
        context(&state, &account_id),
        ShareRequest {
            hostname: &hostname,
            origin: &origin,
            access,
            expires_at,
            owner: APP_OWNER,
            host_header,
            source: None,
            folder: false,
        },
    )
    .await;
    if matches!(outcome, Ok(Outcome::Applied { .. }))
        && let Some(folder) = &folder
        && let Err(err) = share_names::remember(&state.store, folder, &typed).await
    {
        tracing::warn!(%err, "couldn't remember the share's name for its folder");
    }
    changed(&app, &account_id);
    Ok(outcome?)
}

/// Stops a share on your domain: its route, DNS record and login are removed.
#[tauri::command]
#[specta::specta]
pub async fn domain_shares_stop(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
) -> Result<(), AppError> {
    let api = state.accounts.client(&account_id).await?;
    let stopped = domain_shares::stop(
        &state.engine,
        &api,
        &state.machine,
        context(&state, &account_id),
        &hostname,
    )
    .await;
    if stopped.is_ok() {
        // A folder (or an inspected share) had a tap in this app.
        domain_shares::release_tap(&state.inspector, &account_id, &hostname).await;
    }
    changed(&app, &account_id);
    Ok(stopped?)
}
