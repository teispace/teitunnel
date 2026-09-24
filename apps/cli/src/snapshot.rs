//! `teitunnel snapshot`: publish a static copy of a site to your own Cloudflare account
//! (a Worker with static assets), so it stays online while this computer sleeps. Every
//! change is shown as a plan before it's applied, like routes.

use std::{
    io::{self, BufRead, IsTerminal, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

use clap::Subcommand;
use teitunnel_core::{
    engine::{Approval, Outcome, PlanView, StepState, format_bytes},
    snapshot::{
        self, AddressInput, PasswordInput, Preparations, PreparedView, SnapshotChange,
        SnapshotOptions, SnapshotSource, SnapshotView, build, crawl::Limits,
    },
    text::UserText,
};

use crate::{context::App, share::parse_duration};

/// Where the files come from.
#[derive(Debug, Default, clap::Args)]
#[command(next_help_heading = "Files")]
pub(crate) struct SourceArgs {
    /// Build the project first (its package manager runs its build script, e.g.
    /// `pnpm run build`), then publish the build's output folder.
    #[arg(long, conflicts_with = "crawl")]
    build: bool,
    /// Capture a site running on this computer instead of a folder, e.g.
    /// `http://localhost:5173` (a bounded crawl of its pages and files).
    #[arg(long, value_name = "URL")]
    crawl: Option<String>,
}

/// How it answers.
#[derive(Debug, Default, clap::Args)]
#[command(next_help_heading = "Settings")]
pub(crate) struct SettingsArgs {
    /// Serve index.html for paths that don't exist (single-page apps).
    #[arg(long)]
    spa: bool,
    /// Ask visitors for a password (read from the terminal, or from
    /// TEITUNNEL_SNAPSHOT_PASSWORD). Only a salted hash reaches Cloudflare.
    #[arg(long, conflicts_with = "no_password")]
    password: bool,
    /// Remove the password.
    #[arg(long)]
    no_password: bool,
    /// Require a Cloudflare Access login: an email address, or `@domain`; repeatable
    /// (hostnames on your domains only).
    #[arg(long, value_name = "EMAIL|@DOMAIN")]
    allow: Vec<String>,
    /// Delete it by itself after this long, e.g. `7d` or `12h`.
    #[arg(long, value_name = "DURATION", value_parser = parse_days)]
    expires: Option<u32>,
}

/// Shared by every change.
#[derive(Debug, Default, clap::Args)]
pub(crate) struct ChangeArgs {
    /// Account name or id.
    #[arg(long, short)]
    account: Option<String>,
    /// Apply without asking.
    #[arg(long, short)]
    yes: bool,
    /// Also allow deleting a DNS record Teitunnel didn't create on the hostname.
    #[arg(long)]
    replace: bool,
    /// Print the result as JSON.
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum SnapshotCommand {
    /// Publish a folder (or a build, or a running site) as a new Snapshot.
    Publish {
        /// The folder, or the project with --build. Default: the current folder.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// A hostname on one of your domains, e.g. `preview.example.com`. Default: your
        /// account's workers.dev address.
        #[arg(long, value_name = "HOSTNAME")]
        on: Option<String>,
        /// Its name. Default: the folder's or project's.
        #[arg(long)]
        name: Option<String>,
        /// If a Snapshot with this name exists (on this computer, or published by another
        /// computer or an earlier CI job), publish a new version of it instead.
        #[arg(long)]
        or_update: bool,
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        settings: SettingsArgs,
        #[command(flatten)]
        change: ChangeArgs,
    },
    /// Publish a new version: files from the same place again (or from PATH), and/or
    /// new settings. Only changed files are uploaded.
    Update {
        /// The Snapshot's name or hostname.
        snapshot: String,
        /// Files from here instead of where they came from last time.
        path: Option<PathBuf>,
        /// Keep the live files; change only settings.
        #[arg(long, conflicts_with = "path")]
        settings_only: bool,
        #[command(flatten)]
        source: SourceArgs,
        #[command(flatten)]
        settings: SettingsArgs,
        #[command(flatten)]
        change: ChangeArgs,
    },
    /// List Snapshots.
    Ls {
        /// Account name or id (default: every account).
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// List a Snapshot's kept versions.
    Versions {
        /// The Snapshot's name or hostname.
        snapshot: String,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Make an earlier version live again (default: the one before the live one).
    Rollback {
        /// The Snapshot's name or hostname.
        snapshot: String,
        /// The version's number (see `versions`).
        version: Option<u32>,
        #[command(flatten)]
        change: ChangeArgs,
    },
    /// Delete a Snapshot: its address, login and Worker on Cloudflare.
    Rm {
        /// The Snapshot's name or hostname.
        snapshot: String,
        #[command(flatten)]
        change: ChangeArgs,
    },
}

/// `7d`, `12h` or a number of days, as whole days (at least one).
fn parse_days(input: &str) -> Result<u32, String> {
    let input = input.trim();
    let days = if let Some(days) = input.strip_suffix('d') {
        days.parse::<u32>()
            .map_err(|_| format!("\"{input}\" isn't a duration. Try 7d."))?
    } else if input.bytes().all(|b| b.is_ascii_digit()) {
        input
            .parse::<u32>()
            .map_err(|_| format!("\"{input}\" isn't a duration. Try 7d."))?
    } else {
        let duration = parse_duration(input)?;
        u32::try_from(duration.as_secs().div_ceil(24 * 60 * 60)).unwrap_or(u32::MAX)
    };
    if days == 0 || days > 365 {
        return Err("Keep a Snapshot for between 1 and 365 days.".into());
    }
    Ok(days)
}

/// `--allow` values as an Access rule (as for routes).
fn access(allow: &[String]) -> Option<teitunnel_core::engine::AccessRule> {
    crate::access_rule(allow)
}

fn read_password() -> Result<String, String> {
    if let Ok(password) = std::env::var("TEITUNNEL_SNAPSHOT_PASSWORD") {
        return Ok(password);
    }
    if !io::stdin().is_terminal() {
        return Err(
            "Set TEITUNNEL_SNAPSHOT_PASSWORD, or run in a terminal to type the password.".into(),
        );
    }
    write!(
        io::stdout().lock(),
        "Password for visitors (6+ characters): "
    )
    .map_err(|e| e.to_string())?;
    io::stdout().flush().map_err(|e| e.to_string())?;
    let mut password = String::new();
    io::stdin()
        .lock()
        .read_line(&mut password)
        .map_err(|e| e.to_string())?;
    Ok(password.trim_end_matches(['\r', '\n']).to_owned())
}

fn password_input(settings: &SettingsArgs) -> Result<PasswordInput, String> {
    if settings.password {
        Ok(PasswordInput::Set {
            password: read_password()?,
        })
    } else if settings.no_password {
        Ok(PasswordInput::Remove)
    } else {
        Ok(PasswordInput::Keep)
    }
}

fn error(err: &snapshot::SnapshotError) -> String {
    err.text().english()
}

/// Collects the files: a folder, a build (after asking), or a crawl.
async fn prepare(
    preparations: &Preparations,
    path: &Path,
    source: &SourceArgs,
    yes: bool,
) -> Result<PreparedView, String> {
    if let Some(url) = &source.crawl {
        out!("Capturing {url}…")?;
        let dest = snapshot::capture_dir(&crate::context::data_dir()?);
        let prepared = preparations
            .crawl(url, &dest, Limits::default())
            .await
            .map_err(|e| error(&e))?;
        if let Some(report) = &prepared.crawl {
            out!(
                "Captured {} pages, {} files ({}){}.",
                report.pages,
                report.files,
                format_bytes(report.bytes),
                if report.truncated {
                    ", stopped at the limit"
                } else {
                    ""
                }
            )?;
            if !report.failed.is_empty() {
                out!(
                    "! {} links answered with an error, e.g. {}",
                    report.failed.len(),
                    report.failed[0]
                )?;
            }
        }
        return Ok(prepared);
    }
    let path = std::fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if source.build {
        let project = build::detect(&path).map_err(|e| error(&e))?;
        if let Some(warning) = project.warning {
            out!("! {}", project_warning(warning))?;
        }
        if let Some(command) = build::BuildCommand::for_project(&project).map_err(|e| error(&e))? {
            out!("This runs `{}` in {}.", command.display(), project.dir)?;
            if !yes && !crate::confirm("Build?")? {
                return Err("Nothing was built.".into());
            }
        }
        return preparations
            .build(&project, |line| {
                let _ = writeln!(io::stdout().lock(), "  │ {line}");
            })
            .await
            .map_err(|e| error(&e));
    }
    preparations.folder(&path).await.map_err(|e| error(&e))
}

fn project_warning(warning: build::ProjectWarning) -> &'static str {
    match warning {
        build::ProjectWarning::NextNeedsExport => {
            "Next.js builds a server app unless next.config sets output: 'export'. Add it, or capture the running app with --crawl."
        }
        build::ProjectWarning::SvelteKitNeedsStaticAdapter => {
            "SvelteKit needs @sveltejs/adapter-static to produce files. Add it, or capture the running app with --crawl."
        }
    }
}

fn print_prepared(prepared: &PreparedView) -> Result<(), String> {
    out!(
        "{} files ({}).",
        prepared.files,
        format_bytes(prepared.bytes)
    )?;
    if !prepared.skipped.is_empty() {
        let names: Vec<&str> = prepared
            .skipped
            .iter()
            .take(5)
            .map(|s| s.path.as_str())
            .collect();
        out!(
            "Left out (secrets and tooling): {}{}",
            names.join(", "),
            if prepared.skipped.len() > 5 {
                ", …"
            } else {
                ""
            }
        )?;
    }
    Ok(())
}

fn print_plan(plan: &PlanView) -> Result<(), String> {
    for warning in &plan.warnings {
        out!("! {}", crate::warning_text(warning))?;
    }
    for (index, step) in plan.steps.iter().enumerate() {
        out!("{:>2}. {}", index + 1, step.description.english())?;
    }
    Ok(())
}

/// Previews, asks, applies and reports a change; `true` when it was applied.
async fn apply(
    app: &App,
    preparations: &Preparations,
    change: &SnapshotChange,
    args: &ChangeArgs,
    account: &teitunnel_core::accounts::Account,
) -> Result<bool, String> {
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let ctx = app.context(account);
    let plan = snapshot::preview(&app.engine, &api, preparations, ctx, change)
        .await
        .map_err(|e| error(&e))?;
    if plan.steps.is_empty() {
        out!("Nothing to change.")?;
        return Ok(true);
    }
    print_plan(&plan)?;
    if plan.requires_confirmation && !args.replace {
        return Err("This needs a confirmation (see above). Pass --replace to allow it.".into());
    }
    if !args.yes && !crate::confirm("Apply?")? {
        return Err("Nothing changed.".into());
    }
    let connectors = app.connectors(account).await;
    let steps = plan.steps.clone();
    let mut last_percent = None;
    let outcome = snapshot::apply(
        &app.engine,
        &api,
        &connectors,
        preparations,
        ctx,
        "cli",
        change,
        Approval {
            fingerprint: &plan.fingerprint,
            confirmed: args.replace,
        },
        |progress| {
            let Some(step) = steps.get(usize::try_from(progress.step).unwrap_or(usize::MAX)) else {
                return;
            };
            let line = match progress.state {
                StepState::Transferring {
                    files,
                    total_files,
                    bytes,
                    total_bytes,
                } => {
                    let percent = (bytes * 100).checked_div(total_bytes).unwrap_or(100);
                    if last_percent == Some(percent) {
                        return;
                    }
                    last_percent = Some(percent);
                    format!("    uploading: {files} of {total_files} files ({percent}%)")
                }
                StepState::Done => format!("    done: {}", step.description.english()),
                StepState::Failed { .. } => format!("    failed: {}", step.description.english()),
                StepState::Undone => format!("    undone: {}", step.description.english()),
                StepState::UndoFailed { .. } => {
                    format!("    couldn't undo: {}", step.description.english())
                }
                _ => return,
            };
            let _ = writeln!(io::stdout().lock(), "{line}");
        },
    )
    .await
    .map_err(|e| error(&e))?;
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

/// The Snapshot by name, hostname or id: remembered here, or else published from
/// another computer or CI job (then remembered here too).
async fn locate(
    app: &App,
    account: &teitunnel_core::accounts::Account,
    key: &str,
) -> Result<SnapshotView, String> {
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    snapshot::find_or_adopt(&app.engine, &api, &account.id, key, "cli")
        .await
        .map_err(|e| error(&e))
}

fn exit(applied: bool) -> ExitCode {
    if applied {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn describe(snapshot: &SnapshotView) -> String {
    let mut notes = Vec::new();
    if snapshot.password {
        notes.push("password".to_owned());
    }
    if snapshot.access.is_some() {
        notes.push("login".to_owned());
    }
    if snapshot.live_version.is_none() {
        notes.push("incomplete: delete it and publish again".to_owned());
    }
    format!(
        "{}\t{}\tv{}\t{} files, {}{}",
        snapshot.name,
        if snapshot.url.is_empty() {
            "-"
        } else {
            &snapshot.url
        },
        snapshot
            .live_version
            .map_or_else(|| "-".to_owned(), |v| v.to_string()),
        snapshot.files,
        format_bytes(snapshot.bytes),
        if notes.is_empty() {
            String::new()
        } else {
            format!("\t{}", notes.join(", "))
        }
    )
}

fn print_result(app_snapshot: &SnapshotView, json: bool) -> Result<(), String> {
    if json {
        out!(
            "{}",
            serde_json::to_string(app_snapshot).map_err(|e| e.to_string())?
        )?;
    } else if !app_snapshot.url.is_empty() {
        out!("{} is online at {}", app_snapshot.name, app_snapshot.url)?;
    }
    Ok(())
}

/// Runs a `snapshot` command.
pub(crate) async fn run(app: &App, command: SnapshotCommand) -> Result<ExitCode, String> {
    let preparations = Preparations::default();
    match command {
        SnapshotCommand::Publish {
            path,
            on,
            name,
            or_update,
            source,
            settings,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let prepared = prepare(&preparations, &path, &source, change.yes).await?;
            print_prepared(&prepared)?;
            let name = name.unwrap_or_else(|| prepared.suggested_name.clone());
            let options = SnapshotOptions {
                spa: settings.spa || prepared.single_page,
                password: password_input(&settings)?,
                access: access(&settings.allow),
                expires_in_days: settings.expires,
            };
            // `--or-update`: a Snapshot with this name (here, or published by another
            // computer or an earlier CI job) gets a new version instead.
            let existing = if or_update {
                locate(app, &account, &name).await.ok()
            } else {
                None
            };
            let request = match &existing {
                Some(current) => {
                    if let Some(hostname) = on.as_deref()
                        && !current.hostname.eq_ignore_ascii_case(hostname)
                    {
                        out!(
                            "! {} already answers at {}; its address stays.",
                            current.name,
                            current.url
                        )?;
                    }
                    SnapshotChange::Update {
                        snapshot: current.id.clone(),
                        prepared: Some(prepared.id.clone()),
                        options: SnapshotOptions {
                            access: options.access.clone().or_else(|| current.access.clone()),
                            ..options
                        },
                    }
                }
                None => SnapshotChange::Publish {
                    prepared: prepared.id.clone(),
                    name: name.clone(),
                    address: on.map_or(AddressInput::WorkersDev, |hostname| AddressInput::Domain {
                        hostname,
                    }),
                    options,
                },
            };
            let applied = match apply(app, &preparations, &request, &change, &account).await {
                Err(message) if existing.is_some() && message.contains("same files") => {
                    out!("Nothing to change: the same files and settings are live.")?;
                    true
                }
                other => other?,
            };
            let key = existing.as_ref().map_or(name.as_str(), |e| e.id.as_str());
            if applied && let Ok(published) = snapshot::find(&app.engine, &account.id, key).await {
                print_result(&published, change.json)?;
            }
            Ok(exit(applied))
        }
        SnapshotCommand::Update {
            snapshot: key,
            path,
            settings_only,
            source,
            settings,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let current = locate(app, &account, &key).await?;
            let prepared = if settings_only {
                None
            } else {
                let (path, source) = match (path, &current.source) {
                    (Some(path), _) => (path, source),
                    (None, _) if source.crawl.is_some() || source.build => {
                        (PathBuf::from("."), source)
                    }
                    (None, Some(SnapshotSource::Folder { path })) => (PathBuf::from(path), source),
                    (None, Some(SnapshotSource::Build { project, .. })) => (
                        PathBuf::from(project),
                        SourceArgs {
                            build: true,
                            crawl: None,
                        },
                    ),
                    (None, Some(SnapshotSource::Crawl { url })) => (
                        PathBuf::from("."),
                        SourceArgs {
                            build: false,
                            crawl: Some(url.clone()),
                        },
                    ),
                    (None, None) => {
                        return Err("Where the files came from isn't known. Give a folder.".into());
                    }
                };
                let prepared = prepare(&preparations, &path, &source, change.yes).await?;
                print_prepared(&prepared)?;
                Some(prepared.id)
            };
            let access = if settings.allow.is_empty() {
                current.access.clone()
            } else {
                access(&settings.allow)
            };
            let request = SnapshotChange::Update {
                snapshot: current.id.clone(),
                prepared,
                options: SnapshotOptions {
                    spa: settings.spa || current.spa,
                    password: password_input(&settings)?,
                    access,
                    expires_in_days: settings.expires,
                },
            };
            let applied = apply(app, &preparations, &request, &change, &account).await?;
            if applied
                && let Ok(updated) = snapshot::find(&app.engine, &account.id, &current.id).await
            {
                print_result(&updated, change.json)?;
            }
            Ok(exit(applied))
        }
        SnapshotCommand::Ls { account, json } => {
            let account_id = match account {
                Some(wanted) => Some(app.account(Some(&wanted)).await?.id),
                None => None,
            };
            let all = snapshot::list(&app.engine, account_id.as_deref())
                .await
                .map_err(|e| error(&e))?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string(&all).map_err(|e| e.to_string())?
                )?;
            } else if all.is_empty() {
                out!("No Snapshots yet. Publish one with `teitunnel snapshot publish <folder>`.")?;
            } else {
                for snapshot in &all {
                    out!("{}", describe(snapshot))?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        SnapshotCommand::Versions {
            snapshot: key,
            account,
            json,
        } => {
            let account = app.account(account.as_deref()).await?;
            let found = locate(app, &account, &key).await?;
            let versions = snapshot::versions(&app.engine, &found.id)
                .await
                .map_err(|e| error(&e))?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string(&versions).map_err(|e| e.to_string())?
                )?;
            } else {
                for version in &versions {
                    out!(
                        "v{}\t{} files, {}{}",
                        version.number,
                        version.files,
                        format_bytes(version.bytes),
                        if version.live { "\tlive" } else { "" }
                    )?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        SnapshotCommand::Rollback {
            snapshot: key,
            version,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let found = locate(app, &account, &key).await?;
            let version = match version {
                Some(version) => version,
                None => snapshot::versions(&app.engine, &found.id)
                    .await
                    .map_err(|e| error(&e))?
                    .into_iter()
                    .skip_while(|v| !v.live)
                    .nth(1)
                    .map(|v| v.number)
                    .ok_or("There's no earlier version to roll back to.")?,
            };
            let request = SnapshotChange::Rollback {
                snapshot: found.id,
                version,
            };
            Ok(exit(
                apply(app, &preparations, &request, &change, &account).await?,
            ))
        }
        SnapshotCommand::Rm {
            snapshot: key,
            change,
        } => {
            let account = app.account(change.account.as_deref()).await?;
            let found = locate(app, &account, &key).await?;
            let request = SnapshotChange::Delete { snapshot: found.id };
            Ok(exit(
                apply(app, &preparations, &request, &change, &account).await?,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_expiry_in_days() {
        assert_eq!(parse_days("7d"), Ok(7));
        assert_eq!(parse_days("3"), Ok(3));
        assert_eq!(parse_days("12h"), Ok(1));
        assert_eq!(parse_days("49h"), Ok(3));
        assert!(parse_days("0d").is_err());
        assert!(parse_days("soon").is_err());
        assert!(parse_days("400d").is_err());
    }
}
