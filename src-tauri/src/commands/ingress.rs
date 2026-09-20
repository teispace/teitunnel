use crate::error::AppError;
use crate::models::{IngressRule, TunnelConfiguration};
use crate::services::{CloudflareClient, KeyringStore};

#[tauri::command]
pub async fn get_tunnel_configuration(
    account_id: String,
    tunnel_id: String,
    token: Option<String>,
) -> Result<TunnelConfiguration, AppError> {
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client.get_tunnel_configuration(&account_id, &tunnel_id).await
}

#[tauri::command]
pub async fn update_tunnel_configuration(
    account_id: String,
    tunnel_id: String,
    rules: Vec<IngressRule>,
    token: Option<String>,
) -> Result<TunnelConfiguration, AppError> {
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client
        .update_tunnel_configuration(&account_id, &tunnel_id, rules)
        .await
}
