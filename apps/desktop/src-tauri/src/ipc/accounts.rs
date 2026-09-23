//! Cloudflare account commands. Tokens enter through `accounts_add_token` only and are
//! never returned; cert.pem is read here, so its contents never cross IPC.

use std::path::PathBuf;
use teitunnel_core::text::msg::app as m;

use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;
use tauri_specta::Event;
use teitunnel_core::{
    Secret,
    accounts::{Account, Domain, TOKENS_PAGE, capabilities::Capabilities, token_template_url},
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
        return Err(AppError::invalid("credential", m::paste_token()));
    }
    let added = state.accounts.add_token(token).await?;
    changed(&app);
    Ok(added)
}

/// A page of Cloudflare's API token settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum TokenPage {
    /// "Create API token" with Teitunnel's permissions pre-selected.
    Create,
    /// The list of tokens, to add permissions to an existing one.
    Edit,
}

/// Opens one of Cloudflare's API token pages in the browser.
#[tauri::command]
#[specta::specta]
pub fn accounts_open_token_page(app: AppHandle, page: TokenPage) -> Result<(), AppError> {
    let url = match page {
        TokenPage::Create => token_template_url(),
        TokenPage::Edit => TOKENS_PAGE.to_owned(),
    };
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|err| AppError::internal(m::open_browser(err)))
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
    let path = cert_path(&app).ok_or_else(|| AppError::internal(m::no_home()))?;
    let pem = tokio::fs::read_to_string(&path)
        .await
        .map_err(|_| AppError::invalid("credential", m::no_cert()))?;
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
    // Stop this Mac's connector and delete its token before the account goes.
    state.machine.forget_account(&id).await;
    state.remote_logs.forget_account(&id);
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

/// Whether "Sign in with Cloudflare" (OAuth) is available in this build.
#[tauri::command]
#[specta::specta]
pub fn accounts_oauth_available(state: State<'_, AppState>) -> bool {
    state.accounts.oauth().is_some()
}

/// Signs in with Cloudflare in the browser. The authorize URL is sent on `on_url` (for
/// "Copy link" if the browser didn't open); resolves when the browser comes back.
#[tauri::command]
#[specta::specta]
pub async fn accounts_oauth_sign_in(
    app: AppHandle,
    state: State<'_, AppState>,
    on_url: tauri::ipc::Channel<String>,
) -> Result<Vec<Account>, AppError> {
    use teitunnel_core::accounts::{AccountError, oauth};
    let config = state
        .accounts
        .oauth()
        .cloned()
        .ok_or_else(|| AppError::internal(m::oauth_unavailable()))?;
    let login = oauth::start(&config).await.map_err(AccountError::from)?;
    let _ = on_url.send(login.authorize_url.clone());
    if let Err(err) = app.opener().open_url(&login.authorize_url, None::<&str>) {
        tracing::warn!(error = %err, "couldn't open the browser for sign-in");
    }
    let (cancel, cancelled) = tokio::sync::oneshot::channel();
    if let Ok(mut slot) = state.oauth_cancel.lock() {
        *slot = Some(cancel);
    }
    let code = tokio::select! {
        result = login.wait(oauth::LOGIN_TIMEOUT) => result.map_err(AccountError::from)?,
        _ = cancelled => return Err(AppError::invalid("credential", m::sign_in_cancelled())),
    };
    crate::shell::windows::focus_main(&app);
    let tokens = oauth::exchange(state.accounts.http(), &config, code)
        .await
        .map_err(AccountError::from)?;
    let added = state.accounts.add_oauth(tokens).await?;
    changed(&app);
    Ok(added)
}

/// Cancels a sign-in that's waiting for the browser.
#[tauri::command]
#[specta::specta]
pub fn accounts_oauth_cancel(state: State<'_, AppState>) {
    if let Ok(mut slot) = state.oauth_cancel.lock()
        && let Some(cancel) = slot.take()
    {
        let _ = cancel.send(());
    }
}
