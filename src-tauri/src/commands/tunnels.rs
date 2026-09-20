use tauri::State;
use crate::error::AppError;
use crate::models::{CloudflareTunnel, TunnelProcessState};
use crate::services::{CloudflareClient, KeyringStore, ProcessManager};

#[tauri::command]
pub async fn list_tunnels(
    account_id: String,
    token: Option<String>,
) -> Result<Vec<CloudflareTunnel>, AppError> {
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client.list_tunnels(&account_id).await
}

#[tauri::command]
pub async fn create_tunnel(
    account_id: String,
    name: String,
    token: Option<String>,
) -> Result<CloudflareTunnel, AppError> {
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client.create_tunnel(&account_id, &name).await
}

#[tauri::command]
pub async fn start_tunnel(
    account_id: String,
    tunnel_id: String,
    process_manager: State<'_, ProcessManager>,
    token: Option<String>,
) -> Result<TunnelProcessState, AppError> {
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    let tunnel_token = client.get_tunnel_token(&account_id, &tunnel_id).await?;
    process_manager
        .start_remote_tunnel(&tunnel_id, &tunnel_token)
        .await
}

#[tauri::command]
pub async fn stop_tunnel(
    tunnel_id: String,
    process_manager: State<'_, ProcessManager>,
) -> Result<(), AppError> {
    process_manager.stop_tunnel(&tunnel_id).await
}

#[tauri::command]
pub async fn delete_tunnel(
    account_id: String,
    tunnel_id: String,
    process_manager: State<'_, ProcessManager>,
    token: Option<String>,
) -> Result<(), AppError> {
    // 1. Stop if running
    let _ = process_manager.stop_tunnel(&tunnel_id).await;

    // 2. Delete on Cloudflare
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client.delete_tunnel(&account_id, &tunnel_id).await
}

#[tauri::command]
pub async fn get_active_processes(
    process_manager: State<'_, ProcessManager>,
) -> Result<Vec<TunnelProcessState>, AppError> {
    Ok(process_manager.get_active_processes().await)
}
