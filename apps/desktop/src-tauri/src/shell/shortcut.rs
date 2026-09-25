//! The global shortcut (Settings ▸ Integrations, off by default): registered with the
//! system while it's on. What a press does is decided in
//! `teitunnel_core::quick_actions`.

use std::sync::{Mutex, PoisonError};

use tauri::{AppHandle, Manager, Runtime, plugin::TauriPlugin};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};
use teitunnel_core::{
    control::integrations::{GlobalShortcut, ShortcutAction, ShortcutError},
    quick_actions::{ShortcutPick, pick_for_shortcut},
};

use crate::state::AppState;

/// What the registered shortcut does (`None`: none is registered).
#[derive(Default)]
struct Registered(Mutex<Option<ShortcutAction>>);

/// The plugin, calling [`pressed`] on each press.
pub(crate) fn plugin<R: Runtime>() -> TauriPlugin<R> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, _shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                pressed(app);
            }
        })
        .build()
}

/// Whether global shortcuts can't work in this session (Wayland gives apps none).
fn unsupported_here() -> bool {
    cfg!(target_os = "linux")
        && std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s.eq_ignore_ascii_case("wayland"))
}

/// Registers `setting` (replacing any earlier shortcut), or none when it's off.
///
/// # Errors
/// The keys aren't a shortcut, or the system refused it (another app has it).
pub(crate) fn apply<R: Runtime>(
    app: &AppHandle<R>,
    setting: &GlobalShortcut,
) -> Result<(), ShortcutError> {
    let Some(registered) = app.try_state::<Registered>() else {
        return Err(ShortcutError::Unsupported);
    };
    let shortcuts = app.global_shortcut();
    if let Err(err) = shortcuts.unregister_all() {
        tracing::warn!(%err, "couldn't remove the global shortcut");
    }
    *registered.0.lock().unwrap_or_else(PoisonError::into_inner) = None;
    if !setting.enabled {
        return Ok(());
    }
    let shortcut: Shortcut = setting
        .keys
        .parse()
        .map_err(|_| ShortcutError::Invalid(setting.keys.clone()))?;
    shortcuts.register(shortcut).map_err(|err| {
        tracing::warn!(%err, keys = %setting.keys, "couldn't register the global shortcut");
        if unsupported_here() {
            ShortcutError::Unsupported
        } else {
            ShortcutError::Taken
        }
    })?;
    *registered.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(setting.action);
    Ok(())
}

/// Registers the saved shortcut at launch; a failure is only logged (Settings shows the
/// switch, and turning it on again reports why).
pub(crate) fn restore<R: Runtime>(app: &AppHandle<R>) {
    app.manage(Registered::default());
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let setting = match tauri::async_runtime::block_on(teitunnel_core::control::integrations::load(
        &state.store,
    )) {
        Ok(settings) => settings.shortcut,
        Err(err) => {
            tracing::warn!(%err, "couldn't read the global shortcut");
            return;
        }
    };
    if let Err(err) = apply(app, &setting) {
        tracing::warn!(%err, "the global shortcut isn't available");
    }
}

fn pressed<R: Runtime>(app: &AppHandle<R>) {
    let Some(action) = app
        .try_state::<Registered>()
        .and_then(|r| *r.0.lock().unwrap_or_else(PoisonError::into_inner))
    else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app.try_state::<AppState>() else {
            return;
        };
        let services = if action == ShortcutAction::OpenQuickShare {
            Vec::new()
        } else {
            teitunnel_core::discovery::services().await
        };
        match pick_for_shortcut(action, &services, &state.quick_shares.list()) {
            ShortcutPick::Share { origin } => super::tray::share_from_outside(&app, origin),
            ShortcutPick::Copy { url } => super::tray::copy_address(&app, &url),
            ShortcutPick::Choose => super::tray::open_quick_share(&app),
        }
    });
}
