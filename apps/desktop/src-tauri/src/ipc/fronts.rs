//! The offline page and webhook inboxes of routes (the logic is `teitunnel_core::fronts`
//! and `teitunnel_core::inbox`). Changes go through the engine's plan → apply. A
//! verifying inbox's signing secret is read from the keychain in Rust and never crosses
//! IPC.

use tauri::{AppHandle, State, ipc::Channel};
use tauri_specta::Event;
use teitunnel_core::{
    engine::{Approval, Context, Outcome, PlanView, Progress, front::InboxVerify},
    fronts::{self, FrontChange, FrontView},
    inbox::{DrainReport, InboxItem},
    text::{UserText as _, msg},
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn context<'a>(state: &'a AppState, account_id: &'a str) -> Context<'a> {
    Context {
        account: account_id,
        machine_name: &state.machine_name,
        tunnel: None,
    }
}

/// Teitunnel's offline pages and webhook inboxes (from this computer's records).
#[tauri::command]
#[specta::specta]
pub async fn fronts_list(
    state: State<'_, AppState>,
    account_id: Option<String>,
) -> Result<Vec<FrontView>, AppError> {
    Ok(fronts::list(&state.engine, account_id.as_deref()).await?)
}

/// Which senders have a signing secret saved for `hostname`'s inboxes (never the
/// secrets themselves).
#[tauri::command]
#[specta::specta]
pub async fn fronts_inbox_secrets(
    state: State<'_, AppState>,
    hostname: String,
) -> Result<Vec<InboxVerify>, AppError> {
    fronts::inbox_secrets(&state.secrets, &hostname)
        .await
        .map_err(|e| AppError::internal(e.text()))
}

/// Saves the signing secret `hostname`'s verifying inboxes check `verify`'s webhooks
/// with, in the keychain (it goes to Cloudflare only as a Worker secret, never back
/// across IPC).
#[tauri::command]
#[specta::specta]
pub async fn fronts_inbox_secret_set(
    state: State<'_, AppState>,
    hostname: String,
    verify: InboxVerify,
    secret: String,
) -> Result<(), AppError> {
    let secret = secret.trim().to_owned();
    if secret.is_empty() {
        return Err(AppError::invalid("secret", msg::fronts::secret_empty()));
    }
    fronts::set_inbox_secret(
        &state.secrets,
        &hostname,
        verify,
        teitunnel_core::Secret::new(secret),
    )
    .await
    .map_err(|e| AppError::internal(e.text()))
}

/// Plans an offline page or inbox change for review. Nothing is changed.
#[tauri::command]
#[specta::specta]
pub async fn fronts_preview(
    state: State<'_, AppState>,
    account_id: String,
    change: FrontChange,
) -> Result<PlanView, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(fronts::preview(
        &state.engine,
        &api,
        Some(&state.secrets),
        context(&state, &account_id),
        &change,
    )
    .await?)
}

/// The change that puts things back as they are now (for Undo after applying).
#[tauri::command]
#[specta::specta]
pub async fn fronts_undo_change(
    state: State<'_, AppState>,
    account_id: String,
    change: FrontChange,
) -> Result<FrontChange, AppError> {
    Ok(fronts::undo_of(&state.engine, &account_id, &change).await?)
}

/// Applies a reviewed change; step progress streams on `on_progress`.
#[tauri::command]
#[specta::specta]
pub async fn fronts_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    change: FrontChange,
    fingerprint: String,
    confirmed: bool,
    on_progress: Channel<Progress>,
) -> Result<Outcome, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let result = fronts::apply(
        &state.engine,
        &api,
        &state.machine,
        Some(&state.secrets),
        context(&state, &account_id),
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
    for kind in [EntityKind::Fronts, EntityKind::Routes] {
        let _ = EntityChanged {
            kind,
            id: Some(account_id.clone()),
        }
        .emit(&app);
    }
    Ok(result?)
}

/// A webhook inbox's recent webhooks: when each arrived and when it was delivered.
#[tauri::command]
#[specta::specta]
pub async fn inbox_items(
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    path: String,
) -> Result<Vec<InboxItem>, AppError> {
    let inbox = fronts::list(&state.engine, Some(&account_id))
        .await?
        .into_iter()
        .find(|f| f.hostname.eq_ignore_ascii_case(&hostname) && f.path == path)
        .ok_or_else(|| AppError::from(msg::front::error::none(format!("{hostname}{path}"))))?;
    let Some(database) = state.engine.local().cloud_database(&account_id).await? else {
        return Ok(Vec::new());
    };
    let api = state.accounts.client(&account_id).await?;
    Ok(
        teitunnel_core::inbox::items(&api, &account_id, &database, &inbox.script, 100)
            .await
            .map_err(teitunnel_core::Error::from)?,
    )
}

/// Delivers waiting webhooks now (the app also does every 30 seconds).
#[tauri::command]
#[specta::specta]
pub async fn inbox_deliver(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<DrainReport>, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let http =
        teitunnel_core::inbox::client().map_err(|e| AppError::internal(msg::raw(e.to_string())))?;
    let reports =
        teitunnel_core::inbox::drain_account(&state.engine, &api, &http, &account_id).await?;
    let _ = EntityChanged {
        kind: EntityKind::Fronts,
        id: Some(account_id),
    }
    .emit(&app);
    Ok(reports)
}
