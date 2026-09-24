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
    dev_server::DevServer,
    domain::OriginUrl,
    engine::{Failure, Verification},
    quick_share::{
        HostHeader, HostHeaderChoice, QuickShare, QuickShares, ShareStatus, qr_terminal,
    },
    runtime::{PidRegistry, PortAllocator, QUICK_SHARE_PORTS, Supervisor},
    store::Store,
    text::msg::dev_server as m,
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

/// `--host-header <HOST>` / `--no-host-header` / neither (automatic).
pub(crate) fn host_header_choice(value: Option<String>, off: bool) -> HostHeaderChoice {
    match (value, off) {
        (Some(value), _) => HostHeaderChoice::Set { value },
        (None, true) => HostHeaderChoice::Off,
        (None, false) => HostHeaderChoice::Auto,
    }
}

/// Says which Host header a share sends, and why when Teitunnel chose it.
fn announce_host_header(host_header: Option<&HostHeader>) {
    match host_header {
        Some(HostHeader {
            value,
            auto_for: Some(server),
        }) => status(&m::auto_host_header(value, server.name()).to_string()),
        Some(HostHeader {
            value,
            auto_for: None,
        }) => status(&m::sending_host_header(value).to_string()),
        None => {}
    }
}

/// Where a checked address is served from, for the advice that fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Via {
    /// A Quick Share (restarted with `--host-header`).
    QuickShare,
    /// A share on your own domain.
    Domain,
    /// A route (`--host-header` on the route).
    Route,
}

/// Explains a check's findings the way the app does: a dev server refusing the
/// address with the config line that fixes it (and the Host header alternative), an
/// edge limit, or a stream a Quick Share can't carry. Prints nothing when all is well.
pub(crate) fn explain(result: &Verification, via: Via, print: &mut dyn FnMut(&str)) {
    match &result.failure {
        Some(Failure::HostRejected { rejection }) => {
            let name = rejection.server.name();
            print(&m::rejected(name, &rejection.host).to_string());
            print(&m::config_line(&rejection.config_file).to_string());
            for line in rejection.config_line.lines() {
                print(&format!("    {line}"));
            }
            match &rejection.host_header {
                _ if rejection.server == DevServer::Next => print(&m::next_origin().to_string()),
                Some(host) if !rejection.host_header_safe => {
                    print(&m::host_header_unsafe(host, name).to_string());
                }
                Some(host) if via == Via::Route => {
                    print(&m::route_host_header(host, name).to_string());
                }
                Some(host) => print(&m::share_host_header(host, name).to_string()),
                None => {}
            }
        }
        // A check that couldn't reach Cloudflare says nothing about the share.
        Some(Failure::EdgeUnreachable { .. }) | None => {}
        Some(failure) if via != Via::Route => print(&failure.message().to_string()),
        Some(_) => {}
    }
    if result.event_stream && via == Via::QuickShare {
        print(&m::event_stream().to_string());
    }
}

/// Quick Shares run by this process (reaping what an earlier CLI left running), and the
/// folder where this process records them for the app.
pub(crate) async fn quick_shares(dir: &Path) -> Result<(QuickShares, std::path::PathBuf), String> {
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
        context::binary(dir),
        // Spread by pid: the app, or another terminal, may be starting a share too.
        PortAllocator::new(QUICK_SHARE_PORTS).spread(std::process::id()),
        store(dir)?,
        dir.join("quick-share.yml"),
    )
    .with_edge(context::edge());
    tokio::spawn(shares.clone().watch_runtime());
    Ok((shares, owner_dir))
}

/// The message for a Quick Share that couldn't start.
pub(crate) fn start_error(err: teitunnel_core::quick_share::QuickShareError) -> String {
    match err {
        teitunnel_core::quick_share::QuickShareError::Binary(cloudflared::Error::NotFound) => {
            "cloudflared isn't installed. Open Teitunnel to install it, or install it with your package manager.".to_owned()
        }
        other => other.to_string(),
    }
}

pub(crate) async fn run(
    origin: &str,
    stop_after: Option<Duration>,
    qr: bool,
    host_header: &HostHeaderChoice,
    strict: bool,
) -> Result<ExitCode, String> {
    let origin = OriginUrl::parse(origin).map_err(|e| e.to_string())?;
    let dir = context::data_dir()?;
    crate::exposure::check(origin.as_str(), Some(&store(&dir)?), strict).await?;
    let (shares, owner_dir) = quick_shares(&dir).await?;
    let mut changes = shares.subscribe();
    let share = shares
        .start(origin, stop_after, host_header)
        .await
        .map_err(start_error)?;
    status(&format!("Sharing {}…", share.origin));
    announce_host_header(share.host_header.as_ref());

    let current = |shares: &QuickShares| -> Option<QuickShare> {
        shares.list().into_iter().find(|s| s.id == share.id)
    };
    let stop = interrupted();
    tokio::pin!(stop);
    let mut announced = false;
    let mut checked = false;
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
                check,
                ..
            }) => {
                if announced
                    && !checked
                    && let Some(check) = check
                {
                    checked = true;
                    explain(&check, Via::QuickShare, &mut status);
                }
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
#[allow(clippy::too_many_arguments)]
pub(crate) async fn run_on_domain(
    app: &crate::context::App,
    hostname: &str,
    origin: &str,
    account: Option<&str>,
    allow: Option<teitunnel_core::engine::AccessRule>,
    stop_after: Option<Duration>,
    host_header: &HostHeaderChoice,
    strict: bool,
) -> Result<ExitCode, String> {
    use teitunnel_core::{
        domain::Hostname,
        domain_shares::{self, ShareRequest},
        engine::Outcome,
        runtime,
    };
    crate::exposure::check(origin, Some(app.store()), strict).await?;
    let host_header = host_header
        .resolve(origin)
        .await
        .map_err(|e| e.to_string())?;
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
            host_header: host_header.as_ref().map(|h| h.value.clone()),
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
            announce_host_header(host_header.as_ref());
            if let Ok(host) = Hostname::parse(&hostname)
                && let Ok(result) = app
                    .engine
                    .verify(&api, ctx, &host, context::edge(), Duration::from_secs(30))
                    .await
            {
                explain(&result, Via::Domain, &mut status);
            }
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

    fn explained(result: &Verification, via: Via) -> Vec<String> {
        let mut lines = Vec::new();
        explain(result, via, &mut |line| lines.push(line.to_owned()));
        lines
    }

    #[test]
    fn explains_a_dev_server_refusing_the_address() {
        use teitunnel_core::{dev_server::HostRejection, domain::RouteOrigin};
        let origin = RouteOrigin::parse("http://localhost:5173").unwrap();
        let rejected = |server| Verification {
            hostname: "quiet-river.trycloudflare.com".into(),
            status: Some(403),
            failure: Some(Failure::HostRejected {
                rejection: HostRejection::new(
                    server,
                    "quiet-river.trycloudflare.com",
                    Some(&origin),
                ),
            }),
            message: None,
            protected: false,
            event_stream: false,
        };
        let vite = explained(&rejected(DevServer::Vite), Via::QuickShare);
        assert_eq!(
            vite[0],
            "Vite refuses requests for quiet-river.trycloudflare.com: it only answers addresses it knows."
        );
        assert!(vite[1].contains("vite.config.js"), "{vite:?}");
        assert_eq!(
            vite[2],
            "    server: { allowedHosts: ['.trycloudflare.com'] }"
        );
        assert!(vite[3].contains("--host-header localhost:5173"), "{vite:?}");

        let rails = explained(&rejected(DevServer::Rails), Via::QuickShare);
        assert!(
            rails
                .last()
                .unwrap()
                .contains("allowing the address is better"),
            "{rails:?}"
        );
        let next = explained(&rejected(DevServer::Next), Via::QuickShare);
        assert!(
            next.last().unwrap().contains("not the Host header"),
            "{next:?}"
        );
    }

    #[test]
    fn explains_streams_and_limits_only_where_they_apply() {
        let mut result = Verification {
            hostname: "quiet-river.trycloudflare.com".into(),
            status: Some(200),
            failure: None,
            message: None,
            protected: false,
            event_stream: true,
        };
        assert_eq!(explained(&result, Via::QuickShare).len(), 1);
        assert!(
            explained(&result, Via::Domain).is_empty(),
            "domains carry streams"
        );
        result.event_stream = false;
        result.failure = Some(Failure::TooManyRequests);
        assert!(explained(&result, Via::QuickShare)[0].contains("200 requests"));
        result.failure = Some(Failure::EdgeUnreachable {
            message: "offline".into(),
        });
        assert!(explained(&result, Via::QuickShare).is_empty());
    }

    #[test]
    fn describes_durations() {
        assert_eq!(describe(Duration::from_secs(5400)), "1 h 30 min");
        assert_eq!(describe(Duration::from_secs(45)), "45 s");
        assert_eq!(describe(Duration::from_secs(0)), "0 s");
    }
}
