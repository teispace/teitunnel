//! Window lifecycle: the main window and the Settings window.
//!
//! Windows are created hidden and shown once their webview has painted its first frame
//! (`app_ready`), so there's never a white flash. A timeout shows them anyway if the
//! frontend fails to signal, so a broken build is visible, not silent.

use std::time::Duration;

use tauri::{
    AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    plugin::TauriPlugin,
};
use tauri_plugin_window_state::StateFlags;

const MAIN: &str = "main";
const SETTINGS: &str = "settings";
const READY_TIMEOUT: Duration = Duration::from_secs(3);

/// Shows a window once its webview has painted, if it is still hidden.
pub(crate) fn show_when_ready<R: Runtime>(window: &WebviewWindow<R>) {
    if !window.is_visible().unwrap_or(false)
        && let Err(err) = window.show().and_then(|()| window.set_focus())
    {
        tracing::warn!(window = window.label(), error = %err, "failed to show window");
    }
}

/// Shows `window` after [`READY_TIMEOUT`] if its webview never reported ready.
pub(crate) fn show_after_timeout<R: Runtime>(window: WebviewWindow<R>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(READY_TIMEOUT).await;
        if !window.is_visible().unwrap_or(true) {
            tracing::warn!(
                window = window.label(),
                "webview did not report ready in time"
            );
            show_when_ready(&window);
        }
    });
}

/// Shows the main window after the ready timeout, if needed. Call once at startup.
pub(crate) fn show_main_after_timeout<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN) {
        show_after_timeout(window);
    }
}

/// Brings the main window to the front, e.g. when a second instance is launched.
pub(crate) fn focus_main<R: Runtime>(app: &AppHandle<R>) {
    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    if let Err(err) = bring_to_front(&window) {
        tracing::warn!(error = %err, "failed to focus main window");
    }
}

fn bring_to_front<R: Runtime>(window: &WebviewWindow<R>) -> tauri::Result<()> {
    window.unminimize()?;
    window.show()?;
    window.set_focus()
}

/// Opens the Settings window (⌘,), or brings it to the front if it is already open.
pub(crate) fn open_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window(SETTINGS) {
        return bring_to_front(&window);
    }
    let builder = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("settings".into()))
        .title("Settings")
        .inner_size(620.0, 500.0)
        .resizable(false)
        .minimizable(false)
        .maximizable(false)
        .center()
        .visible(false);
    // Standard-height title bar with the traffic lights in their native spot; the page
    // draws the centred title and the tab toolbar below it.
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true);
    show_after_timeout(builder.build()?);
    Ok(())
}

/// Closing the main window keeps Teitunnel running in the menu bar (tunnels stay up);
/// ⌘Q quits. The Dock icon or the menu bar item brings the window back.
pub(crate) fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event
        && window.label() == MAIN
    {
        api.prevent_close();
        if let Err(err) = window.hide() {
            tracing::warn!(error = %err, "failed to hide main window");
        }
    }
}

/// Restores window size and position, but never visibility: showing is ours to decide.
pub(crate) fn state_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_window_state::Builder::new()
        .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
        .with_denylist(&[SETTINGS])
        .build()
}
