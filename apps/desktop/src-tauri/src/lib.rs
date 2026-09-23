//! Teitunnel desktop shell.
//!
//! A thin adapter between the webview and `teitunnel-core`: IPC commands and events,
//! windows, menus, tray and plugins. Business logic lives in `teitunnel-core`.

// Everything below is private to the shell; `pub` inside modules means crate-visible.
#![allow(unreachable_pub)]

mod bootstrap;
mod error;
mod ipc;
mod logging;
mod shell;
mod state;

use tauri::Manager;

pub use ipc::export_bindings;

/// Builds and runs the application until the last window closes or the user quits.
///
/// # Errors
/// Returns an error when Tauri fails to initialise (e.g. the webview is unavailable).
pub fn run() -> Result<(), tauri::Error> {
    ipc::mark_launch();
    // An E2E build is a test harness, not the app: say so plainly instead of panicking
    // when it's opened by hand.
    #[cfg(feature = "e2e")]
    if std::env::var_os("TEITUNNEL_CLOUDFLARED").is_none() {
        #[allow(clippy::print_stderr)]
        {
            eprintln!(
                "This is Teitunnel's end-to-end test build (built with --features e2e). \
                 It only runs under `pnpm e2e`. For the app, run `pnpm dev` or \
                 `pnpm tauri build --debug`."
            );
        }
        std::process::exit(2);
    }
    let specta = ipc::builder();

    #[cfg(debug_assertions)]
    if let Err(err) = ipc::export_bindings() {
        #[allow(clippy::print_stderr)]
        {
            eprintln!("failed to export IPC bindings: {err}");
        }
    }

    let builder = tauri::Builder::default();
    // E2E builds only: an embedded WebDriver server (it can drive the whole UI, so it
    // must never be compiled into a release).
    #[cfg(feature = "e2e")]
    let builder = builder
        .plugin(tauri_plugin_wdio_webdriver::init())
        .plugin(tauri_plugin_wdio::init());

    builder
        // Must be first so a second launch exits before initialising anything else.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            shell::windows::focus_main(app);
        }))
        .plugin(shell::windows::state_plugin())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        // Open at login starts hidden, into the menu bar (`--hidden`).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .invoke_handler(specta.invoke_handler())
        .menu(shell::menu::build)
        .on_menu_event(|app, event| shell::menu::on_event(app, &event))
        .setup(move |app| {
            // An isolated run (`TEITUNNEL_DATA_DIR`: E2E, measurements) logs there too.
            let log_dir = match std::env::var_os("TEITUNNEL_DATA_DIR") {
                Some(dir) => std::path::PathBuf::from(dir).join("logs"),
                None => app.path().app_log_dir()?,
            };
            app.manage(logging::init(&log_dir)?);
            specta.mount_events(app);
            #[cfg(feature = "e2e")]
            app.add_capability(include_str!("../e2e/capability.json"))?;

            shell::windows::init_start_hidden();
            app.manage(bootstrap::init(app.handle())?);
            shell::windows::show_main_after_timeout(app.handle());
            tracing::info!(version = %app.package_info().version, "teitunnel started");
            Ok(())
        })
        .on_window_event(shell::windows::on_window_event)
        .build(tauri::generate_context!())?
        .run(|app, event| match event {
            tauri::RunEvent::ExitRequested { api, .. } => bootstrap::on_exit_requested(app, &api),
            // Clicking the Dock icon with no visible window reopens the main window.
            #[cfg(target_os = "macos")]
            tauri::RunEvent::Reopen { has_visible_windows: false, .. } => shell::windows::focus_main(app),
            _ => {}
        });
    Ok(())
}
