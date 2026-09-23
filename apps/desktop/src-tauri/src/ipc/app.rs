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

/// Quits after the user confirmed. With `keep_running`, this Mac's connectors switch to
/// Always-on first (so routes stay up); if that fails, the app stays open.
#[tauri::command]
#[specta::specta]
pub async fn app_quit(
    app: tauri::AppHandle,
    state: tauri::State<'_, crate::state::AppState>,
    keep_running: bool,
) -> Result<(), crate::error::AppError> {
    if keep_running {
        for account in state.accounts.list().await? {
            let Ok(Some(tunnel)) = state.engine.local().machine_tunnel(&account.id).await else {
                continue;
            };
            if state.machine.is_always_on(&tunnel.tunnel_id) {
                continue;
            }
            let api = state.accounts.client(&account.id).await?;
            state
                .machine
                .set_always_on(&api, &account.id, true)
                .await
                .map_err(crate::error::AppError::internal)?;
        }
    }
    state
        .quit_confirmed
        .store(true, std::sync::atomic::Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

/// Whether Teitunnel opens at login (hidden, in the menu bar).
#[tauri::command]
#[specta::specta]
pub fn app_open_at_login(app: AppHandle) -> bool {
    use tauri_plugin_autostart::ManagerExt;
    app.autolaunch().is_enabled().unwrap_or(false)
}

/// Turns opening at login on or off.
#[tauri::command]
#[specta::specta]
pub fn app_set_open_at_login(app: AppHandle, enabled: bool) -> Result<(), AppError> {
    use tauri_plugin_autostart::ManagerExt;
    let launcher = app.autolaunch();
    let result = if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    };
    result.map_err(|e| AppError::internal(format!("Couldn't change the login item: {e}")))
}

/// Writes a file named `name` to Downloads (or home), shows it in Finder, and returns
/// its path. `write` runs off the async runtime.
pub(crate) async fn save_to_downloads<R: tauri::Runtime>(
    app: &AppHandle<R>,
    name: &str,
    write: impl FnOnce(&std::path::Path) -> std::io::Result<()> + Send + 'static,
) -> Result<String, AppError> {
    use tauri_plugin_opener::OpenerExt;
    let dir = app
        .path()
        .download_dir()
        .or_else(|_| app.path().home_dir())
        .map_err(|e| AppError::internal(e.to_string()))?;
    let path = dir.join(name);
    let target = path.clone();
    tauri::async_runtime::spawn_blocking(move || write(&target))
        .await
        .map_err(|e| AppError::internal(e.to_string()))?
        .map_err(|e| AppError::internal(format!("Couldn't save {name}: {e}")))?;
    let _ = app.opener().reveal_item_in_dir(&path);
    Ok(path.display().to_string())
}

/// Saves log lines (as shown, after filtering) to a text file in Downloads, with
/// anything secret-looking redacted, and shows it in Finder. Returns its path.
#[tauri::command]
#[specta::specta]
pub async fn app_save_log(app: AppHandle, lines: Vec<String>) -> Result<String, AppError> {
    let name = teitunnel_core::diagnostics::timestamped_name("teitunnel-log", "txt");
    save_to_downloads(&app, &name, move |path| {
        let mut text = lines
            .iter()
            .map(|line| teitunnel_core::redact::redact(line).into_owned())
            .collect::<Vec<_>>()
            .join("\n");
        text.push('\n');
        std::fs::write(path, text)
    })
    .await
}
