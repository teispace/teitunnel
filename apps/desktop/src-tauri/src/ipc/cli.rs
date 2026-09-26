//! Putting the bundled `teitunnel` on the PATH. The work is in
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
pub async fn cli_status() -> Result<CliState, AppError> {
    super::off_main(|| layout().map_or(CliState::Unavailable, |l| l.state())).await
}

/// Puts the command line tool on the PATH.
#[tauri::command]
#[specta::specta]
pub async fn cli_install() -> Result<CliState, AppError> {
    super::off_main(|| {
        let Some(layout) = layout() else {
            return Ok(CliState::Unavailable);
        };
        layout.install().map_err(|err| failed(&err))
    })
    .await?
}

/// Removes the command line tool Teitunnel installed.
#[tauri::command]
#[specta::specta]
pub async fn cli_uninstall() -> Result<CliState, AppError> {
    super::off_main(|| {
        let Some(layout) = layout() else {
            return Ok(CliState::Unavailable);
        };
        layout.uninstall().map_err(|err| failed(&err))
    })
    .await?
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

/// The browser extension's link to the app: which browsers can start the
/// bundled `teitunnel` as its native messaging host.
#[derive(Debug, Clone, serde::Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct BrowserHostView {
    /// This build carries the command line tool (development builds don't).
    pub available: bool,
    /// Every supported browser.
    pub browsers: Vec<teitunnel_core::browser_host::BrowserHostStatus>,
}

/// The bundled command line tool, and the browsers' folders.
fn browsers() -> Option<(std::path::PathBuf, teitunnel_core::browser_host::Layout)> {
    Some((
        layout()?.bundled,
        teitunnel_core::browser_host::Layout::detect()?,
    ))
}

const UNAVAILABLE: BrowserHostView = BrowserHostView {
    available: false,
    browsers: Vec::new(),
};

/// Which browsers can use the extension.
#[tauri::command]
#[specta::specta]
pub async fn browser_host_status() -> Result<BrowserHostView, AppError> {
    super::off_main(|| {
        browsers().map_or(UNAVAILABLE, |(exe, layout)| BrowserHostView {
            available: true,
            browsers: layout.status(&exe),
        })
    })
    .await
}

/// Lets the extension talk to the app, in every installed browser.
#[tauri::command]
#[specta::specta]
pub async fn browser_host_install() -> Result<BrowserHostView, AppError> {
    let Some((exe, layout)) = browsers() else {
        return Ok(UNAVAILABLE);
    };
    Ok(BrowserHostView {
        available: true,
        browsers: layout.install(&exe).await.map_err(|err| failed(&err))?,
    })
}

/// Stops letting the extension talk to the app.
#[tauri::command]
#[specta::specta]
pub async fn browser_host_uninstall() -> Result<BrowserHostView, AppError> {
    let Some((exe, layout)) = browsers() else {
        return Ok(UNAVAILABLE);
    };
    Ok(BrowserHostView {
        available: true,
        browsers: layout.uninstall(&exe).await.map_err(|err| failed(&err))?,
    })
}
