//! Window lifecycle: the main window and the Settings window.
//!
//! Windows are created hidden and shown once their webview has painted its first frame
//! (`app_ready`), so there's never a white flash. A timeout shows them anyway if the
//! frontend fails to signal, so a broken build is visible, not silent.

use std::{
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Duration,
};

use tauri::{
    AppHandle, Manager, Runtime, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    plugin::TauriPlugin,
};
use tauri_plugin_window_state::StateFlags;

const MAIN: &str = "main";
const SETTINGS: &str = "settings";
const READY_TIMEOUT: Duration = Duration::from_secs(3);

/// Set when the app was opened at login (`--hidden`): the main window stays hidden
/// until the user opens it (menu bar, Dock, second launch).
static START_HIDDEN: AtomicBool = AtomicBool::new(false);

/// Records whether this launch should keep the main window hidden. Call once at startup.
pub(crate) fn init_start_hidden() {
    let hidden = std::env::args().any(|arg| arg == "--hidden");
    START_HIDDEN.store(hidden, Ordering::SeqCst);
}

fn stays_hidden<R: Runtime>(window: &WebviewWindow<R>) -> bool {
    window.label() == MAIN && START_HIDDEN.load(Ordering::SeqCst)
}

/// Shows a window once its webview has painted, if it is still hidden.
pub(crate) fn show_when_ready<R: Runtime>(window: &WebviewWindow<R>) {
    if stays_hidden(window) {
        return;
    }
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
    // The user asked for the window: a login launch no longer keeps it hidden.
    START_HIDDEN.store(false, Ordering::SeqCst);
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
///
/// Everything happens on the async runtime: on Windows, building a webview from the main
/// thread (a synchronous command, a menu or tray event) deadlocks in WebView2, so the
/// sidebar, menu and tray entries did nothing there.
pub(crate) fn open_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // One at a time: two quick clicks could otherwise both find no window and build
        // two. A click while one is opening has nothing to add: that one shows it.
        let Some(_opening) = Opening::start() else {
            return;
        };
        if let Err(err) = show_or_build_settings(&app) {
            tracing::warn!(error = %err, "failed to open settings");
        }
    });
    Ok(())
}

/// Set while a click is opening Settings.
static SETTINGS_OPENING: AtomicBool = AtomicBool::new(false);

/// Holds [`SETTINGS_OPENING`] until dropped.
struct Opening;

impl Opening {
    fn start() -> Option<Self> {
        SETTINGS_OPENING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then_some(Self)
    }
}

impl Drop for Opening {
    fn drop(&mut self) {
        SETTINGS_OPENING.store(false, Ordering::Release);
    }
}

/// How many Settings windows failed to build this session. A failed build (for example
/// WebView2's `TaskCanceled` when the app is asked to quit meanwhile) leaves its label
/// registered with no native window until the app restarts, and Tauri has no way to drop
/// it, so the next one gets a new label.
static SETTINGS_FAILED: AtomicU32 = AtomicU32::new(0);

/// The label for the Settings window after `failed` failed builds.
fn settings_label(failed: u32) -> String {
    if failed == 0 {
        SETTINGS.to_owned()
    } else {
        format!("{SETTINGS}-{}", failed + 1)
    }
}

/// Whether `label` is a Settings window's.
fn is_settings(label: &str) -> bool {
    label == SETTINGS
        || label
            .strip_prefix(SETTINGS)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// Brings the Settings window to the front, or builds it. Called off the main thread, so
/// a getter waits behind a build still in progress: a window that's only slow to build is
/// never mistaken for one that failed.
fn show_or_build_settings<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let label = settings_label(SETTINGS_FAILED.load(Ordering::Acquire));
    if let Some(window) = app.get_webview_window(&label) {
        match window.is_visible() {
            Ok(_) => return bring_to_front(&window),
            Err(err) => {
                tracing::warn!(
                    window = %label,
                    error = %err,
                    "settings window has no native window (it failed to build); building a new one"
                );
                let failed = SETTINGS_FAILED.fetch_add(1, Ordering::AcqRel) + 1;
                return build_settings(app, &settings_label(failed));
            }
        }
    }
    build_settings(app, &label)
}

fn build_settings<R: Runtime>(app: &AppHandle<R>, label: &str) -> tauri::Result<()> {
    let builder = WebviewWindowBuilder::new(app, label, WebviewUrl::App("settings".into()))
        .title(teitunnel_core::text::msg::menu::settings_window().to_string())
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
/// ⌘Q quits. The Dock icon or the menu bar item brings the window back. On Windows and
/// Linux, where the tray icon is the only way back, closing quits when it's hidden
/// (asking first if routes run, like quitting).
pub(crate) fn on_window_event<R: Runtime>(window: &tauri::Window<R>, event: &tauri::WindowEvent) {
    if let tauri::WindowEvent::CloseRequested { api, .. } = event
        && window.label() == MAIN
    {
        api.prevent_close();
        if !cfg!(target_os = "macos") && !super::tray::is_visible() {
            window.app_handle().exit(0);
            return;
        }
        if let Err(err) = window.hide() {
            tracing::warn!(error = %err, "failed to hide main window");
        }
    }
}

/// Restores window size and position, but never visibility: showing is ours to decide.
pub(crate) fn state_plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_window_state::Builder::new()
        .with_state_flags(StateFlags::all() - StateFlags::VISIBLE)
        .with_filter(|label| !is_settings(label))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_settings_window_gets_a_new_label() {
        assert_eq!(settings_label(0), "settings");
        assert_eq!(settings_label(1), "settings-2");
        assert_eq!(settings_label(2), "settings-3");
        for failed in 0..4 {
            assert!(is_settings(&settings_label(failed)));
        }
        for other in [
            "main",
            "settings-",
            "settings-x",
            "settingsx",
            "settings-2a",
        ] {
            assert!(!is_settings(other), "{other}");
        }
    }

    #[test]
    fn settings_opens_one_at_a_time() {
        let first = Opening::start();
        assert!(first.is_some());
        assert!(Opening::start().is_none(), "a second click while opening");
        drop(first);
        assert!(
            Opening::start().is_some(),
            "free again once the first is done"
        );
    }
}
