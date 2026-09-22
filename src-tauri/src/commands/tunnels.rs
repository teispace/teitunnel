use tauri::State;
use crate::error::AppError;
use crate::models::{CloudflareTunnel, TunnelProcessState};
use crate::services::{CloudflareClient, ProcessManager};
use super::resolve_token;

#[tauri::command]
pub async fn list_tunnels(
    account_id: String,
    token: Option<String>,
) -> Result<Vec<CloudflareTunnel>, AppError> {
    let tok = resolve_token(token)?;
    let client = CloudflareClient::new(tok);
    client.list_tunnels(&account_id).await
}

#[tauri::command]
pub async fn create_tunnel(
    account_id: String,
    name: String,
    token: Option<String>,
) -> Result<CloudflareTunnel, AppError> {
    let tok = resolve_token(token)?;
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
    let tok = resolve_token(token)?;
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

    let mut api_deleted = false;

    // 2. Try deleting via Cloudflare REST API if account_id is present
    if !account_id.trim().is_empty() {
        if let Ok(tok) = resolve_token(token) {
            let client = CloudflareClient::new(tok);
            if client.delete_tunnel(&account_id, &tunnel_id).await.is_ok() {
                api_deleted = true;
            }
        }
    }

    // 3. Clean up CLI cert tunnel and local credentials file
    let cert_res = process_manager.delete_cert_tunnel(&tunnel_id).await;

    if !api_deleted && cert_res.is_err() {
        return cert_res;
    }

    Ok(())
}

#[tauri::command]
pub async fn get_active_processes(
    process_manager: State<'_, ProcessManager>,
) -> Result<Vec<TunnelProcessState>, AppError> {
    Ok(process_manager.get_active_processes().await)
}

#[tauri::command]
pub async fn start_tunnel_by_token(
    tunnel_id: String,
    token: String,
    process_manager: State<'_, ProcessManager>,
) -> Result<TunnelProcessState, AppError> {
    process_manager.start_remote_tunnel(&tunnel_id, &token).await
}

#[tauri::command]
pub async fn start_named_tunnel(
    tunnel_name: String,
    process_manager: State<'_, ProcessManager>,
) -> Result<TunnelProcessState, AppError> {
    process_manager.start_named_tunnel(&tunnel_name).await
}

#[tauri::command]
pub async fn list_cert_tunnels(
    process_manager: State<'_, ProcessManager>,
) -> Result<Vec<CloudflareTunnel>, AppError> {
    process_manager.list_cert_tunnels().await
}

#[tauri::command]
pub async fn create_cert_tunnel(
    name: String,
    process_manager: State<'_, ProcessManager>,
) -> Result<CloudflareTunnel, AppError> {
    process_manager.create_cert_tunnel(&name).await
}

#[tauri::command]
pub async fn delete_cert_tunnel(
    tunnel_id: String,
    process_manager: State<'_, ProcessManager>,
) -> Result<(), AppError> {
    process_manager.delete_cert_tunnel(&tunnel_id).await
}
