//! `teitunnel up` and `always-on`: running this machine's tunnels without the app,
//! for servers and containers (M10-04, D-069).

use std::{process::ExitCode, time::Duration};

use teitunnel_core::{engine::Connectors as _, machine::MachineTunnels, runtime::ConnectorState};

use crate::{context::App, share::interrupted};

fn status(message: &str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

fn describe(state: Option<&ConnectorState>) -> &'static str {
    match state {
        Some(ConnectorState::Healthy { .. }) => "connected",
        Some(ConnectorState::Starting | ConnectorState::Connecting) => "connecting",
        Some(ConnectorState::Degraded) => "connection lost, reconnecting",
        Some(ConnectorState::Crashed { .. }) => "stopped unexpectedly, restarting",
        Some(ConnectorState::CrashLoop { .. }) => "keeps stopping",
        Some(ConnectorState::Stopping) => "stopping",
        Some(ConnectorState::Stopped) | None => "stopped",
    }
}

/// `teitunnel up`'s options.
#[derive(Debug, Default, Clone, clap::Args)]
pub(crate) struct UpArgs {
    /// The project file to apply first (default: teitunnel.yml in this folder or the
    /// nearest one above it in the repository, when there is one).
    #[arg(long, short = 'f', value_name = "PATH", conflicts_with = "no_project")]
    pub(crate) file: Option<std::path::PathBuf>,
    /// Don't apply a project file, even if there is one here.
    #[arg(long)]
    pub(crate) no_project: bool,
    #[command(flatten)]
    pub(crate) apply: crate::project::ApplyOptions,
}

/// The project to apply, if any: `--file`, else the one for this folder.
fn project_for(args: &UpArgs) -> Result<Option<teitunnel_core::project::Loaded>, String> {
    if args.no_project {
        return Ok(None);
    }
    if let Some(file) = &args.file {
        return crate::project::load(Some(file)).map(Some);
    }
    let here = std::env::current_dir().map_err(|e| e.to_string())?;
    match teitunnel_core::project::find(&here) {
        Some(path) => crate::project::load(Some(&path)).map(Some),
        None => Ok(None),
    }
}

/// Runs this machine's tunnels (every account's) in the foreground until interrupted:
/// the entrypoint for a container, or a unit of any process supervisor. Tunnels that
/// are Always-on run as their own service and are left to it. With a project file
/// (`teitunnel.yml`) here, it's applied first (its plan shown and confirmed) and its
/// shares run as long as this does.
pub(crate) async fn up(app: &App, args: &UpArgs) -> Result<ExitCode, String> {
    let applied = match project_for(args)? {
        Some(loaded) => {
            status(&format!("Project file: {}", loaded.path.display()));
            crate::project::apply(app, &loaded, &args.apply).await?
        }
        None => None,
    };
    let (machine, supervisor) = app.machine(false).await;
    let mut running: Vec<(String, String)> = Vec::new();
    for account in app.accounts.list().await.map_err(|e| e.to_string())? {
        let tunnels = app
            .engine
            .local()
            .tunnels(&account.id)
            .await
            .map_err(|e| e.to_string())?;
        if tunnels.is_empty() {
            continue;
        }
        let api = app
            .accounts
            .client(&account.id)
            .await
            .map_err(|e| e.to_string())?;
        if let Err(message) = machine.resume(&api, &account.id).await {
            status(&format!("{}: {}", account.name, message.english()));
        }
        for tunnel in tunnels {
            if tunnel.always_on {
                status(&format!(
                    "{} runs as a service already (Always-on); leaving it to it.",
                    tunnel.name
                ));
            } else {
                running.push((tunnel.tunnel_id, tunnel.name));
            }
        }
    }
    let has_shares = applied.as_ref().is_some_and(|a| !a.shares.is_empty());
    if running.is_empty() && !has_shares {
        return Err(
            "This machine has no tunnel to run. Add a route first (`teitunnel route add …`)."
                .into(),
        );
    }
    let shares = match applied {
        Some(applied) if has_shares => {
            match crate::project::start_shares(app, applied, &machine, args.apply.strict).await {
                Ok(shares) => Some(shares),
                Err(message) => {
                    supervisor.stop_all().await;
                    return Err(message);
                }
            }
        }
        _ => None,
    };
    if !running.is_empty() {
        status(&format!(
            "Running {} tunnel(s): {}. Press Ctrl-C to stop.",
            running.len(),
            running
                .iter()
                .map(|(_, name)| name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let sweeper = crate::inspect::sweep_left_behind(app, machine.clone());
    // Paused pages and schedules of this machine's routes, while the app isn't doing it.
    let inspector = crate::mcp::inspector(app);
    let route_host = crate::sharing::RouteHost::spawn(app, machine.clone(), inspector.clone());
    let report = |machine: &MachineTunnels, last: &mut Vec<&'static str>| {
        for (index, (id, name)) in running.iter().enumerate() {
            let now = describe(machine.state(id).as_ref());
            if last.get(index) != Some(&now) {
                status(&format!("{name}: {now}"));
            }
            if let Some(slot) = last.get_mut(index) {
                *slot = now;
            } else {
                last.push(now);
            }
        }
    };
    let monitor =
        crate::analytics::spawn_monitor(app, teitunnel_core::analytics::Analytics::default());
    let mut last = Vec::new();
    let stop = interrupted();
    tokio::pin!(stop);
    loop {
        report(&machine, &mut last);
        tokio::select! {
            () = &mut stop => break,
            () = tokio::time::sleep(Duration::from_secs(2)) => {}
        }
    }
    status("Stopping…");
    if let Some(shares) = shares {
        shares.stop(app).await;
    }
    sweeper.abort();
    route_host.stop(app, &machine, &inspector).await;
    inspector.shutdown().await;
    monitor.release().await;
    supervisor.stop_all().await;
    Ok(ExitCode::SUCCESS)
}

/// What `always-on` should do.
#[derive(Debug, Clone, Copy)]
pub(crate) enum AlwaysOn {
    On,
    Off,
    Status,
}

/// Installs, removes or shows the OS service running one of this machine's tunnels. As
/// root on a Linux server the service is a system unit (starts at boot, sandboxed);
/// otherwise the user's (launchd, systemd --user, Task Scheduler).
pub(crate) async fn always_on(
    app: &App,
    action: AlwaysOn,
    account: Option<&str>,
    tunnel: Option<&str>,
) -> Result<ExitCode, String> {
    let account = app.account(account).await?;
    let (machine, _supervisor) = app.machine(true).await;
    if !machine.supports_always_on() {
        return Err("No service manager is available here. Run `teitunnel up` under your process supervisor (Docker, runit, …) instead.".into());
    }
    let tunnels = app
        .engine
        .local()
        .tunnels(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let chosen: Vec<_> = match tunnel {
        Some(name) => {
            let found = tunnels
                .iter()
                .find(|t| t.name.eq_ignore_ascii_case(name) || t.tunnel_id == name)
                .ok_or_else(|| format!("This machine has no tunnel named “{name}”."))?;
            vec![found.clone()]
        }
        None => tunnels.clone(),
    };
    if chosen.is_empty() {
        return Err("This machine has no tunnel yet. Add a route first.".into());
    }
    match action {
        AlwaysOn::Status => {
            for t in &chosen {
                let state = machine.service_state(&t.tunnel_id).await;
                let running = state.as_ref().is_some_and(|s| s.pid.is_some());
                out!(
                    "{}\t{}",
                    t.name,
                    match (t.always_on, running) {
                        (true, true) => "always on, running",
                        (true, false) => "always on, not running",
                        (false, _) => "runs with the app",
                    }
                )?;
            }
        }
        AlwaysOn::On => {
            let api = app
                .accounts
                .client(&account.id)
                .await
                .map_err(|e| e.to_string())?;
            for t in &chosen {
                out!("Turning Always-on on for {}…", t.name)?;
                machine
                    .set_always_on(&api, &account.id, Some(&t.tunnel_id), true)
                    .await
                    .map_err(|e| e.english())?;
            }
            out!("Done: the connectors run as a service and start again after a restart.")?;
            if cfg!(target_os = "linux") && !teitunnel_core::service::is_root() {
                out!(
                    "Note: user services stop at logout unless lingering is on (`loginctl enable-linger`)."
                )?;
            }
        }
        AlwaysOn::Off => {
            for t in &chosen {
                machine
                    .disable_service(&account.id, Some(&t.tunnel_id))
                    .await
                    .map_err(|e| e.english())?;
                out!(
                    "{}: the service is removed; its routes are served while the app or `teitunnel up` runs.",
                    t.name
                )?;
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
