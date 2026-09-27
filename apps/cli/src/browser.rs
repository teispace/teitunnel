//! The browser extension: `teitunnel browser install|uninstall|status`
//! registers this command as the extension's native messaging host in every installed
//! browser, and a browser starting it runs [`host`], which relays the extension's calls
//! to the running app.

use std::{path::PathBuf, process::ExitCode};

use clap::Subcommand;
use teitunnel_control::Endpoint;
use teitunnel_core::browser_host::{self, Browser, BrowserHostStatus, Layout};

use crate::context;

/// `teitunnel browser …`.
#[derive(Debug, Clone, Copy, Subcommand)]
pub(crate) enum BrowserCommand {
    /// Let the Teitunnel browser extension talk to the app, in every installed browser.
    Install {
        /// Only this browser: chrome, chromium, edge, brave, vivaldi, arc or firefox.
        #[arg(long, value_parser = parse_browser)]
        browser: Option<Browser>,
    },
    /// Stop letting the extension talk to the app.
    Uninstall {
        /// Only this browser.
        #[arg(long, value_parser = parse_browser)]
        browser: Option<Browser>,
    },
    /// Which browsers can use the extension.
    Status {
        /// JSON output.
        #[arg(long)]
        json: bool,
    },
}

fn parse_browser(value: &str) -> Result<Browser, String> {
    Browser::parse(value).ok_or_else(|| {
        format!("\"{value}\" isn't a browser. Use chrome, chromium, edge, brave, vivaldi, arc or firefox.")
    })
}

/// This command, as browsers should start it.
fn exe() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

/// "3 minutes", "2 hours", "5 days".
fn age(millis: u64) -> String {
    let minutes = millis / 60_000;
    let (count, unit) = match minutes {
        0..60 => (minutes.max(1), "minute"),
        60..1440 => (minutes / 60, "hour"),
        _ => (minutes / 1440, "day"),
    };
    format!("{count} {unit}{}", if count == 1 { "" } else { "s" })
}

fn print(status: &[BrowserHostStatus], json: bool) -> Result<(), String> {
    if json {
        out!(
            "{}",
            serde_json::to_string_pretty(status).map_err(|e| e.to_string())?
        )?;
        return Ok(());
    }
    let now = teitunnel_core::domain_shares::now_ms();
    for browser in status.iter().filter(|b| b.detected) {
        let seen = browser.extension_seen_at.map_or_else(String::new, |at| {
            format!(
                "; extension last connected {} ago",
                age(now.saturating_sub(at))
            )
        });
        out!(
            "{}: {}{seen}",
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

pub(crate) fn run(command: BrowserCommand) -> Result<ExitCode, String> {
    let layout = Layout::detect().ok_or("Couldn't find your home folder.")?;
    let exe = exe()?;
    let seen = context::data_dir()
        .map(|dir| browser_host::seen(&dir))
        .unwrap_or_default();
    match command {
        BrowserCommand::Install { browser } => {
            let status = browser_host::with_seen(
                layout.install(&exe, browser).map_err(|e| e.to_string())?,
                &seen,
            );
            print(&status, false)?;
            if status.iter().any(|b| b.installed) {
                out!(
                    "Install the Teitunnel extension in the browser, then open its menu on a local page."
                )?;
            }
        }
        BrowserCommand::Uninstall { browser } => {
            print(
                &browser_host::with_seen(
                    layout.uninstall(&exe, browser).map_err(|e| e.to_string())?,
                    &seen,
                ),
                false,
            )?;
        }
        BrowserCommand::Status { json } => {
            print(&browser_host::with_seen(layout.status(&exe), &seen), json)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Serves the extension over standard input and output until the browser closes them.
/// Nothing else may be written to standard output.
pub(crate) async fn host(args: &[String]) -> ExitCode {
    let Ok(dir) = context::data_dir() else {
        return ExitCode::FAILURE;
    };
    if let Some(layout) = Layout::detect() {
        browser_host::record_started(&dir, args, &layout, teitunnel_core::domain_shares::now_ms());
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn says_how_long_ago() {
        assert_eq!(age(10_000), "1 minute");
        assert_eq!(age(5 * 60_000), "5 minutes");
        assert_eq!(age(2 * 3_600_000), "2 hours");
        assert_eq!(age(3 * 86_400_000), "3 days");
    }
}
