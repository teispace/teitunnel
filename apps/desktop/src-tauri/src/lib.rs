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

use tauri::Manager;

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
            shell::window::focus_main(app);
        }))
        .plugin(shell::window::state_plugin())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(specta.invoke_handler())
        .setup(move |app| {
            let log_dir = app.path().app_log_dir()?;
            app.manage(logging::init(&log_dir)?);
            specta.mount_events(app);
            shell::window::show_main_after_timeout(app.handle().clone());
            tracing::info!(version = %app.package_info().version, "teitunnel started");
            Ok(())
        })
        .run(tauri::generate_context!())
}
