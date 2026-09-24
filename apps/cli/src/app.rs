//! Using the running app over its control connection: `teitunnel share 3000` asks the
//! app to run the share (so it shows and lasts in the app), `shares`, `routes` and
//! `status` read what the app knows. Without the app the commands work as before.

use std::{io::IsTerminal, path::Path, process::ExitCode, time::Duration};

use teitunnel_control::{
    ClientError, ControlClient, Endpoint,
    protocol::{
        ClientInfo, HostHeader, RoutesList, ShareInfo, ShareKind, StartShare, Status, code,
    },
};
use teitunnel_core::quick_share::HostHeaderChoice;

use crate::share::status as note;

/// How this command introduces itself to the app (and in Settings ▸ Integrations).
pub(crate) fn client_info() -> ClientInfo {
    ClientInfo {
        name: "teitunnel-cli".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

/// Where a command runs: `--app`, `--here`, or whichever is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Where {
    /// Use the app if it runs, otherwise this terminal.
    Auto,
    /// The app, or fail.
    App,
    /// This terminal, even if the app runs.
    Here,
}

impl Where {
    pub(crate) fn from_flags(app: bool, here: bool) -> Self {
        match (app, here) {
            (true, _) => Self::App,
            (_, true) => Self::Here,
            _ => Self::Auto,
        }
    }
}

/// The app's control connection, if it's wanted and answers. `Err` when `--app` was
/// given and the app can't be used.
pub(crate) async fn connect(
    data_dir: &Path,
    wanted: Where,
) -> Result<Option<ControlClient>, String> {
    if wanted == Where::Here {
        return Ok(None);
    }
    match ControlClient::connect(&Endpoint::new(data_dir), client_info()).await {
        Ok(client) => Ok(Some(client)),
        Err(err) if wanted == Where::Auto && teitunnel_control::client::is_unavailable(&err) => {
            Ok(None)
        }
        Err(ClientError::NotRunning) => {
            Err("Teitunnel isn't running. Open it, or leave out --app to use this terminal.".into())
        }
        Err(err) => Err(describe(&err)),
    }
}

/// An error from the app, as a sentence.
pub(crate) fn describe(err: &ClientError) -> String {
    match err.code() {
        Some(code::DECLINED) => "Not allowed in Teitunnel. Nothing changed.".into(),
        Some(code::TIMEOUT) => {
            "Teitunnel didn't get an answer in time (the question may still be open there).".into()
        }
        _ => err.to_string(),
    }
}

fn host_header(choice: &HostHeaderChoice) -> HostHeader {
    match choice {
        HostHeaderChoice::Auto => HostHeader::Auto,
        HostHeaderChoice::Off => HostHeader::Off,
        HostHeaderChoice::Set { value } => HostHeader::Set {
            value: value.clone(),
        },
    }
}

/// `teitunnel share <origin>` through the app: the app runs the share and keeps it;
/// this command prints the address and ends.
pub(crate) async fn share(
    client: &ControlClient,
    origin: &str,
    stop_after: Option<Duration>,
    qr: bool,
    choice: &HostHeaderChoice,
) -> Result<ExitCode, String> {
    if client.hello().approved {
        note("Sharing through the Teitunnel app…");
    } else {
        note("Sharing through the Teitunnel app. Allow it there…");
    }
    let share = client
        .start_share(&StartShare {
            origin: origin.to_owned(),
            stop_after_seconds: stop_after.map(|d| d.as_secs()),
            host_header: host_header(choice),
        })
        .await
        .map_err(|e| describe(&e))?;
    let url = share.url.clone().unwrap_or_default();
    out!("{url}")?;
    if qr
        && std::io::stdout().is_terminal()
        && let Some(code) = teitunnel_core::quick_share::qr_terminal(&url)
    {
        out!("\n{code}")?;
    }
    note(&format!(
        "{} is public at {url} for anyone with the link. It runs in the Teitunnel app: stop it there or with `teitunnel shares --stop {url}`.",
        share.origin
    ));
    Ok(ExitCode::SUCCESS)
}

/// One line per share.
pub(crate) fn share_line(share: &ShareInfo, now_ms: u64) -> String {
    let by = match share.kind {
        ShareKind::Quick => "in the app",
        ShareKind::Terminal => "in a terminal",
        ShareKind::Domain => "on your domain",
    };
    let status = if share.status == "live" {
        String::new()
    } else {
        format!(", {}", share.status)
    };
    let ends = share.expires_at.map_or_else(String::new, |at| {
        format!(", ends in {} min", at.saturating_sub(now_ms) / 60_000)
    });
    let requests = share
        .requests
        .map_or_else(String::new, |n| format!(", {n} requests"));
    format!(
        "{}\t{}\t{by}{status}{requests}{ends}",
        share.url.as_deref().unwrap_or("(starting)"),
        share.origin
    )
}

/// `teitunnel shares` through the app.
pub(crate) async fn shares(
    client: &ControlClient,
    stop: Option<&str>,
    json: bool,
) -> Result<ExitCode, String> {
    if let Some(id) = stop {
        client.stop_share(id).await.map_err(|e| describe(&e))?;
        out!("Stopped sharing {id}.")?;
        return Ok(ExitCode::SUCCESS);
    }
    let shares = client.shares().await.map_err(|e| describe(&e))?;
    if json {
        out!(
            "{}",
            serde_json::to_string(&shares).map_err(|e| e.to_string())?
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if shares.is_empty() {
        out!("No shares running. Start one with `teitunnel share 3000`.")?;
    }
    let now = teitunnel_core::domain_shares::now_ms();
    for share in &shares {
        out!("{}", share_line(share, now))?;
    }
    Ok(ExitCode::SUCCESS)
}

/// `teitunnel routes` through the app (its connectors' state is the live one).
pub(crate) fn print_routes(list: &RoutesList, json: bool) -> Result<ExitCode, String> {
    if json {
        out!(
            "{}",
            serde_json::to_string(&list.routes).map_err(|e| e.to_string())?
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if list.routes.is_empty() {
        out!("No routes on this machine in {}.", list.account.name)?;
    }
    for route in &list.routes {
        let path = route
            .path
            .as_deref()
            .map(|p| format!(" {p}"))
            .unwrap_or_default();
        let login = route
            .login
            .as_deref()
            .map(|p| format!("\tlogin: {p}"))
            .unwrap_or_default();
        let tunnel = match (&route.tunnel_name, list.tunnels.len() > 1) {
            (Some(name), true) => format!("\ttunnel: {name}"),
            _ => String::new(),
        };
        out!(
            "{}{path}\t{}\t{}{login}{tunnel}",
            route.hostname,
            route.origin,
            route.status_text
        )?;
        if let Some(connect) = &route.connect {
            out!("    connect: {connect}")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `teitunnel status`: whether the app runs, and what it serves.
pub(crate) async fn status(data_dir: &Path, json: bool) -> Result<ExitCode, String> {
    let client = connect(data_dir, Where::Auto).await?;
    let Some(client) = client else {
        let terminals = teitunnel_core::cli_shares::list(&data_dir.join("run-cli"));
        if json {
            out!(
                "{}",
                serde_json::json!({ "running": false, "terminals": terminals })
            )?;
            return Ok(ExitCode::SUCCESS);
        }
        out!("Teitunnel isn't running.")?;
        if !terminals.is_empty() {
            out!("Shares in terminals:")?;
            for share in &terminals {
                out!("  {}\t{}", share.url, share.origin)?;
            }
        }
        return Ok(ExitCode::SUCCESS);
    };
    let status = client.status().await.map_err(|e| describe(&e))?;
    if json {
        let mut value = serde_json::to_value(&status).map_err(|e| e.to_string())?;
        if let Some(object) = value.as_object_mut() {
            object.insert("running".into(), true.into());
        }
        out!("{value}")?;
        return Ok(ExitCode::SUCCESS);
    }
    for line in status_lines(&status, teitunnel_core::domain_shares::now_ms()) {
        out!("{line}")?;
    }
    Ok(ExitCode::SUCCESS)
}

/// What `status` prints about a running app.
pub(crate) fn status_lines(status: &Status, now_ms: u64) -> Vec<String> {
    let mut lines = vec![format!("Teitunnel {} is running.", status.app.version)];
    if status.accounts.is_empty() {
        lines.push("No Cloudflare account is connected.".into());
    }
    for account in &status.accounts {
        lines.push(format!("{}:", account.name));
        let tunnels: Vec<_> = status
            .tunnels
            .iter()
            .filter(|t| t.account_id == account.id)
            .collect();
        if tunnels.is_empty() {
            lines.push("  no tunnel on this machine yet".into());
        }
        for tunnel in tunnels {
            let default = if tunnel.is_default { ", default" } else { "" };
            lines.push(format!(
                "  tunnel {}: {}{default}",
                tunnel.name, tunnel.state
            ));
        }
    }
    if status.shares.is_empty() {
        lines.push("No shares running.".into());
    } else {
        lines.push("Shares:".into());
        lines.extend(
            status
                .shares
                .iter()
                .map(|s| format!("  {}", share_line(s, now_ms))),
        );
    }
    lines
}

#[cfg(test)]
mod tests;
