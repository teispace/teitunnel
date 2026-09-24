//! `teitunnel share <origin>`: a Quick Share that lives exactly as long as the
//! command. The one place the CLI runs a connector (D-056 keeps routes' connectors in
//! the app): a share started from a terminal belongs to that terminal, so Ctrl-C, closing
//! the terminal or `--for` ends it, and a CLI that dies without stopping it has its
//! connector reaped by the next one.

use std::{
    io::{self, IsTerminal},
    path::Path,
    process::ExitCode,
    time::Duration,
};

use teitunnel_core::{
    domain::OriginUrl,
    quick_share::{QuickShare, QuickShares, ShareStatus, qr_terminal},
    runtime::{PidRegistry, PortAllocator, QUICK_SHARE_PORTS, Supervisor},
    store::Store,
};

use crate::context;

/// The longest `--for` accepted.
const MAX_DURATION: Duration = Duration::from_secs(7 * 24 * 3600);

/// Parses `90s`, `30m`, `2h` or a bare number of minutes.
pub(crate) fn parse_duration(input: &str) -> Result<Duration, String> {
    let input = input.trim();
    let split = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    let (number, unit) = input.split_at(split);
    let value: u64 = number
        .parse()
        .map_err(|_| format!("\"{input}\" isn't a duration. Try 30m, 2h or 90s."))?;
    let seconds = match unit {
        "s" => value,
        "" | "m" => value.saturating_mul(60),
        "h" => value.saturating_mul(3600),
        _ => return Err(format!("\"{input}\" isn't a duration. Try 30m, 2h or 90s.")),
    };
    let duration = Duration::from_secs(seconds);
    if duration.is_zero() || duration > MAX_DURATION {
        return Err("Share for between 1 second and 7 days.".into());
    }
    Ok(duration)
}

/// The app's database when it exists (so the share shows up in its history), otherwise
/// one in memory: sharing doesn't need the app to be set up.
fn store(dir: &Path) -> Result<Store, String> {
    let path = dir.join("teitunnel.db");
    if path.exists() {
        Store::open(&path).map_err(|e| e.to_string())
    } else {
        Store::open_in_memory().map_err(|e| e.to_string())
    }
}

/// Resolves when the terminal asks the command to end: Ctrl-C, or (Unix) the terminal
/// closing or a `kill`.
pub(crate) async fn interrupted() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut hup)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::hangup()),
        ) else {
            let _ = tokio::signal::ctrl_c().await;
            return;
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term.recv() => {}
            _ = hup.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub(crate) fn status(message: &str) {
    use std::io::Write as _;
    let _ = writeln!(io::stderr().lock(), "{message}");
}

pub(crate) async fn run(
    origin: &str,
    stop_after: Option<Duration>,
    qr: bool,
) -> Result<ExitCode, String> {
    let origin = OriginUrl::parse(origin).map_err(|e| e.to_string())?;
    let dir = context::data_dir()?;
    let runs = dir.join("run-cli");
    let reaped = PidRegistry::reap_abandoned(&runs).await;
    if !reaped.is_empty() {
        status(&format!(
            "Stopped {} share(s) left running by an earlier teitunnel.",
            reaped.len()
        ));
    }
    let owner_dir = runs.join(teitunnel_core::runtime::this_process());
    let supervisor = Supervisor::new(
        PidRegistry::for_this_process(&runs),
        tokio::runtime::Handle::current(),
    );
    let shares = QuickShares::new(
        supervisor,
        context::binary(&dir),
        // Spread by pid: the app, or another terminal, may be starting a share too.
        PortAllocator::new(QUICK_SHARE_PORTS).spread(std::process::id()),
        store(&dir)?,
    );
    tokio::spawn(shares.clone().watch_runtime());
    let mut changes = shares.subscribe();
    let share = shares
        .start(origin, stop_after)
        .await
        .map_err(|e| match e {
            teitunnel_core::quick_share::QuickShareError::Binary(
                cloudflared::Error::NotFound,
            ) => "cloudflared isn't installed. Open Teitunnel to install it, or install it with your package manager.".to_owned(),
            other => other.to_string(),
        })?;
    status(&format!("Sharing {}…", share.origin));

    let current = |shares: &QuickShares| -> Option<QuickShare> {
        shares.list().into_iter().find(|s| s.id == share.id)
    };
    let stop = interrupted();
    tokio::pin!(stop);
    let mut announced = false;
    let mut reconnecting = false;
    let outcome = loop {
        match current(&shares) {
            None => break Ok(ExitCode::SUCCESS), // `--for` elapsed.
            Some(QuickShare {
                status: ShareStatus::Failed { message },
                ..
            }) => break Err(message.english()),
            Some(QuickShare {
                status: ShareStatus::Live,
                url: Some(url),
                ..
            }) => {
                if !announced {
                    announced = true;
                    announce(&url, &share, stop_after, qr)?;
                    // So the app can list this share, and stop it.
                    let started_at = teitunnel_core::domain_shares::now_ms();
                    let record = teitunnel_core::cli_shares::CliShare {
                        owner: teitunnel_core::runtime::this_process(),
                        origin: share.origin.to_string(),
                        url: url.clone(),
                        started_at,
                        stop_at: stop_after
                            .map(|d| started_at + u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
                    };
                    if let Err(err) = teitunnel_core::cli_shares::record(&owner_dir, &record) {
                        status(&format!("(Couldn't record the share for the app: {err})"));
                    }
                } else if reconnecting {
                    status("Reconnected.");
                }
                reconnecting = false;
            }
            Some(QuickShare {
                status: ShareStatus::Reconnecting,
                ..
            }) => {
                if !reconnecting {
                    status("Connection lost; reconnecting…");
                }
                reconnecting = true;
            }
            Some(_) => {}
        }
        tokio::select! {
            () = &mut stop => break Ok(ExitCode::SUCCESS),
            _ = changes.recv() => {}
            // `--for` removes the share without a change event reaching a lagging
            // receiver; look again now and then.
            () = tokio::time::sleep(Duration::from_secs(1)) => {}
        }
    };
    teitunnel_core::cli_shares::forget(&owner_dir);
    shares.stop_all().await;
    status("Stopped sharing.");
    outcome
}

fn announce(
    url: &str,
    share: &QuickShare,
    stop_after: Option<Duration>,
    qr: bool,
) -> Result<(), String> {
    // The URL alone on stdout, so `teitunnel share 3000 | head -1` works in scripts.
    out!("{url}")?;
    if qr
        && io::stdout().is_terminal()
        && let Some(code) = qr_terminal(url)
    {
        out!("\n{code}")?;
    }
    let until = stop_after.map_or_else(
        || "Press Ctrl-C to stop.".to_owned(),
        |d| format!("Stops in {}, or press Ctrl-C.", describe(d)),
    );
    status(&format!(
        "{} is public at {url} for anyone with the link. {until}",
        share.origin
    ));
    Ok(())
}

/// A duration in words, e.g. `1 h 30 min`.
fn describe(duration: Duration) -> String {
    let total = duration.as_secs();
    let (h, m, s) = (total / 3600, total % 3600 / 60, total % 60);
    let mut parts = Vec::new();
    if h > 0 {
        parts.push(format!("{h} h"));
    }
    if m > 0 {
        parts.push(format!("{m} min"));
    }
    if s > 0 || parts.is_empty() {
        parts.push(format!("{s} s"));
    }
    parts.join(" ")
}

/// `teitunnel share <origin> --on <hostname>`: a temporary route on one of the
/// account's domains, through this machine's tunnel, for as long as the command runs
/// (or `--for`). The app, or an Always-on connector, serves it; the app also removes it
/// if this command dies without doing so.
pub(crate) async fn run_on_domain(
    app: &crate::context::App,
    hostname: &str,
    origin: &str,
    account: Option<&str>,
    allow: Option<teitunnel_core::engine::AccessRule>,
    stop_after: Option<Duration>,
) -> Result<ExitCode, String> {
    use teitunnel_core::{
        domain_shares::{self, ShareRequest},
        engine::Outcome,
        runtime,
    };
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let connectors = app.connectors(&account).await;
    let ctx = app.context(&account);
    let expires_at = stop_after
        .map(|d| domain_shares::now_ms() + u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let owner = runtime::this_process();
    let outcome = domain_shares::start(
        &app.engine,
        &api,
        &connectors,
        ctx,
        ShareRequest {
            hostname,
            origin,
            access: allow,
            expires_at,
            owner: &owner,
        },
    )
    .await
    .map_err(|e| match e {
        teitunnel_core::engine::EngineError::NeedsConfirmation => {
            format!("{hostname} has a DNS record Teitunnel didn't create. Choose another hostname.")
        }
        other => other.to_string(),
    })?;
    let hostname = hostname.trim().to_ascii_lowercase();
    match outcome {
        Outcome::Applied {
            connector_error, ..
        } => {
            out!("https://{hostname}")?;
            if let Some(error) = connector_error {
                status(&format!("Note: {}", error.english()));
            }
            let until = stop_after.map_or_else(
                || "Press Ctrl-C to stop.".to_owned(),
                |d| format!("Stops in {}, or press Ctrl-C.", describe(d)),
            );
            status(&format!(
                "{origin} is public at https://{hostname}. {until}"
            ));
        }
        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
            return Err(error.english());
        }
    }
    match stop_after {
        Some(after) => {
            tokio::select! {
                () = interrupted() => {}
                () = tokio::time::sleep(after) => {}
            }
        }
        None => interrupted().await,
    }
    let stopped = domain_shares::stop(&app.engine, &api, &connectors, ctx, &hostname).await;
    match stopped {
        Ok(()) => {
            status("Stopped sharing; the route and its DNS record are removed.");
            Ok(ExitCode::SUCCESS)
        }
        Err(message) => Err(format!(
            "Couldn't remove the route: {}. Teitunnel removes it the next time it runs.",
            message.english()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(parse_duration("90s"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_duration("30m"), Ok(Duration::from_secs(1800)));
        assert_eq!(parse_duration("30"), Ok(Duration::from_secs(1800)));
        assert_eq!(parse_duration(" 2h "), Ok(Duration::from_secs(7200)));
        for bad in ["", "m", "1d", "-5m", "0", "169h", "1.5h"] {
            assert!(parse_duration(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn describes_durations() {
        assert_eq!(describe(Duration::from_secs(5400)), "1 h 30 min");
        assert_eq!(describe(Duration::from_secs(45)), "45 s");
        assert_eq!(describe(Duration::from_secs(0)), "0 s");
    }
}
