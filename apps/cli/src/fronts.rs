//! `teitunnel offline` and `teitunnel inbox`: Workers on your Cloudflare account in
//! front of a route, for while this computer is off. The offline page replaces
//! Cloudflare's error 1033 with your own; a webhook inbox keeps webhooks and this
//! computer delivers them in order when it's back. Changes are shown as a plan first.

use std::{
    io::{self, Write},
    process::ExitCode,
};

use clap::{Args, Subcommand, ValueEnum};
use teitunnel_core::{
    accounts::Account,
    engine::{
        Approval, Outcome, StepState,
        front::{FrontKind, InboxSettings, InboxVerify, OfflinePage},
    },
    fronts::{self, FrontChange},
    text::UserText,
};

use teitunnel_core::inspect::lens::webhook::Provider;

use crate::context::App;

/// Shared by every change.
#[derive(Debug, Default, Args)]
pub(crate) struct ChangeArgs {
    /// Account name or id.
    #[arg(long, short)]
    account: Option<String>,
    /// Apply without asking.
    #[arg(long, short)]
    yes: bool,
}

/// `teitunnel offline <hostname>`.
#[derive(Debug, Args)]
pub(crate) struct OfflineArgs {
    /// The route's hostname, e.g. `app.example.com`.
    hostname: String,
    /// The page's title (turns the page on).
    #[arg(long)]
    title: Option<String>,
    /// A line or two for visitors.
    #[arg(long)]
    message: Option<String>,
    /// Also show it when the tunnel is up but the app doesn't answer (502/504).
    #[arg(long)]
    when_app_down: bool,
    /// Remove the offline page.
    #[arg(long, conflicts_with_all = ["title", "message", "when_app_down"])]
    off: bool,
    #[command(flatten)]
    change: ChangeArgs,
}

/// Signatures an inbox can check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Verify {
    /// GitHub (`X-Hub-Signature-256`).
    Github,
    /// Stripe (`Stripe-Signature`).
    Stripe,
    /// Standard Webhooks (Svix, Clerk, Resend…).
    Standard,
}

#[derive(Debug, Subcommand)]
pub(crate) enum InboxCommand {
    /// Keep webhooks to a path while this computer is off; they're delivered in order
    /// when it's back.
    Add {
        /// The route's hostname.
        hostname: String,
        /// The path, e.g. `/webhooks/`.
        path: String,
        /// Most webhooks kept waiting.
        #[arg(long, default_value_t = 500)]
        max: u32,
        /// Days a webhook is kept.
        #[arg(long, default_value_t = 7)]
        days: u32,
        /// Keep only webhooks signed with the secret saved for this hostname
        /// (`teitunnel inbox secret`, or the app's inspector).
        #[arg(long, value_enum)]
        verify: Option<Verify>,
        #[command(flatten)]
        change: ChangeArgs,
    },
    /// Remove an inbox (webhooks still waiting stay until their retention ends).
    Rm {
        /// The hostname.
        hostname: String,
        /// The path.
        path: String,
        #[command(flatten)]
        change: ChangeArgs,
    },
    /// Offline pages and inboxes on this computer's records.
    Ls {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// An inbox's recent webhooks: when each arrived and when it was delivered.
    Items {
        /// The hostname.
        hostname: String,
        /// The path.
        path: String,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Save a webhook signing secret for a hostname in the keychain (read from
    /// TEITUNNEL_WEBHOOK_SECRET or standard input, never the command line). A verifying
    /// inbox sends it to Cloudflare as a Worker secret; the inspector checks signatures
    /// with it.
    Secret {
        /// The hostname.
        hostname: String,
        /// Who signs the webhooks.
        #[arg(value_enum)]
        provider: Verify,
    },
    /// Deliver waiting webhooks now (the app does every 30 seconds).
    Deliver {
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
    },
}

fn page_of(args: &OfflineArgs, current: Option<OfflinePage>) -> OfflinePage {
    let mut page = current.unwrap_or_default();
    if let Some(title) = &args.title {
        page.title.clone_from(title);
    }
    if let Some(message) = &args.message {
        page.message.clone_from(message);
    }
    if args.when_app_down {
        page.when_app_down = true;
    }
    page
}

/// `teitunnel offline`.
pub(crate) async fn offline(app: &App, args: OfflineArgs) -> Result<ExitCode, String> {
    let account = app.account(args.change.account.as_deref()).await?;
    let host = args.hostname.to_ascii_lowercase();
    let current = fronts::list(&app.engine, Some(&account.id))
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|f| f.hostname == host && f.kind == FrontKind::Offline);
    let changes = args.off || args.title.is_some() || args.message.is_some() || args.when_app_down;
    if !changes {
        match current {
            Some(front) => {
                let page = front.page.unwrap_or_default();
                out!("{host}: offline page on")?;
                out!("  title:   {}", page.title)?;
                out!("  message: {}", page.message)?;
                out!(
                    "  also when the app doesn't answer: {}",
                    if page.when_app_down { "yes" } else { "no" }
                )?;
            }
            None => out!(
                "{host} has no offline page. Turn it on with `teitunnel offline {host} --title \"Back soon\"`."
            )?,
        }
        return Ok(ExitCode::SUCCESS);
    }
    let change = FrontChange::Offline {
        hostname: host,
        page: (!args.off).then(|| page_of(&args, current.and_then(|f| f.page))),
    };
    Ok(exit(apply(app, &account, &change, &args.change).await?))
}

/// `teitunnel inbox …`.
pub(crate) async fn inbox(app: &App, command: InboxCommand) -> Result<ExitCode, String> {
    match command {
        InboxCommand::Add {
            hostname,
            path,
            max,
            days,
            verify,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let request = FrontChange::Inbox {
                hostname,
                path,
                inbox: Some(InboxSettings {
                    max_items: max,
                    retention_days: days,
                    verify: verify.map(|v| match v {
                        Verify::Github => InboxVerify::Github,
                        Verify::Stripe => InboxVerify::Stripe,
                        Verify::Standard => InboxVerify::Standard,
                    }),
                }),
            };
            Ok(exit(apply(app, &account, &request, &change).await?))
        }
        InboxCommand::Rm {
            hostname,
            path,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let request = FrontChange::Inbox {
                hostname,
                path,
                inbox: None,
            };
            Ok(exit(apply(app, &account, &request, &change).await?))
        }
        InboxCommand::Ls { json } => {
            let list = fronts::list(&app.engine, None)
                .await
                .map_err(|e| e.to_string())?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string_pretty(&list).map_err(|e| e.to_string())?
                )?;
            } else if list.is_empty() {
                out!("No offline pages or webhook inboxes.")?;
            } else {
                for front in list {
                    let what = match front.kind {
                        FrontKind::Offline => "offline page".to_owned(),
                        FrontKind::Inbox => format!("inbox {}", front.path),
                    };
                    let state = if front.routed { "" } else { "\t(no route)" };
                    out!("{}\t{what}\t{}{state}", front.hostname, front.script)?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        InboxCommand::Items {
            hostname,
            path,
            account,
            json,
        } => {
            let account = app.account(account.as_deref()).await?;
            let host = hostname.to_ascii_lowercase();
            let Some(inbox) = fronts::list(&app.engine, Some(&account.id))
                .await
                .map_err(|e| e.to_string())?
                .into_iter()
                .find(|f| f.hostname == host && f.path == path && f.kind == FrontKind::Inbox)
            else {
                return Err(format!(
                    "{host} has no inbox at {path}. See `teitunnel inbox ls`."
                ));
            };
            let Some(database) = app
                .engine
                .local()
                .cloud_database(&account.id)
                .await
                .map_err(|e| e.to_string())?
            else {
                out!("Nothing kept yet.")?;
                return Ok(ExitCode::SUCCESS);
            };
            let api = app
                .accounts
                .client(&account.id)
                .await
                .map_err(|e| e.to_string())?;
            let items =
                teitunnel_core::inbox::items(&api, &account.id, &database, &inbox.script, 100)
                    .await
                    .map_err(|e| e.text().english())?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string_pretty(&items).map_err(|e| e.to_string())?
                )?;
            } else if items.is_empty() {
                out!("Nothing kept yet.")?;
            } else {
                for item in items {
                    let state = match (item.delivered_at, item.status, &item.error) {
                        (Some(_), Some(status), _) => format!("delivered ({status})"),
                        (_, _, Some(error)) => format!("waiting: {error}"),
                        _ => "waiting".to_owned(),
                    };
                    out!(
                        "{}\t{} {}\t{} bytes\t{state}",
                        item.id,
                        item.method,
                        item.path,
                        item.size
                    )?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        InboxCommand::Secret { hostname, provider } => {
            let secret = read_secret()?;
            if secret.is_empty() {
                return Err("The secret is empty.".into());
            }
            let provider = match provider {
                Verify::Github => Provider::GitHub,
                Verify::Stripe => Provider::Stripe,
                Verify::Standard => Provider::StandardWebhooks,
            };
            let host = hostname.to_ascii_lowercase();
            teitunnel_core::inspect::secrets::set_webhook_secret(
                app.secrets(),
                &format!("host:{host}"),
                provider,
                teitunnel_core::Secret::new(secret),
            )
            .await
            .map_err(|e| e.to_string())?;
            out!("Saved in the keychain for {host}.")?;
            Ok(ExitCode::SUCCESS)
        }
        InboxCommand::Deliver { account } => {
            let account = app.account(account.as_deref()).await?;
            let api = app
                .accounts
                .client(&account.id)
                .await
                .map_err(|e| e.to_string())?;
            let http = teitunnel_core::inbox::client().map_err(|e| e.to_string())?;
            let reports =
                teitunnel_core::inbox::drain_account(&app.engine, &api, &http, &account.id)
                    .await
                    .map_err(|e| e.to_string())?;
            if reports.is_empty() {
                out!("No inbox on a route this computer serves.")?;
            }
            for report in reports {
                let error = report
                    .error
                    .map(|e| format!(" (stopped: {e})"))
                    .unwrap_or_default();
                out!(
                    "{}{}\t{} delivered, {} waiting{error}",
                    report.hostname,
                    report.path,
                    report.delivered,
                    report.waiting
                )?;
            }
            Ok(ExitCode::SUCCESS)
        }
    }
}

/// The secret from `TEITUNNEL_WEBHOOK_SECRET` or one line of standard input.
fn read_secret() -> Result<String, String> {
    use std::io::{BufRead, IsTerminal};
    if let Ok(secret) = std::env::var("TEITUNNEL_WEBHOOK_SECRET") {
        return Ok(secret.trim().to_owned());
    }
    if io::stdin().is_terminal() {
        write!(io::stdout().lock(), "Signing secret: ").map_err(|e| e.to_string())?;
        io::stdout().flush().map_err(|e| e.to_string())?;
    }
    let mut secret = String::new();
    io::stdin()
        .lock()
        .read_line(&mut secret)
        .map_err(|e| e.to_string())?;
    Ok(secret.trim().to_owned())
}

fn exit(applied: bool) -> ExitCode {
    if applied {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Previews, asks, applies and reports a change; `true` when it was applied.
async fn apply(
    app: &App,
    account: &Account,
    change: &FrontChange,
    args: &ChangeArgs,
) -> Result<bool, String> {
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let ctx = app.context(account);
    let secrets = app.secrets();
    let plan = fronts::preview(&app.engine, &api, Some(secrets), ctx, change)
        .await
        .map_err(|e| e.text().english())?;
    if plan.steps.is_empty() {
        out!("Nothing to change.")?;
        return Ok(true);
    }
    for warning in &plan.warnings {
        out!("! {}", crate::warning_text(warning))?;
    }
    for (index, step) in plan.steps.iter().enumerate() {
        out!("{:>2}. {}", index + 1, step.description.english())?;
    }
    if !args.yes && !crate::confirm("Apply?")? {
        out!("Nothing changed.")?;
        return Ok(false);
    }
    let connectors = app.connectors(account).await;
    let steps = plan.steps.clone();
    let outcome = fronts::apply(
        &app.engine,
        &api,
        &connectors,
        Some(secrets),
        ctx,
        change,
        Approval {
            fingerprint: &plan.fingerprint,
            confirmed: false,
        },
        |progress| {
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
            let _ = writeln!(
                io::stdout().lock(),
                "    {mark}: {}",
                step.description.english()
            );
        },
    )
    .await
    .map_err(|e| e.text().english())?;
    match outcome {
        Outcome::Applied { .. } => Ok(true),
        Outcome::RolledBack { error, .. } => {
            out!("Failed: {}. Everything was undone.", error.english())?;
            Ok(false)
        }
        Outcome::PartiallyApplied {
            error, leftovers, ..
        } => {
            out!("Failed: {}. These couldn't be undone:", error.english())?;
            for leftover in leftovers {
                out!("  - {}", leftover.english())?;
            }
            Ok(false)
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Offline {
        #[command(flatten)]
        args: OfflineArgs,
    }

    #[derive(Debug, Parser)]
    struct Inbox {
        #[command(subcommand)]
        command: InboxCommand,
    }

    #[test]
    fn offline_options_keep_what_they_dont_name() {
        let args = Offline::try_parse_from(["offline", "app.xyz.com", "--title", "Away"])
            .unwrap()
            .args;
        let page = page_of(
            &args,
            Some(OfflinePage {
                title: "Old".into(),
                message: "Kept".into(),
                when_app_down: true,
            }),
        );
        assert_eq!(page.title, "Away");
        assert_eq!(page.message, "Kept");
        assert!(page.when_app_down);
        assert!(
            Offline::try_parse_from(["offline", "app.xyz.com", "--off", "--title", "x"]).is_err()
        );
    }

    #[test]
    fn parses_inbox_commands() {
        let parsed = Inbox::try_parse_from([
            "inbox",
            "add",
            "app.xyz.com",
            "/hooks/",
            "--max",
            "50",
            "--days",
            "3",
            "--verify",
            "stripe",
        ])
        .unwrap();
        let InboxCommand::Add {
            max, days, verify, ..
        } = parsed.command
        else {
            panic!()
        };
        assert_eq!((max, days, verify), (50, 3, Some(Verify::Stripe)));
    }
}
