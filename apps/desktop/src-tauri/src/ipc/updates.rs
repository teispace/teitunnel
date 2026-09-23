//! App update commands (M6-02). The work is in `shell::updates`.

use tauri::{AppHandle, State};
use teitunnel_core::updates::UpdateStatus;

use crate::{error::AppError, shell::updates::Updates};

/// Where updates stand.
#[tauri::command]
#[specta::specta]
pub async fn updates_status(
    app: AppHandle,
    updates: State<'_, Updates>,
) -> Result<UpdateStatus, AppError> {
    Ok(updates.status(&app).await)
}

/// Checks for an update now (and downloads it), even with automatic checks off.
#[tauri::command]
#[specta::specta]
pub async fn updates_check(
    app: AppHandle,
    updates: State<'_, Updates>,
) -> Result<UpdateStatus, AppError> {
    updates.check(&app, true).await;
    Ok(updates.status(&app).await)
}

/// Quits, installs the downloaded update and starts the new version.
#[tauri::command]
#[specta::specta]
pub fn updates_restart(app: AppHandle, updates: State<'_, Updates>) {
    updates.restart(&app);
}
