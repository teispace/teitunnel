//! Main window lifecycle.
//!
//! The window is created hidden (see `tauri.conf.json`) and shown once the webview
//! has painted its first frame, so there's never a white flash. A timeout shows it
//! anyway if the frontend fails to signal, so a broken build is visible, not silent.

use std::time::Duration;

use tauri::{AppHandle, Manager, Runtime, WebviewWindow, plugin::TauriPlugin};
use tauri_plugin_window_state::StateFlags;

const MAIN: &str = "main";
const READY_TIMEOUT: Duration = Duration::from_secs(3);

fn main_window<R: Runtime>(app: &AppHandle<R>) -> Option<WebviewWindow<R>> {
    app.get_webview_window(MAIN)
}

/// Shows the main window if it is still hidden.
pub(crate) fn show_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = main_window(app) else {
        return;
    };
    if !window.is_visible().unwrap_or(false)
        && let Err(err) = window.show().and_then(|()| window.set_focus())
    {
        tracing::warn!(error = %err, "failed to show main window");
    }
}

/// Brings the main window to the front, e.g. when a second instance is launched.
pub(crate) fn focus_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = main_window(app) else {
        return;
    };
    let result = window
        .unminimize()
        .and_then(|()| window.show())
        .and_then(|()| window.set_focus());
    if let Err(err) = result {
        tracing::warn!(error = %err, "failed to focus main window");
    }
}

/// Shows the main window after [`READY_TIMEOUT`] if the webview never reported ready.
pub(crate) fn show_main_after_timeout<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(READY_TIMEOUT).await;
        if main_window(&app).is_some_and(|w| !w.is_visible().unwrap_or(true)) {
            tracing::warn!("webview did not report ready in time; showing window anyway");
            show_main(&app);
        }
    });
}

/// Restores window size and position, but never visibility: showing is ours to decide.
pub(crate) fn state_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_window_state::Builder::new()
        .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
        .build()
}
