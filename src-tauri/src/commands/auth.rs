use crate::error::AppError;
use crate::models::{CloudflareAccount, CloudflareZone};
use crate::services::{CloudflareClient, KeyringStore};

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
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client.list_accounts().await
}

#[tauri::command]
pub async fn list_zones(
    account_id: String,
    token: Option<String>,
) -> Result<Vec<CloudflareZone>, AppError> {
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client.list_zones(&account_id).await
}
