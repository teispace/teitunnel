//! `teitunnel mcp`: Teitunnel's MCP server for AI agents over stdio, and the commands
//! that connect AI clients to it (`install`, `uninstall`, `config`, `status`).
//!
//! The server uses the app's accounts, engine and database like every other command;
//! connectors keep running in the app or as Always-on services (their state is probed).
//! Shares an agent starts run in this process and end with it. Stdout carries only MCP
//! messages; anything for a person goes to stderr. While the app runs, the person
//! approves an agent's changes there, in a dialog showing the plan, and the app lists
//! the agent in Settings ▸ AI Tools ([`teitunnel_mcp::AppApprover`]).

use std::{process::ExitCode, sync::Arc};

use clap::Subcommand;
use teitunnel_core::inspect::Inspector;
use teitunnel_core::{
    engine::Engine,
    remote_logs::RemoteLogs,
    runtime::{PortAllocator, QUICK_SHARE_PORTS},
};
use teitunnel_mcp::{
    ConnectorSource, CoreBackend, CoreParts, ExposeTools, InspectorTraffic, McpServer, Mode,
    Settings, SharedBackend,
    backend::BoxFuture,
    clients::{self, Client, Paths, ServerCommand},
};
use tokio_util::sync::CancellationToken;

/// This process's inspector: its shares' captures go to the app's history (when it's
/// set up) and to agents' traffic tools.
pub(crate) fn inspector(app: &App) -> Inspector {
    Inspector::new(
        Some(app.store().clone()),
        Some(app.secrets().clone()),
        &teitunnel_core::runtime::this_process(),
    )
}

use crate::{context::App, probe::ProbedConnectors, share::status};

/// `teitunnel mcp …`
#[derive(Debug, Subcommand)]
pub(crate) enum McpCommand {
    /// Add Teitunnel to an AI client's MCP configuration (merged; a backup is kept).
    Install {
        /// claude-code, claude-desktop, cursor, vscode, codex, windsurf, zed or
        /// gemini-cli.
        client: String,
        /// The mode the client's server runs in (default: the settings file's, else ask).
        #[arg(long, value_parser = parse_mode)]
        mode: Option<Mode>,
    },
    /// Remove Teitunnel from an AI client's MCP configuration (nothing else changes).
    Uninstall {
        /// The client (see `install`).
        client: String,
    },
    /// Print the configuration to add by hand.
    Config {
        /// The client (see `install`).
        client: String,
        /// The mode to run in.
        #[arg(long, value_parser = parse_mode)]
        mode: Option<Mode>,
    },
    /// Show which AI clients are installed and connected.
    Status {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
}

pub(crate) fn parse_mode(value: &str) -> Result<Mode, String> {
    value.parse()
}

/// The command a client runs: this executable (its real path), `mcp`, and the mode.
fn server_command(mode: Option<Mode>) -> Result<ServerCommand, String> {
    let exe = std::env::current_exe()
        .and_then(std::fs::canonicalize)
        .map_err(|e| format!("Couldn't find this program's path: {e}"))?;
    let mut args = vec!["mcp".to_owned()];
    if let Some(mode) = mode {
        args.extend(["--mode".to_owned(), mode.to_string()]);
    }
    Ok(ServerCommand {
        command: exe.display().to_string(),
        args,
        env: std::collections::BTreeMap::new(),
    })
}

fn client(name: &str) -> Result<Client, String> {
    Client::parse(name).ok_or_else(|| {
        format!(
            "\"{name}\" isn't a client Teitunnel knows. Use one of: {}.",
            Client::ALL.map(Client::id).join(", ")
        )
    })
}

/// Runs a client-setup command.
pub(crate) fn setup(command: McpCommand) -> Result<ExitCode, String> {
    let paths = Paths::detect().map_err(|e| e.to_string())?;
    match command {
        McpCommand::Install { client: name, mode } => {
            let client = client(&name)?;
            let change = clients::install(client, &server_command(mode)?, &paths)
                .map_err(|e| e.to_string())?;
            if change.changed {
                out!(
                    "Connected {} to Teitunnel ({}).",
                    client.name(),
                    change.path.display()
                )?;
                if let Some(backup) = change.backup {
                    out!("The previous file is saved as {}.", backup.display())?;
                }
                out!(
                    "Restart {} (or reload its MCP servers) to use it.",
                    client.name()
                )?;
            } else {
                out!("{} is already connected to Teitunnel.", client.name())?;
            }
        }
        McpCommand::Uninstall { client: name } => {
            let client = client(&name)?;
            let change = clients::uninstall(client, &paths).map_err(|e| e.to_string())?;
            if change.changed {
                out!(
                    "Removed Teitunnel from {} ({}).",
                    client.name(),
                    change.path.display()
                )?;
            } else {
                out!("{} wasn't connected to Teitunnel.", client.name())?;
            }
        }
        McpCommand::Config { client: name, mode } => {
            let client = client(&name)?;
            status(&format!(
                "Add this to {} ({}):",
                client.name(),
                client.config_path(&paths).display()
            ));
            out!("{}", client.snippet(&server_command(mode)?))?;
        }
        McpCommand::Status { json } => {
            let all: Vec<_> = Client::ALL
                .iter()
                .map(|c| clients::status(*c, &paths))
                .collect();
            if json {
                out!(
                    "{}",
                    serde_json::to_string(&all).map_err(|e| e.to_string())?
                )?;
            } else {
                for s in &all {
                    let state = match (s.connected, s.detected, &s.problem) {
                        (_, _, Some(problem)) => format!("can't read its configuration: {problem}"),
                        (true, _, None) => "connected".to_owned(),
                        (false, true, None) => "installed, not connected".to_owned(),
                        (false, false, None) => "not found".to_owned(),
                    };
                    out!("{:<15}\t{state}", s.client.id())?;
                }
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// The app's connectors, probed (this process doesn't run them).
struct Probe {
    engine: Arc<Engine>,
    accounts: teitunnel_core::accounts::Accounts,
}

impl ConnectorSource for Probe {
    type Connectors = ProbedConnectors;

    fn connectors<'a>(&'a self, account: Option<&'a str>) -> BoxFuture<'a, ProbedConnectors> {
        Box::pin(async move {
            let mut connectors = ProbedConnectors::default();
            let accounts: Vec<String> = match account {
                Some(account) => vec![account.to_owned()],
                None => self
                    .accounts
                    .list()
                    .await
                    .unwrap_or_default()
                    .into_iter()
                    .map(|a| a.id)
                    .collect(),
            };
            for account in accounts {
                for tunnel in self
                    .engine
                    .local()
                    .tunnels(&account)
                    .await
                    .unwrap_or_default()
                {
                    connectors
                        .probe(&tunnel.tunnel_id, tunnel.metrics_port)
                        .await;
                }
            }
            connectors
        })
    }

    fn runs_connectors(&self) -> bool {
        false
    }
}

/// The MCP server's settings: the data folder's `mcp.json`, the environment, then flags.
pub(crate) fn settings(
    dir: &std::path::Path,
    mode: Option<Mode>,
    allow_secrets: bool,
) -> Result<Settings, String> {
    Ok(Settings::load(dir, |name| std::env::var(name).ok())?.with_flags(mode, allow_secrets))
}

/// The backend over `app`, with connector state from `source`.
pub(crate) fn backend<S: ConnectorSource>(
    app: &App,
    source: S,
    machine: teitunnel_core::machine::MachineTunnels,
    supervisor: teitunnel_core::runtime::Supervisor,
    inspector: &Inspector,
) -> SharedBackend {
    let quick_shares = teitunnel_core::quick_share::QuickShares::new(
        supervisor,
        app.binary.clone(),
        PortAllocator::new(QUICK_SHARE_PORTS).spread(std::process::id()),
        app.store().clone(),
        app.dir().join("quick-share.yml"),
    )
    .with_inspector(inspector.clone());
    tokio::spawn(quick_shares.clone().watch_idle());
    CoreBackend::new(
        CoreParts {
            accounts: app.accounts.clone(),
            engine: Arc::clone(&app.engine),
            store: app.store().clone(),
            binary: app.binary.clone(),
            machine_name: app.machine_name.clone(),
            machine,
            quick_shares,
            runs: app.dir().join("run-cli"),
            edge: crate::context::edge(),
            remote_logs: RemoteLogs::default(),
            pauses: Arc::new(teitunnel_core::pause::Enforcer::new()),
        },
        source,
    )
}

/// Resolves on SIGTERM or Ctrl-C (Unix: also SIGHUP).
async fn signal() {
    crate::share::interrupted().await;
}

/// `teitunnel mcp`: serves over stdio until the client disconnects.
///
/// The app's "Stop" on one of this server's shares sends SIGTERM (as it does to a
/// terminal's `teitunnel share`): the shares stop and the server keeps serving its
/// agent. A signal with no share running ends the server.
pub(crate) async fn serve(mode: Option<Mode>, allow_secrets: bool) -> Result<ExitCode, String> {
    let app = App::open_or_empty().await?;
    let settings = settings(app.dir(), mode, allow_secrets)?;
    let (machine, supervisor) = app.machine(true).await;
    let source = Probe {
        engine: Arc::clone(&app.engine),
        accounts: app.accounts.clone(),
    };
    let inspector = inspector(&app);
    let backend = backend(
        &app,
        source,
        machine.clone(),
        supervisor.clone(),
        &inspector,
    );
    let server = McpServer::builder(Arc::clone(&backend), settings.clone())
        .traffic(Arc::new(InspectorTraffic::new(
            inspector.clone(),
            settings.allow_secrets,
        )))
        .provider(Arc::new(
            teitunnel_mcp::reservations::ReservationTools::new(Arc::clone(&backend)),
        ))
        .provider(Arc::new(teitunnel_mcp::CommentsTools::new(Arc::clone(
            &backend,
        ))))
        .provider(Arc::new(ExposeTools::new(
            Arc::clone(&backend),
            inspector.clone(),
        )))
        .approver(teitunnel_mcp::AppApprover::new(app.dir(), settings.mode))
        .build();
    status(&format!(
        "Teitunnel MCP server ({} mode) on stdio. Connect an AI client with `teitunnel mcp install <client>`.",
        settings.mode
    ));
    // Pauses and schedules of the shares on a domain this server starts.
    let share_loop = crate::sharing::spawn_share_loop(app.store().clone(), inspector.clone());
    // What killed agents and terminals left behind, while the app isn't running.
    let sweeper = crate::inspect::sweep_left_behind(&app, machine.clone());
    let stop = CancellationToken::new();
    let serving = tokio::spawn(teitunnel_mcp::serve_stdio(server, stop.clone()));
    tokio::pin!(serving);
    let result = loop {
        tokio::select! {
            result = &mut serving => break result.map_err(|e| e.to_string()).and_then(|r| r),
            () = signal() => {
                let stopped = backend.stop_own_shares().await;
                if stopped == 0 {
                    stop.cancel();
                } else {
                    status(&format!("Stopped {stopped} share(s); still serving."));
                }
            }
        }
    };
    share_loop.abort();
    sweeper.abort();
    backend.stop_own_shares().await;
    supervisor.stop_all().await;
    inspector.shutdown().await;
    result.map(|()| ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_the_command_clients_run() {
        let command = server_command(Some(Mode::ReadOnly)).unwrap();
        assert_eq!(command.args, ["mcp", "--mode", "read-only"]);
        assert!(std::path::Path::new(&command.command).is_absolute());
        assert_eq!(server_command(None).unwrap().args, ["mcp"]);
        assert!(client("cursor").is_ok());
        assert!(client("emacs").unwrap_err().contains("claude-code"));
    }
}
