use tauri::{AppHandle, Emitter};
use crate::error::AppError;
use crate::models::{BinaryStatus, DownloadProgress};
use crate::services::BinaryManager;

#[tauri::command]
pub fn check_binary_status() -> BinaryStatus {
    BinaryManager::check_status()
}

#[tauri::command]
pub async fn download_managed_binary(app: AppHandle) -> Result<BinaryStatus, AppError> {
    let app_handle = app.clone();
    let status = BinaryManager::download_managed_binary(move |progress: DownloadProgress| {
        let _ = app_handle.emit("binary-download-progress", progress);
    })
    .await?;

    let _ = app.emit("binary-status-updated", status.clone());
    Ok(status)
}
