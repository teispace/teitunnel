//! The browser extension (M12-07, D-133): `teitunnel browser install|uninstall|status`
//! registers this command as the extension's native messaging host in every installed
//! browser, and a browser starting it runs [`host`], which relays the extension's calls
//! to the running app.

use std::{path::PathBuf, process::ExitCode};

use clap::Subcommand;
use teitunnel_control::Endpoint;
use teitunnel_core::browser_host::{self, BrowserHostStatus, Layout};

use crate::context;

/// `teitunnel browser …`.
#[derive(Debug, Subcommand)]
pub(crate) enum BrowserCommand {
    /// Let the Teitunnel browser extension talk to the app, in every installed browser.
    Install,
    /// Stop letting the extension talk to the app.
    Uninstall,
    /// Which browsers can use the extension.
    Status {
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
}

/// This command, as browsers should start it.
fn exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

fn print(status: &[BrowserHostStatus], json: bool) -> Result<(), String> {
    if json {
        out!(
            "{}",
            serde_json::to_string_pretty(status).map_err(|e| e.to_string())?
        )?;
        return Ok(());
    }
    for browser in status.iter().filter(|b| b.detected) {
        out!(
            "{}: {}",
            browser.name,
            if browser.installed {
                "ready"
            } else {
                "not set up"
            }
        )?;
    }
    if !status.iter().any(|b| b.detected) {
        out!("No supported browser found (Chrome, Edge, Brave, Chromium, Vivaldi, Arc, Firefox).")?;
    }
    Ok(())
}

pub(crate) async fn run(command: BrowserCommand) -> Result<ExitCode, String> {
    let layout = Layout::detect().ok_or("Couldn't find your home folder.")?;
    let exe = exe()?;
    match command {
        BrowserCommand::Install => {
            let status = layout.install(&exe).await.map_err(|e| e.to_string())?;
            print(&status, false)?;
            if status.iter().any(|b| b.installed) {
                out!(
                    "Install the Teitunnel extension in the browser, then open its menu on a local page."
                )?;
            }
        }
        BrowserCommand::Uninstall => {
            print(
                &layout.uninstall(&exe).await.map_err(|e| e.to_string())?,
                false,
            )?;
        }
        BrowserCommand::Status { json } => print(&layout.status(&exe), json)?,
    }
    Ok(ExitCode::SUCCESS)
}

/// Serves the extension over standard input and output until the browser closes them.
/// Nothing else may be written to standard output.
pub(crate) async fn host() -> ExitCode {
    let Ok(dir) = context::data_dir() else {
        return ExitCode::FAILURE;
    };
    match browser_host::serve(
        tokio::io::stdin(),
        tokio::io::stdout(),
        &Endpoint::new(&dir),
    )
    .await
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}
