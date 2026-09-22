//! App-level commands.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};

use crate::{error::AppError, shell};

/// Static facts about the running app.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// App version (semver).
    pub version: String,
    /// Operating system: `macos`, `linux` or `windows`.
    pub platform: String,
    /// CPU architecture, e.g. `aarch64`.
    pub arch: String,
    /// Directory holding the database, logs and managed binaries.
    pub data_dir: String,
}

/// Returns the app version, platform and data directory.
#[tauri::command]
#[specta::specta]
pub fn app_info(app: AppHandle) -> Result<AppInfo, AppError> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        platform: std::env::consts::OS.to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        data_dir: app.path().app_data_dir()?.display().to_string(),
    })
}

/// Called by the webview once the first frame is painted; shows the main window.
#[tauri::command]
#[specta::specta]
pub fn app_ready(app: AppHandle) {
    shell::window::show_main(&app);
}

/// The system accent colour as `#rrggbb`, or `null` to keep the stylesheet default.
#[tauri::command]
#[specta::specta]
pub fn app_accent_color() -> Option<String> {
    shell::accent::accent_color()
}
