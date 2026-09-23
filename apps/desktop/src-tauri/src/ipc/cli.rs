//! Putting the bundled `teitunnel-cli` on the PATH (D-077). The work is in
//! `teitunnel_core::cli_install`.

use teitunnel_core::{
    cli_install::{CliState, Layout},
    text::msg::cli_install as m,
};

use crate::error::{AppError, ErrorCode};

fn layout() -> Option<Layout> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| Layout::detect(&exe))
}

fn failed(err: &std::io::Error) -> AppError {
    tracing::warn!(error = %err, "command line tool");
    let code = if err.kind() == std::io::ErrorKind::AlreadyExists {
        ErrorCode::Conflict
    } else {
        ErrorCode::Internal
    };
    AppError::new(code, m::failed(err.to_string()))
}

/// Whether the command line tool is on the PATH, and how to put it there.
#[tauri::command]
#[specta::specta]
pub fn cli_status() -> CliState {
    layout().map_or(CliState::Unavailable, |l| l.state())
}

/// Puts the command line tool on the PATH.
#[tauri::command]
#[specta::specta]
pub fn cli_install() -> Result<CliState, AppError> {
    let Some(layout) = layout() else {
        return Ok(CliState::Unavailable);
    };
    layout.install().map_err(|err| failed(&err))
}

/// Removes the command line tool Teitunnel installed.
#[tauri::command]
#[specta::specta]
pub fn cli_uninstall() -> Result<CliState, AppError> {
    let Some(layout) = layout() else {
        return Ok(CliState::Unavailable);
    };
    layout.uninstall().map_err(|err| failed(&err))
}

/// At launch: an installed copy follows the app after an update.
pub fn refresh_installed() {
    if let Some(layout) = layout() {
        match layout.refresh() {
            Ok(true) => tracing::info!("updated the installed command line tool"),
            Ok(false) => {}
            Err(err) => tracing::warn!(error = %err, "couldn't update the command line tool"),
        }
    }
}
