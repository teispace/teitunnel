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
    Secret,
    dev_server::DevServer,
    domain::OriginUrl,
    engine::{Failure, Verification},
    inspect::{Inspector, TapPatch, lens::TapId},
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
pub(crate) fn store(dir: &Path) -> Result<Store, String> {
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

/// How a share goes through the inspector.
#[derive(Debug, Clone, Default)]
pub(crate) struct ShareOptions {
    /// Through the inspector (the default; `--no-inspect` turns it off).
    pub(crate) inspect: bool,
    /// Stop after this long without a request.
    pub(crate) idle: Option<Duration>,
    /// Paths that print a notice when requested, e.g. `/webhooks/*`.
    pub(crate) watch: Vec<String>,
    /// Print a line per request.
    pub(crate) log: bool,
    /// Require this bearer token (`Authorization: Bearer …`).
    pub(crate) bearer: Option<Secret<String>>,
}

/// Configures an inspected share's tap: idle stop and watched paths.
fn configure_tap(inspector: &Inspector, tap: &TapId, options: &ShareOptions) -> Result<(), String> {
    inspector
        .configure(
            tap,
            &TapPatch {
                idle_stop_minutes: options
                    .idle
                    .map(|d| u32::try_from(d.as_secs().div_ceil(60)).unwrap_or(u32::MAX)),
                watched_paths: (!options.watch.is_empty()).then(|| options.watch.clone()),
                ..TapPatch::default()
            },
        )
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Prints watched-path hits and idle stops.
fn announce_events(inspector: &Inspector) -> tokio::task::JoinHandle<()> {
    use teitunnel_core::inspect::InspectEvent;
    use tokio::sync::broadcast::error::RecvError;
    let mut events = inspector.subscribe();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(InspectEvent::Watched { method, path, .. }) => {
                    status(&format!("→ {method} {path} (watched path)"));
                }
                Ok(InspectEvent::Idle { minutes, .. }) => {
                    status(&format!("No requests for {minutes} min; stopping."));
                }
                Ok(InspectEvent::Taps) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => return,
            }
        }
    })
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
    let store = store(dir)?;
    // Captures go to the app's history (when it's set up), for `teitunnel traffic`.
    let inspector = Inspector::new(
        Some(store.clone()),
        None,
        &teitunnel_core::runtime::this_process(),
    );
    let shares = QuickShares::new(
        supervisor,
        context::binary(dir),
        // Spread by pid: the app, or another terminal, may be starting a share too.
        PortAllocator::new(QUICK_SHARE_PORTS).spread(std::process::id()),
        store,
        dir.join("quick-share.yml"),
    )
    .with_edge(context::edge())
    .with_inspector(inspector.clone());
    tokio::spawn(shares.clone().watch_runtime());
    tokio::spawn(shares.clone().watch_idle());
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
    json: bool,
    host_header: &HostHeaderChoice,
    options: &ShareOptions,
    strict: bool,
) -> Result<ExitCode, String> {
    let origin = OriginUrl::parse(origin).map_err(|e| e.to_string())?;
    let dir = context::data_dir()?;
    crate::exposure::check(origin.as_str(), Some(&store(&dir)?), strict).await?;
    let (shares, owner_dir) = quick_shares(&dir).await?;
    let Some(inspector) = shares.inspector().cloned() else {
        unreachable!("quick_shares sets up the inspector");
    };
    let mut changes = shares.subscribe();
    let inspect = options.inspect || options.bearer.is_some();
    let share = shares
        .start_with(origin, stop_after, host_header, Some(inspect))
        .await
        .map_err(start_error)?;
    status(&format!("Sharing {}…", share.origin));
    announce_host_header(share.host_header.as_ref());
    let tap = TapId::new(&share.id).ok();
    let mut printer = None;
    let mut events = None;
    if share.inspected
        && let Some(tap) = &tap
    {
        configure_tap(&inspector, tap, options)?;
        if let Some(token) = &options.bearer {
            inspector
                .require_bearer(tap, token)
                .map_err(|e| e.to_string())?;
        }
        if options.log {
            printer = crate::traffic::print_requests(&inspector);
        }
        events = Some(announce_events(&inspector));
        status(
            "Requests go through Teitunnel's inspector (`teitunnel traffic`; --no-inspect turns it off).",
        );
    }

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
            None => break Ok(ExitCode::SUCCESS), // `--for` elapsed, or idle.
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
                    announce(&url, &share, stop_after, qr && !json, json)?;
                    if options.bearer.is_some() {
                        status(&format!(
                            "Callers must send `Authorization: Bearer <token>`. Streaming answers need your own domain (--on): Quick Tunnels don't carry event streams. OpenAI-compatible base URL: {url}/v1"
                        ));
                    }
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
    for task in [printer, events].into_iter().flatten() {
        task.abort();
    }
    inspector.shutdown().await;
    status("Stopped sharing.");
    outcome
}

fn announce(
    url: &str,
    share: &QuickShare,
    stop_after: Option<Duration>,
    qr: bool,
    json: bool,
) -> Result<(), String> {
    // The URL alone on stdout (or `{"url", "hostname"}` with --json), so
    // `teitunnel share 3000 | head -1` works in scripts.
    out!("{}", url_line(url, json))?;
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

/// What `share --on` needs besides the service and hostname.
#[derive(Debug, Default)]
pub(crate) struct DomainShareOptions {
    /// The account, when several are connected.
    pub(crate) account: Option<String>,
    /// Require a login.
    pub(crate) allow: Option<teitunnel_core::engine::AccessRule>,
    /// Stop by itself after this long.
    pub(crate) stop_after: Option<Duration>,
    /// Print `{"url", "hostname"}` on stdout instead of the bare URL.
    pub(crate) json: bool,
    /// Don't share when the exposure check finds a leak.
    pub(crate) strict: bool,
}

/// Exit code for a hostname someone else holds (or a DNS record Teitunnel didn't
/// create): scripts tell it from other failures.
pub(crate) const EXIT_HELD: u8 = 3;

/// Prints `message` as an error and exits with [`EXIT_HELD`].
pub(crate) fn held(message: &str) -> ExitCode {
    status(&format!("teitunnel: {message}"));
    ExitCode::from(EXIT_HELD)
}

/// The URL line a share prints on stdout: the bare URL, or JSON for scripts.
pub(crate) fn url_line(url: &str, json: bool) -> String {
    if json {
        let hostname = url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .trim_end_matches('/');
        serde_json::json!({ "url": url, "hostname": hostname }).to_string()
    } else {
        url.to_owned()
    }
}

/// Why a share on a domain was refused before anything changed: who holds the name, or
/// a record Teitunnel didn't create.
async fn refusal(
    app: &crate::context::App,
    api: &cf_api::Client,
    account: &str,
    hostname: &str,
) -> String {
    use teitunnel_core::{reservations, text::UserText as _};
    match reservations::availability(&app.engine, api, account, hostname).await {
        Ok(reservations::Availability::Held { hold }) => format!(
            "{} A share never takes a name over; choose another hostname, or ask them to release it.",
            reservations::describe(&hold).english()
        ),
        Ok(_) => {
            format!("{hostname} has a DNS record Teitunnel didn't create. Choose another hostname.")
        }
        Err(err) => err.text().english(),
    }
}

/// `teitunnel share <origin> --on <hostname>`: a temporary route on one of the
/// account's domains, through this machine's tunnel, for as long as the command runs
/// (or `--for`). The app, or an Always-on connector, serves it; with neither running
/// (a server, a CI job), this command runs the tunnel's connector itself until it ends.
/// The app also removes the route if this command dies without doing so. Inspected (the default), the route points at
/// an inspector in this process, which forwards to the service.
pub(crate) async fn run_on_domain(
    app: &crate::context::App,
    hostname: &str,
    origin: &str,
    domain: DomainShareOptions,
    host_header: &HostHeaderChoice,
    options: &ShareOptions,
    after_live: impl FnOnce(&str) -> Result<(), String>,
) -> Result<ExitCode, String> {
    use teitunnel_core::{
        domain::Hostname,
        domain_shares::{self, ShareRequest},
        engine::{Connectors as _, Outcome},
        inspect::{TapScope, TapSpec},
        runtime,
    };
    let DomainShareOptions {
        account,
        allow,
        stop_after,
        json,
        strict,
    } = domain;
    crate::exposure::check(origin, Some(app.store()), strict).await?;
    let host_header = host_header
        .resolve(origin)
        .await
        .map_err(|e| e.to_string())?;
    let account = app.account(account.as_deref()).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let probed = app.connectors(&account).await;
    let served_elsewhere = app
        .engine
        .local()
        .machine_tunnel(&account.id)
        .await
        .ok()
        .flatten()
        .is_some_and(|t| probed.is_running(&t.tunnel_id));
    // Nobody serves this machine's tunnel: this command runs its connector.
    let (machine, supervisor) = if served_elsewhere {
        (None, None)
    } else {
        let (machine, supervisor) = app.machine(false).await;
        (Some(machine), Some(supervisor))
    };
    let ctx = app.context(&account);
    let expires_at = stop_after
        .map(|d| domain_shares::now_ms() + u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    let owner = runtime::this_process();
    let hostname = hostname.trim().to_ascii_lowercase();
    let inspector = Inspector::new(
        Some(app.store().clone()),
        Some(app.secrets().clone()),
        &owner,
    );
    let inspect = options.inspect || options.bearer.is_some();
    // Inspected, the route's service is the tap, which sets the Host header itself.
    let (service, route_host_header, tap) = if inspect {
        let origin_url = OriginUrl::parse(origin).map_err(|e| e.to_string())?;
        let mut spec = TapSpec::new(
            TapScope::route(&account.id, &hostname, None),
            &hostname,
            origin_url.as_str(),
        );
        spec.public_url = Some(format!("https://{hostname}"));
        spec.host_header = host_header.as_ref().map(|h| h.value.clone());
        spec.bearer = options.bearer.iter().cloned().collect();
        let tap = inspector.start(spec).await.map_err(|e| e.to_string())?;
        configure_tap(&inspector, &tap.id, options)?;
        (tap.address.clone(), None, Some(tap.id))
    } else {
        (
            origin.to_owned(),
            host_header.as_ref().map(|h| h.value.clone()),
            None,
        )
    };
    let request = ShareRequest {
        hostname: &hostname,
        origin: &service,
        access: allow,
        expires_at,
        owner: &owner,
        host_header: route_host_header,
    };
    let started = match &machine {
        Some(machine) => domain_shares::start(&app.engine, &api, machine, ctx, request).await,
        None => domain_shares::start(&app.engine, &api, &probed, ctx, request).await,
    };
    let outcome = match started {
        Ok(outcome) => outcome,
        Err(err) => {
            inspector.shutdown().await;
            if let Some(supervisor) = supervisor {
                supervisor.stop_all().await;
            }
            return match err {
                teitunnel_core::engine::EngineError::NeedsConfirmation => {
                    Ok(held(&refusal(app, &api, &account.id, &hostname).await))
                }
                other => Err(other.to_string()),
            };
        }
    };
    match outcome {
        Outcome::Applied {
            connector_error, ..
        } => {
            let url = format!("https://{hostname}");
            out!("{}", url_line(&url, json))?;
            if let Some(error) = connector_error {
                status(&format!("Note: {}", error.english()));
            }
            let until = stop_after.map_or_else(
                || "Press Ctrl-C to stop.".to_owned(),
                |d| format!("Stops in {}, or press Ctrl-C.", describe(d)),
            );
            status(&format!("{origin} is public at {url}. {until}"));
            if machine.is_some() {
                status("This command runs the tunnel's connector until the share ends.");
            }
            announce_host_header(host_header.as_ref());
            if let Ok(host) = Hostname::parse(&hostname)
                && let Ok(result) = app
                    .engine
                    .verify(&api, ctx, &host, context::edge(), Duration::from_secs(30))
                    .await
            {
                explain(&result, Via::Domain, &mut status);
            }
            after_live(&hostname)?;
        }
        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
            inspector.shutdown().await;
            if let Some(supervisor) = supervisor {
                supervisor.stop_all().await;
            }
            return Err(error.english());
        }
    }
    let mut tasks = Vec::new();
    if tap.is_some() {
        if options.log {
            tasks.extend(crate::traffic::print_requests(&inspector));
        }
        tasks.push(announce_events(&inspector));
    }
    // An idle stop ends the command like Ctrl-C.
    let idle = {
        let mut events = inspector.subscribe();
        async move {
            loop {
                match events.recv().await {
                    Ok(teitunnel_core::inspect::InspectEvent::Idle { .. }) => return,
                    Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        std::future::pending::<()>().await;
                    }
                }
            }
        }
    };
    match stop_after {
        Some(after) => {
            tokio::select! {
                () = interrupted() => {}
                () = tokio::time::sleep(after) => {}
                () = idle => {}
            }
        }
        None => {
            tokio::select! {
                () = interrupted() => {}
                () = idle => {}
            }
        }
    }
    for task in tasks {
        task.abort();
    }
    let stopped = match &machine {
        Some(machine) => domain_shares::stop(&app.engine, &api, machine, ctx, &hostname).await,
        None => domain_shares::stop(&app.engine, &api, &probed, ctx, &hostname).await,
    };
    if let Some(supervisor) = supervisor {
        supervisor.stop_all().await;
    }
    inspector.shutdown().await;
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
