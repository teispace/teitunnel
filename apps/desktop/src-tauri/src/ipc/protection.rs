//! Edge protection and service tokens for a hostname (the logic is
//! `teitunnel_core::protection`). Changes go through the engine's plan → apply.
//!
//! A new token's secret never crosses IPC: it stays in Rust (`IssuedSecrets`) for a few
//! minutes, and "Copy" puts it on the clipboard from here.

use tauri::{AppHandle, State, ipc::Channel};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tauri_specta::Event;
use teitunnel_core::{
    engine::{Approval, Context, Outcome, PlanView, Progress},
    protection::{
        self, IssuedTokenView, ProtectionChange, ProtectionView, SecretCopy, ServiceTokenView,
    },
    text::msg,
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

/// How applying a protection change ended, with any new token (never its secret).
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct ProtectionOutcome {
    /// How applying ended.
    pub outcome: Outcome,
    /// Tokens created or rotated; copy their secret with `protection_copy_secret`.
    pub issued: Vec<IssuedTokenView>,
}

/// What Teitunnel enforces for a hostname at the edge, with the zone's quotas.
#[tauri::command]
#[specta::specta]
pub async fn protection_get(
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
) -> Result<ProtectionView, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(protection::view(&state.engine, &api, context(&state, &account_id), &hostname).await?)
}

/// Teitunnel's service tokens for a hostname, with their expiry.
#[tauri::command]
#[specta::specta]
pub async fn protection_tokens(
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
) -> Result<Vec<ServiceTokenView>, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(protection::tokens(&state.engine, &api, &account_id, &hostname).await?)
}

/// Plans a protection change for review. Nothing is changed.
#[tauri::command]
#[specta::specta]
pub async fn protection_preview(
    state: State<'_, AppState>,
    account_id: String,
    change: ProtectionChange,
) -> Result<PlanView, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(protection::preview(&state.engine, &api, context(&state, &account_id), &change).await?)
}

/// Applies a reviewed protection change; step progress streams on `on_progress`.
#[tauri::command]
#[specta::specta]
pub async fn protection_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    change: ProtectionChange,
    fingerprint: String,
    confirmed: bool,
    on_progress: Channel<Progress>,
) -> Result<ProtectionOutcome, AppError> {
    let api = state.accounts.client(&account_id).await?;
    let result = protection::apply(
        &state.engine,
        &api,
        &state.machine,
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
    let _ = EntityChanged {
        kind: EntityKind::Routes,
        id: Some(account_id),
    }
    .emit(&app);
    let (outcome, issued) = result?;
    Ok(ProtectionOutcome {
        outcome,
        issued: state.issued_secrets.keep(issued),
    })
}

/// Copies a new token's secret (or both headers) to the clipboard, from Rust.
#[tauri::command]
#[specta::specta]
pub async fn protection_copy_secret(
    app: AppHandle,
    state: State<'_, AppState>,
    token_id: String,
    what: SecretCopy,
) -> Result<(), AppError> {
    let text = state
        .issued_secrets
        .copy_text(&token_id, what)
        .ok_or_else(|| AppError::from(msg::protection::error::secret_gone()))?;
    app.clipboard()
        .write_text(text.expose().clone())
        .map_err(|e| AppError::internal(msg::raw(e.to_string())))?;
    Ok(())
}

/// Forgets a new token's secret (its sheet was closed).
#[tauri::command]
#[specta::specta]
pub async fn protection_forget_secret(
    state: State<'_, AppState>,
    token_id: String,
) -> Result<(), AppError> {
    state.issued_secrets.forget(&token_id);
    Ok(())
}
