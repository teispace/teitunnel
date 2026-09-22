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

/// Called by a webview once its first frame is painted; shows its window.
#[tauri::command]
#[specta::specta]
pub fn app_ready(window: tauri::WebviewWindow) {
    shell::windows::show_when_ready(&window);
}

/// The system accent colour as `#rrggbb`, or `null` to keep the stylesheet default.
#[tauri::command]
#[specta::specta]
pub fn app_accent_color() -> Option<String> {
    shell::accent::accent_color()
}

/// Records an uncaught webview error in the app log (redacted like every other line).
#[tauri::command]
#[specta::specta]
pub fn app_report_error(message: String, stack: Option<String>) {
    tracing::error!(target: "webview", %message, stack = stack.as_deref().unwrap_or(""), "uncaught error");
}

/// Opens the Settings window (same as ⌘,).
#[tauri::command]
#[specta::specta]
pub fn app_open_settings(app: AppHandle) -> Result<(), AppError> {
    Ok(shell::windows::open_settings(&app)?)
}
