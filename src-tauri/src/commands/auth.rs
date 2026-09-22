use tauri::State;
use crate::error::AppError;
use crate::models::{CertStatus, CloudflareAccount, CloudflareZone};
use crate::services::{BinaryManager, CloudflareClient, KeyringStore, ProcessManager};

#[tauri::command]
pub async fn verify_and_save_token(token: String) -> Result<bool, AppError> {
    let client = CloudflareClient::new(token.clone());
    let valid = client.verify_token().await?;
    if valid {
        KeyringStore::save_token(&token)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[tauri::command]
pub fn get_saved_token() -> Result<Option<String>, AppError> {
    KeyringStore::get_token()
}

#[tauri::command]
pub fn delete_saved_token() -> Result<(), AppError> {
    KeyringStore::delete_token()
}

#[tauri::command]
pub async fn list_accounts(token: Option<String>) -> Result<Vec<CloudflareAccount>, AppError> {
    let tok = super::resolve_token(token)?;
    let client = CloudflareClient::new(tok);
    client.list_accounts().await
}

#[tauri::command]
pub async fn list_zones(
    account_id: String,
    token: Option<String>,
) -> Result<Vec<CloudflareZone>, AppError> {
    let tok = super::resolve_token(token)?;
    let client = CloudflareClient::new(tok);
    client.list_zones(&account_id).await
}

#[tauri::command]
pub fn check_cert_status() -> Result<CertStatus, AppError> {
    Ok(BinaryManager::check_cert_status())
}

#[tauri::command]
pub async fn start_browser_login(
    process_manager: State<'_, ProcessManager>,
) -> Result<(), AppError> {
    process_manager.start_browser_login().await
}

#[tauri::command]
pub async fn cancel_browser_login(
    process_manager: State<'_, ProcessManager>,
) -> Result<(), AppError> {
    process_manager.cancel_browser_login().await
}

#[tauri::command]
pub fn delete_cert() -> Result<(), AppError> {
    BinaryManager::delete_cert()
}
