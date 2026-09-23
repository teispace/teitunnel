//! `teitunnel-cli`: Teitunnel's routes from the terminal. It uses the app's accounts,
//! keychain and database, and makes every change through the same plan → apply engine,
//! showing the plan before applying it. It never runs connectors: the app, or an
//! Always-on service, serves the routes.

mod context;
mod probe;

use std::{
    io::{self, BufRead, IsTerminal, Write},
    process::ExitCode,
    time::Duration,
};

use clap::{Parser, Subcommand, ValueEnum};
use teitunnel_core::{
    domain::Hostname,
    engine::{Approval, Change, Edge, Outcome, Plan, RouteInput, StepState, Warning},
    export::{ExportFormat, render},
};

use crate::context::App;

/// How long a fresh route may take to start answering.
const VERIFY_PATIENCE: Duration = Duration::from_secs(30);

#[derive(Debug, Parser)]
#[command(
    name = "teitunnel-cli",
    version,
    about = "Manage Teitunnel routes from the terminal.",
    long_about = "Manage Teitunnel routes from the terminal. Uses the accounts connected in the Teitunnel app; every change is shown before it's applied."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List connected Cloudflare accounts.
    Accounts {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// List this Mac's routes and their status.
    Routes {
        /// Account name or id (needed when several are connected).
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Add, or remove, a route.
    #[command(subcommand)]
    Route(RouteCommand),
    /// Print this Mac's tunnel and routes as config.yml, Docker Compose or Terraform.
    Export {
        /// What to export as.
        #[arg(value_enum)]
        format: Format,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum RouteCommand {
    /// Route a hostname to a service on this Mac, e.g. `app.example.com 3000`.
    Add {
        /// Public hostname on one of the account's domains.
        hostname: String,
        /// Where traffic goes: a port (`3000`), `host:port`, or a URL.
        origin: String,
        /// Only requests whose path matches this regex, e.g. `^/api`.
        #[arg(long)]
        path: Option<String>,
        #[command(flatten)]
        apply: ApplyArgs,
    },
    /// Remove a route (and its DNS record, if Teitunnel created it).
    Remove {
        /// Hostname.
        hostname: String,
        /// The route's path rule, if it has one.
        #[arg(long)]
        path: Option<String>,
        #[command(flatten)]
        apply: ApplyArgs,
    },
}

#[derive(Debug, clap::Args)]
struct ApplyArgs {
    /// Account name or id.
    #[arg(long, short)]
    account: Option<String>,
    /// Apply without asking.
    #[arg(long, short)]
    yes: bool,
    /// Also allow replacing or deleting DNS records Teitunnel didn't create.
    #[arg(long)]
    replace: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Format {
    ConfigYaml,
    DockerCompose,
    Terraform,
}

impl From<Format> for ExportFormat {
    fn from(format: Format) -> Self {
        match format {
            Format::ConfigYaml => Self::ConfigYaml,
            Format::DockerCompose => Self::DockerCompose,
            Format::Terraform => Self::Terraform,
        }
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.command).await {
        Ok(code) => code,
        Err(message) => {
            let _ = writeln!(io::stderr().lock(), "teitunnel-cli: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Writes a line to stdout; a write error (e.g. a closed pipe) ends the command.
macro_rules! out {
    ($($arg:tt)*) => {
        writeln!(io::stdout().lock(), $($arg)*).map_err(|e| e.to_string())
    };
}

async fn run(command: Command) -> Result<ExitCode, String> {
    let app = App::open()?;
    match command {
        Command::Accounts { json } => accounts(&app, json).await,
        Command::Routes { account, json } => routes(&app, account.as_deref(), json).await,
        Command::Route(RouteCommand::Add {
            hostname,
            origin,
            path,
            apply,
        }) => {
            let change = Change::AddRoute {
                route: RouteInput {
                    hostname,
                    path,
                    origin,
                },
            };
            change_routes(&app, change, &apply).await
        }
        Command::Route(RouteCommand::Remove {
            hostname,
            path,
            apply,
        }) => change_routes(&app, Change::RemoveRoute { hostname, path }, &apply).await,
        Command::Export { format, account } => export(&app, format, account.as_deref()).await,
    }
}

async fn accounts(app: &App, json: bool) -> Result<ExitCode, String> {
    let accounts = app.accounts.list().await.map_err(|e| e.to_string())?;
    if json {
        let list: Vec<_> = accounts
            .iter()
            .map(|a| serde_json::json!({ "id": a.id, "name": a.name }))
            .collect();
        out!("{}", serde_json::Value::Array(list))?;
    } else if accounts.is_empty() {
        out!("No Cloudflare account is connected. Connect one in Teitunnel.")?;
    } else {
        for account in &accounts {
            out!("{}\t{}", account.name, account.id)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn routes(app: &App, account: Option<&str>, json: bool) -> Result<ExitCode, String> {
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let connectors = app.connectors(&account).await;
    let overview = app
        .engine
        .overview(&api, &connectors, app.context(&account))
        .await
        .map_err(|e| e.to_string())?;
    let statuses = overview.statuses();
    if json {
        let list: Vec<_> = overview
            .routes
            .iter()
            .zip(&statuses)
            .map(|(route, (_, status))| {
                serde_json::json!({
                    "hostname": route.hostname,
                    "path": route.path,
                    "origin": route.origin,
                    "status": status,
                })
            })
            .collect();
        out!("{}", serde_json::Value::Array(list))?;
        return Ok(ExitCode::SUCCESS);
    }
    if overview.routes.is_empty() {
        out!("No routes on this Mac in {}.", account.name)?;
    }
    for (route, (_, status)) in overview.routes.iter().zip(&statuses) {
        let path = route
            .path
            .as_deref()
            .map(|p| format!(" {p}"))
            .unwrap_or_default();
        out!("{}{path}\t{}\t{status}", route.hostname, route.origin)?;
    }
    Ok(ExitCode::SUCCESS)
}

fn warning_text(warning: &Warning) -> String {
    match warning {
        Warning::ReplacesForeignRecord {
            hostname,
            kind,
            content,
        } => format!(
            "{hostname} already has {} {kind} record ({content}) that Teitunnel didn't create. It will be replaced.",
            if kind.starts_with(['A', 'E', 'I', 'O', 'U']) {
                "an"
            } else {
                "a"
            }
        ),
        Warning::DeletesForeignRecord {
            hostname,
            kind,
            content,
        } => format!(
            "The {kind} record for {hostname} ({content}) wasn't created by Teitunnel. It will be deleted."
        ),
        Warning::KeepsForeignRecord { hostname } => format!(
            "The DNS record for {hostname} wasn't created by Teitunnel, so it's left in place."
        ),
        Warning::TunnelEmpty => {
            "No routes will be left. The tunnel stays, so adding a route later is quick.".into()
        }
        Warning::RemoteOrigin { origin } => {
            format!("{origin} isn't on this Mac. It must be reachable from here.")
        }
    }
}

fn print_plan(plan: &Plan, account_id: &str) -> Result<(), String> {
    let view = plan.view(account_id);
    for warning in &view.warnings {
        out!("! {}", warning_text(warning))?;
    }
    for (index, step) in view.steps.iter().enumerate() {
        out!("{:>2}. {}", index + 1, step.description)?;
    }
    Ok(())
}

/// Asks before applying, unless `--yes`. Without a terminal to ask on, `--yes` is needed.
fn confirm(apply: &ApplyArgs) -> Result<bool, String> {
    if apply.yes {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        return Err("Not a terminal, so nothing to ask. Pass --yes to apply.".into());
    }
    write!(io::stdout().lock(), "Apply? [y/N] ").map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut answer = String::new();
    io::stdin()
        .lock()
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    Ok(matches!(answer.trim(), "y" | "Y" | "yes" | "Yes"))
}

async fn change_routes(app: &App, change: Change, apply: &ApplyArgs) -> Result<ExitCode, String> {
    let account = app.account(apply.account.as_deref()).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let ctx = app.context(&account);
    let intent = app
        .engine
        .intent_for(&api, ctx, &change)
        .await
        .map_err(|e| e.to_string())?;
    let plan = app
        .engine
        .preview(&api, ctx, &intent)
        .await
        .map_err(|e| e.to_string())?;
    if plan.is_empty() {
        out!("Nothing to change.")?;
        return Ok(ExitCode::SUCCESS);
    }
    print_plan(&plan, &account.id)?;
    if plan.requires_confirmation && !apply.replace {
        return Err(
            "This changes DNS records Teitunnel didn't create. Pass --replace to allow it.".into(),
        );
    }
    if !confirm(apply)? {
        out!("Nothing changed.")?;
        return Ok(ExitCode::SUCCESS);
    }

    let connectors = app.connectors(&account).await;
    let steps = plan.view(&account.id).steps;
    let approval = Approval {
        fingerprint: &plan.fingerprint,
        confirmed: apply.replace,
    };
    let outcome = app
        .engine
        .apply(&api, &connectors, ctx, &intent, approval, |progress| {
            let Some(step) = steps.get(usize::try_from(progress.step).unwrap_or(usize::MAX)) else {
                return;
            };
            let mark = match progress.state {
                StepState::Done => "done",
                StepState::Failed { .. } => "failed",
                StepState::Undone => "undone",
                StepState::UndoFailed { .. } => "couldn't undo",
                _ => return,
            };
            let _ = writeln!(io::stdout().lock(), "    {mark}: {}", step.description);
        })
        .await
        .map_err(|e| e.to_string())?;

    match outcome {
        Outcome::Applied {
            verify,
            connector_error,
            ..
        } => {
            if let Some(error) = connector_error {
                out!("Note: {error}")?;
            }
            let mut ok = true;
            for hostname in verify {
                ok &= check(app, &api, &account, &hostname).await?;
            }
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Outcome::RolledBack { error, .. } => {
            out!("Failed: {error}. Everything was undone.")?;
            Ok(ExitCode::FAILURE)
        }
        Outcome::PartiallyApplied {
            error, leftovers, ..
        } => {
            out!("Failed: {error}. These couldn't be undone:")?;
            for leftover in leftovers {
                out!("  - {leftover}")?;
            }
            Ok(ExitCode::FAILURE)
        }
    }
}

/// Checks a hostname end to end through Cloudflare's edge; `true` if it works.
async fn check(
    app: &App,
    api: &cf_api::Client,
    account: &teitunnel_core::accounts::Account,
    hostname: &str,
) -> Result<bool, String> {
    let host = Hostname::parse(hostname).map_err(|e| e.to_string())?;
    out!("Checking https://{hostname}…")?;
    let result = app
        .engine
        .verify(
            api,
            app.context(account),
            &host,
            Edge::Cloudflare,
            VERIFY_PATIENCE,
        )
        .await
        .map_err(|e| e.to_string())?;
    match (&result.failure, &result.message) {
        (None, _) => {
            out!("https://{hostname} works.")?;
            Ok(true)
        }
        (Some(_), Some(message)) => {
            out!("https://{hostname} doesn't work yet: {message}")?;
            Ok(false)
        }
        (Some(failure), None) => {
            out!("https://{hostname} doesn't work yet: {failure:?}")?;
            Ok(false)
        }
    }
}

async fn export(app: &App, format: Format, account: Option<&str>) -> Result<ExitCode, String> {
    let account = app.account(account).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let input = app
        .engine
        .export_input(&api, app.context(&account), None)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| format!("This Mac has no routes in {} yet.", account.name))?;
    let file = render(&input, format.into());
    write!(io::stdout().lock(), "{}", file.contents).map_err(|e| e.to_string())?;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;

    #[test]
    fn the_command_line_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_a_route_add() {
        let cli = Cli::try_parse_from([
            "teitunnel-cli",
            "route",
            "add",
            "app.example.com",
            "3000",
            "--path",
            "^/api",
            "--yes",
        ])
        .unwrap_or_else(|e| unreachable!("{e}"));
        let Command::Route(RouteCommand::Add {
            hostname,
            origin,
            path,
            apply,
        }) = cli.command
        else {
            unreachable!()
        };
        assert_eq!(
            (hostname.as_str(), origin.as_str(), path.as_deref()),
            ("app.example.com", "3000", Some("^/api"))
        );
        assert!(apply.yes && !apply.replace);
    }

    #[test]
    fn describes_every_warning() {
        let text = warning_text(&Warning::ReplacesForeignRecord {
            hostname: "a.xyz.com".into(),
            kind: "A".into(),
            content: "192.0.2.1".into(),
        });
        assert!(text.starts_with("a.xyz.com already has an A record"));
    }
}
