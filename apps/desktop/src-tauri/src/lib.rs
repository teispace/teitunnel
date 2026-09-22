//! Teitunnel desktop shell.
//!
//! A thin adapter between the webview and `teitunnel-core`: IPC commands and events,
//! windows, menus, tray and plugins. Business logic lives in `teitunnel-core`.

// Everything below is private to the shell; `pub` inside modules means crate-visible.
#![allow(unreachable_pub)]

mod error;
mod ipc;
mod logging;
mod shell;
mod state;

use tauri::Manager;
use teitunnel_core::{settings, store::Store};

pub use ipc::export_bindings;

/// Builds and runs the application until the last window closes or the user quits.
///
/// # Errors
/// Returns an error when Tauri fails to initialise (e.g. the webview is unavailable).
pub fn run() -> Result<(), tauri::Error> {
    let specta = ipc::builder();

    #[cfg(debug_assertions)]
    if let Err(err) = ipc::export_bindings() {
        #[allow(clippy::print_stderr)]
        {
            eprintln!("failed to export IPC bindings: {err}");
        }
    }

    tauri::Builder::default()
        // Must be first so a second launch exits before initialising anything else.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::windows::focus_main(app);
        }))
        .plugin(shell::windows::state_plugin())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(specta.invoke_handler())
        .menu(shell::menu::build)
        .on_menu_event(|app, event| shell::menu::on_event(app, &event))
        .setup(move |app| {
            let log_dir = app.path().app_log_dir()?;
            app.manage(logging::init(&log_dir)?);
            specta.mount_events(app);

            let store = Store::open(&app.path().app_data_dir()?.join("teitunnel.db"))?;
            let prefs = tauri::async_runtime::block_on(settings::load(&store))?;
            shell::tray::install(app.handle(), prefs.show_in_menu_bar)?;
            app.manage(state::AppState { store });
            shell::windows::show_main_after_timeout(app.handle());
            tracing::info!(version = %app.package_info().version, "teitunnel started");
            Ok(())
        })
        .on_window_event(shell::windows::on_window_event)
        .build(tauri::generate_context!())?
        .run(|app, event| {
            // Clicking the Dock icon with no visible window reopens the main window.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { has_visible_windows: false, .. } = event {
                shell::windows::focus_main(app);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
    Ok(())
}
