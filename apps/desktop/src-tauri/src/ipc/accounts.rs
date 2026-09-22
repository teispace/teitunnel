//! Cloudflare account commands. Tokens enter through `accounts_add_token` only and are
//! never returned; cert.pem is read here, so its contents never cross IPC.

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;
use teitunnel_core::{
    Secret,
    accounts::{Account, Domain, capabilities::Capabilities, token_template_url},
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn changed(app: &AppHandle) {
    let _ = EntityChanged {
        kind: EntityKind::Accounts,
        id: None,
    }
    .emit(app);
}

fn cert_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .home_dir()
        .ok()
        .map(|home| home.join(".cloudflared").join("cert.pem"))
}

/// Connected accounts.
#[tauri::command]
#[specta::specta]
pub async fn accounts_list(state: State<'_, AppState>) -> Result<Vec<Account>, AppError> {
    Ok(state.accounts.list().await?)
}

/// Verifies an API token and connects every account it reaches. The token is stored in
/// the keychain and never sent back.
#[tauri::command]
#[specta::specta]
pub async fn accounts_add_token(
    app: AppHandle,
    state: State<'_, AppState>,
    token: String,
) -> Result<Vec<Account>, AppError> {
    let token = Secret::new(token);
    if token.expose().trim().is_empty() {
        return Err(AppError::invalid(
            "credential",
            "Paste the API token you created.",
        ));
    }
    let added = state.accounts.add_token(token).await?;
    changed(&app);
    Ok(added)
}

/// Opens Cloudflare's "Create API token" page with Teitunnel's permissions pre-selected.
#[tauri::command]
#[specta::specta]
pub fn accounts_open_token_page(app: AppHandle) -> Result<(), AppError> {
    app.opener()
        .open_url(token_template_url(), None::<&str>)
        .map_err(|err| AppError::internal(format!("Couldn't open the browser: {err}")))
}

/// Whether `~/.cloudflared/cert.pem` (from `cloudflared tunnel login`) exists.
#[tauri::command]
#[specta::specta]
pub fn accounts_detect_cert(app: AppHandle) -> bool {
    cert_path(&app).is_some_and(|path| path.is_file())
}

/// Imports the login from `~/.cloudflared/cert.pem` (the file is only read).
#[tauri::command]
#[specta::specta]
pub async fn accounts_import_cert(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Account, AppError> {
    let path =
        cert_path(&app).ok_or_else(|| AppError::internal("Couldn't find your home folder."))?;
    let pem = tokio::fs::read_to_string(&path).await.map_err(|_| {
        AppError::invalid(
            "credential",
            "No cert.pem found. Run `cloudflared tunnel login` first, or use an API token.",
        )
    })?;
    let account = state.accounts.import_cert(&pem).await?;
    changed(&app);
    Ok(account)
}

/// Disconnects an account and deletes its credentials from the keychain.
#[tauri::command]
#[specta::specta]
pub async fn accounts_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    state.accounts.remove(&id).await?;
    changed(&app);
    Ok(())
}

/// What the account's credential can do.
#[tauri::command]
#[specta::specta]
pub async fn accounts_capabilities(
    state: State<'_, AppState>,
    id: String,
) -> Result<Capabilities, AppError> {
    Ok(state.accounts.capabilities(&id).await?)
}

/// Domains in an account.
#[tauri::command]
#[specta::specta]
pub async fn domains_list(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<Domain>, AppError> {
    Ok(state.accounts.domains(&account_id).await?)
}
