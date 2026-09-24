//! `teitunnel project …` and the project part of `teitunnel up`: a `teitunnel.yml`
//! checked into a repository declares the project's routes, shares, Snapshots and local
//! domains. Applying it shows one combined plan first, then applies it through the
//! engine; applying an applied file changes nothing (M12-12).

use std::{
    io::Write as _,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use clap::Subcommand;
use teitunnel_core::{
    accounts::Account,
    cli_shares::{self, CliShare},
    domain::OriginUrl,
    domain_shares::{self, ShareRequest},
    engine::{Change, Connectors, Context, Outcome, StepState},
    project::{
        self, HostHeaderDecl, ItemKind, ItemState, Loaded, ProjectPlan, Severity, ShareAction,
        SnapshotSourceDecl, registry,
    },
    quick_share::{HostHeaderChoice, QuickShares, ShareStatus},
    snapshot::{self, Preparations, SnapshotError, build},
    text::UserText as _,
};

use crate::{context::App, share::status};

/// `teitunnel project …`.
#[derive(Debug, Subcommand)]
pub(crate) enum ProjectCommand {
    /// Write a starter teitunnel.yml for this folder, from the routes and shares this
    /// machine runs for its project.
    Init {
        /// Replace an existing teitunnel.yml.
        #[arg(long)]
        force: bool,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
    },
    /// Check the project file: every problem with its line and column. Needs no account.
    Check {
        #[command(flatten)]
        file: FileArg,
    },
    /// Show what applying the project file would change. Nothing is changed.
    Diff {
        #[command(flatten)]
        file: FileArg,
        /// Account name or id (default: the file's `account`).
        #[arg(long, short)]
        account: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Apply the project file: its routes and Snapshots, then its shares until Ctrl-C.
    Apply {
        #[command(flatten)]
        file: FileArg,
        #[command(flatten)]
        apply: ApplyOptions,
    },
    /// Stop the project's shares; with --remove-routes, also remove the routes it
    /// created (never routes that were there before).
    Down {
        #[command(flatten)]
        file: FileArg,
        /// Also remove the routes applying the project created.
        #[arg(long)]
        remove_routes: bool,
        /// Account name or id.
        #[arg(long, short)]
        account: Option<String>,
        /// Apply without asking.
        #[arg(long, short)]
        yes: bool,
    },
    /// List the projects this machine knows.
    List {
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
}

/// Which project file.
#[derive(Debug, Default, Clone, clap::Args)]
pub(crate) struct FileArg {
    /// The project file, or a folder with one (default: this folder or the nearest one
    /// above it in the repository).
    #[arg(long, short = 'f', value_name = "PATH")]
    pub(crate) file: Option<PathBuf>,
}

/// How to apply.
#[derive(Debug, Default, Clone, clap::Args)]
pub(crate) struct ApplyOptions {
    /// Account name or id (default: the file's `account`).
    #[arg(long, short)]
    pub(crate) account: Option<String>,
    /// Apply without asking.
    #[arg(long, short)]
    pub(crate) yes: bool,
    /// Also allow what needs a confirmation (replacing DNS records Teitunnel didn't
    /// create).
    #[arg(long)]
    pub(crate) replace: bool,
    /// Don't start a share when the exposure check finds a leak.
    #[arg(long)]
    pub(crate) strict: bool,
}

/// Reads the project file (`file`, or the one for this folder) and prints its problems.
///
/// # Errors
/// No file, or a file with errors.
pub(crate) fn load(file: Option<&Path>) -> Result<Loaded, String> {
    let start = match file {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let loaded = project::load(&start).map_err(|e| e.to_string())?;
    print_diagnostics(&loaded);
    loaded.file().map_err(|e| e.to_string())?;
    Ok(loaded)
}

fn print_diagnostics(loaded: &Loaded) {
    let name = loaded.path.file_name().map_or_else(
        || loaded.path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    for d in &loaded.parsed.diagnostics {
        let level = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        status(&format!(
            "{name}:{}:{}: {level}: {}",
            d.line,
            d.column,
            d.message.english()
        ));
    }
}

/// The account a project applies to: `--account`, else the file's, else the only one.
async fn account(app: &App, loaded: &Loaded, wanted: Option<&str>) -> Result<Account, String> {
    let from_file = loaded.parsed.file.as_ref().and_then(|f| f.account.clone());
    app.account(wanted.or(from_file.as_deref())).await
}

/// Services Quick Shares of terminals share now.
fn quick_origins() -> Vec<String> {
    crate::context::data_dir()
        .map(|dir| cli_shares::list(&dir.join("run-cli")))
        .unwrap_or_default()
        .into_iter()
        .map(|s| s.origin)
        .collect()
}

fn state_word(state: ItemState) -> &'static str {
    match state {
        ItemState::Applied => "applied",
        ItemState::Differs => "differs",
        ItemState::Missing => "missing",
        ItemState::Unsupported => "skipped",
    }
}

fn kind_word(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Route => "route",
        ItemKind::Share => "share",
        ItemKind::Snapshot => "snapshot",
        ItemKind::LocalDomain => "local",
    }
}

/// The build command a Snapshot's project runs, for the plan.
fn build_command(dir: &str) -> Option<String> {
    let project = build::detect(Path::new(dir)).ok()?;
    build::BuildCommand::for_project(&project)
        .ok()
        .flatten()
        .map(|c| c.display())
}

fn print_plan(plan: &ProjectPlan, account: &Account) -> Result<(), String> {
    out!("Project {} ({}) in {}:", plan.name, plan.path, account.name)?;
    for item in &plan.items {
        let note = item
            .note
            .as_ref()
            .map(|n| format!(" ({})", n.english()))
            .unwrap_or_default();
        out!(
            "  {:<8} {} -> {}\t{}{note}",
            kind_word(item.kind),
            item.name,
            item.target,
            state_word(item.state)
        )?;
    }
    if plan.is_empty() {
        return Ok(());
    }
    out!("\nWhat applying does:")?;
    let mut n = 0;
    for route in &plan.routes {
        for warning in &route.plan.warnings {
            out!("  ! {}", crate::warning_text(warning))?;
        }
        for step in &route.plan.steps {
            n += 1;
            out!("  {n:>2}. {}", step.description.english())?;
        }
    }
    for snapshot in &plan.snapshots {
        n += 1;
        let address = snapshot
            .hostname
            .clone()
            .unwrap_or_else(|| "workers.dev".to_owned());
        let source = match &snapshot.source {
            SnapshotSourceDecl::Folder(path) => format!("the files in {path}"),
            SnapshotSourceDecl::Build(path) => match build_command(path) {
                Some(command) => format!("{path}, built with `{command}`"),
                None => path.clone(),
            },
        };
        if snapshot.exists {
            out!(
                "  {n:>2}. Publish a new version of Snapshot {} from {source} if it changed",
                snapshot.name
            )?;
        } else {
            out!(
                "  {n:>2}. Publish Snapshot {} from {source} at {address}",
                snapshot.name
            )?;
        }
    }
    for share in &plan.shares {
        n += 1;
        match &share.hostname {
            Some(hostname) => out!(
                "  {n:>2}. Share {} at https://{hostname} while this runs",
                share.origin
            )?,
            None => out!(
                "  {n:>2}. Share {} at a trycloudflare.com address while this runs",
                share.origin
            )?,
        }
    }
    Ok(())
}

/// What applying left to run: the shares to start.
pub(crate) struct Applied {
    pub(crate) account: Account,
    pub(crate) shares: Vec<ShareAction>,
}

/// Plans the project, shows the plan, asks (unless `--yes`), and applies its routes and
/// Snapshots. Shares are returned for the caller to run.
///
/// # Errors
/// An invalid file, a refused plan, a declined confirmation, or a failed change.
pub(crate) async fn apply(
    app: &App,
    loaded: &Loaded,
    options: &ApplyOptions,
) -> Result<Option<Applied>, String> {
    let account = account(app, loaded, options.account.as_deref()).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let connectors = app.connectors(&account).await;
    let ctx = app.context(&account);
    let plan = project::plan(
        &app.engine,
        &api,
        &connectors,
        ctx,
        loaded,
        &quick_origins(),
    )
    .await
    .map_err(|e| e.to_string())?;
    print_plan(&plan, &account)?;
    if plan.is_empty() {
        out!("Nothing to change.")?;
        return Ok(None);
    }
    if plan.requires_confirmation && !options.replace {
        return Err("This needs a confirmation (see above). Pass --replace to allow it.".into());
    }
    if !options.yes && !crate::confirm("Apply?")? {
        out!("Nothing changed.")?;
        return Ok(None);
    }

    let mut created = Vec::new();
    for action in &plan.routes {
        let steps = action.plan.steps.clone();
        let outcome = project::apply_route(
            &app.engine,
            &api,
            &connectors,
            ctx,
            action,
            options.replace,
            |progress| {
                let Some(step) = steps.get(usize::try_from(progress.step).unwrap_or(usize::MAX))
                else {
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
                    std::io::stdout().lock(),
                    "    {mark}: {}",
                    step.description.english()
                );
            },
        )
        .await
        .map_err(|e| format!("{}: {e}", action.hostname))?;
        match outcome {
            Outcome::Applied {
                connector_error, ..
            } => {
                if let Some(error) = connector_error {
                    out!("Note: {}", error.english())?;
                }
                if matches!(action.change, Change::AddRoute { .. }) {
                    created.push(registry::CreatedRoute {
                        account_id: account.id.clone(),
                        hostname: action.hostname.clone(),
                        path: action.path.clone(),
                    });
                }
            }
            Outcome::RolledBack { error, .. } => {
                return Err(format!(
                    "{}: {}. Its changes were undone.",
                    action.hostname,
                    error.english()
                ));
            }
            Outcome::PartiallyApplied {
                error, leftovers, ..
            } => {
                for leftover in leftovers {
                    out!("  - left in place: {}", leftover.english())?;
                }
                return Err(format!("{}: {}", action.hostname, error.english()));
            }
        }
    }
    registry::applied(
        app.store(),
        &loaded.path.display().to_string(),
        &loaded.name,
        created,
    )
    .await
    .map_err(|e| e.to_string())?;

    let file = loaded.file().map_err(|e| e.to_string())?;
    let resolved = project::resolve(file, &loaded.vars).map_err(|e| e.to_string())?;
    for (action, (decl, hostname)) in plan.snapshots.iter().zip(&resolved.snapshots) {
        publish_snapshot(app, &account, action, decl, hostname.as_deref(), options).await?;
    }
    Ok(Some(Applied {
        account,
        shares: plan.shares,
    }))
}

async fn publish_snapshot(
    app: &App,
    account: &Account,
    action: &project::SnapshotAction,
    decl: &project::SnapshotDecl,
    hostname: Option<&str>,
    options: &ApplyOptions,
) -> Result<(), String> {
    let error = |e: SnapshotError| format!("Snapshot {}: {}", action.name, e.text().english());
    let preparations = Preparations::default();
    let prepared = match &action.source {
        SnapshotSourceDecl::Folder(path) => {
            preparations.folder(Path::new(path)).await.map_err(error)?
        }
        SnapshotSourceDecl::Build(path) => {
            let project = build::detect(Path::new(path)).map_err(error)?;
            preparations
                .build(&project, |line| {
                    let _ = writeln!(std::io::stdout().lock(), "  | {line}");
                })
                .await
                .map_err(error)?
        }
    };
    let existing = app
        .engine
        .local()
        .sites(Some(&account.id))
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|s| s.name.eq_ignore_ascii_case(&action.name));
    let needs_password = existing.as_ref().is_none_or(|row| !row.password);
    let password = match &decl.password {
        Some(reference) if needs_password => Some(
            project::resolve_secret(reference, app.secrets().as_ref())
                .map_err(|e| e.to_string())?,
        ),
        _ => None,
    };
    let change = project::snapshot_change(decl, hostname, prepared.id, existing.as_ref(), password);
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let ctx = app.context(account);
    let plan = match snapshot::preview(&app.engine, &api, &preparations, ctx, &change).await {
        Err(SnapshotError::Unchanged) => {
            out!("Snapshot {} is up to date.", action.name)?;
            return Ok(());
        }
        other => other.map_err(error)?,
    };
    if plan.requires_confirmation && !options.replace {
        return Err(format!(
            "Snapshot {} would replace a DNS record Teitunnel didn't create. Pass --replace to allow it.",
            action.name
        ));
    }
    let connectors = app.connectors(account).await;
    let outcome = snapshot::apply(
        &app.engine,
        &api,
        &connectors,
        &preparations,
        ctx,
        "cli",
        &change,
        teitunnel_core::engine::Approval {
            fingerprint: &plan.fingerprint,
            confirmed: options.replace,
        },
        |_| {},
    )
    .await
    .map_err(error)?;
    match outcome {
        Outcome::Applied { .. } => {
            out!("Published Snapshot {}.", action.name)?;
            Ok(())
        }
        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
            Err(format!("Snapshot {}: {}", action.name, error.english()))
        }
    }
}

/// Shares a project runs while this process does.
pub(crate) struct Running {
    account: Account,
    quick: Option<(QuickShares, PathBuf)>,
    domains: Vec<String>,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running")
            .field("domains", &self.domains)
            .finish_non_exhaustive()
    }
}

fn host_header(decl: &HostHeaderDecl) -> HostHeaderChoice {
    match decl {
        HostHeaderDecl::Auto => HostHeaderChoice::Auto,
        HostHeaderDecl::Off => HostHeaderChoice::Off,
        HostHeaderDecl::Set(value) => HostHeaderChoice::Set {
            value: value.clone(),
        },
    }
}

/// Starts a project's shares (after the exposure check), printing their addresses.
///
/// # Errors
/// A share that couldn't start (those started are stopped again), or `strict` and a
/// finding.
pub(crate) async fn start_shares<K: Connectors>(
    app: &App,
    applied: Applied,
    connectors: &K,
    strict: bool,
) -> Result<Running, String> {
    let mut running = Running {
        account: applied.account,
        quick: None,
        domains: Vec::new(),
    };
    for (index, share) in applied.shares.iter().enumerate() {
        let started = start_share(app, &mut running, connectors, index, share, strict).await;
        if let Err(message) = started {
            running.stop(app).await;
            return Err(message);
        }
    }
    Ok(running)
}

async fn start_share<K: Connectors>(
    app: &App,
    running: &mut Running,
    connectors: &K,
    index: usize,
    share: &ShareAction,
    strict: bool,
) -> Result<(), String> {
    crate::exposure::check(&share.origin, Some(app.store()), strict).await?;
    let choice = host_header(&share.host_header);
    let Some(hostname) = &share.hostname else {
        let origin = OriginUrl::parse(&share.origin).map_err(|e| e.to_string())?;
        if running.quick.is_none() {
            running.quick = Some(crate::share::quick_shares(&crate::context::data_dir()?).await?);
        }
        let Some((quick, owner_dir)) = &running.quick else {
            return Ok(());
        };
        let started = quick
            .start(
                origin,
                share
                    .expires_after
                    .map(|s| Duration::from_secs(u64::from(s))),
                &choice,
            )
            .await
            .map_err(crate::share::start_error)?;
        let url = wait_for_url(quick, &started.id).await?;
        out!(
            "https://{} -> {}",
            url.trim_start_matches("https://"),
            share.origin
        )?;
        let record = CliShare {
            owner: teitunnel_core::runtime::this_process(),
            origin: started.origin.to_string(),
            url,
            started_at: domain_shares::now_ms(),
            stop_at: None,
        };
        let _ = cli_shares::record_as(owner_dir, &format!("project-{index}"), &record);
        return Ok(());
    };
    let api = app
        .accounts
        .client(&running.account.id)
        .await
        .map_err(|e| e.to_string())?;
    let resolved = choice
        .resolve(&share.origin)
        .await
        .map_err(|e| e.to_string())?;
    let owner = teitunnel_core::runtime::this_process();
    let outcome = domain_shares::start(
        &app.engine,
        &api,
        connectors,
        app.context(&running.account),
        ShareRequest {
            hostname,
            origin: &share.origin,
            access: share.login.clone(),
            expires_at: share
                .expires_after
                .map(|s| domain_shares::now_ms() + u64::from(s) * 1000),
            owner: &owner,
            host_header: resolved.map(|h| h.value),
        },
    )
    .await
    .map_err(|e| match e {
        teitunnel_core::engine::EngineError::NeedsConfirmation => format!(
            "{hostname} has a DNS record Teitunnel didn't create; a share never replaces one."
        ),
        other => other.to_string(),
    })?;
    match outcome {
        Outcome::Applied { .. } => {
            running.domains.push(hostname.clone());
            out!("https://{hostname} -> {}", share.origin)?;
            Ok(())
        }
        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
            Err(format!("{hostname}: {}", error.english()))
        }
    }
}

async fn wait_for_url(quick: &QuickShares, id: &str) -> Result<String, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
    loop {
        match quick.list().into_iter().find(|s| s.id == id) {
            Some(share) => match (share.status, share.url) {
                (ShareStatus::Failed { message }, _) => return Err(message.english()),
                (ShareStatus::Live, Some(url)) => return Ok(url),
                _ => {}
            },
            None => return Err("The share stopped before it was ready.".into()),
        }
        if tokio::time::Instant::now() > deadline {
            return Err("The share didn't get an address within a minute.".into());
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

impl Running {
    /// Stops every share (removing the temporary routes and records).
    pub(crate) async fn stop(self, app: &App) {
        if let Some((quick, owner_dir)) = self.quick {
            quick.stop_all().await;
            for entry in std::fs::read_dir(&owner_dir)
                .into_iter()
                .flatten()
                .flatten()
            {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(key) = name
                    .strip_prefix("share-")
                    .and_then(|n| n.strip_suffix(".json"))
                    .filter(|k| k.starts_with("project-"))
                {
                    cli_shares::forget_as(&owner_dir, key);
                }
            }
        }
        if self.domains.is_empty() {
            return;
        }
        let Ok(api) = app.accounts.client(&self.account.id).await else {
            return;
        };
        let connectors = app.connectors(&self.account).await;
        for hostname in &self.domains {
            if let Err(message) = domain_shares::stop(
                &app.engine,
                &api,
                &connectors,
                app.context(&self.account),
                hostname,
            )
            .await
            {
                status(&format!(
                    "Couldn't remove {hostname}: {}. Teitunnel removes it the next time it runs.",
                    message.english()
                ));
            }
        }
    }
}

/// `teitunnel project …`.
pub(crate) async fn run(command: ProjectCommand) -> Result<ExitCode, String> {
    match command {
        ProjectCommand::Check { file } => {
            let start = match file.file {
                Some(path) => path,
                None => std::env::current_dir().map_err(|e| e.to_string())?,
            };
            let loaded = project::load(&start).map_err(|e| e.to_string())?;
            print_diagnostics(&loaded);
            if loaded.parsed.has_errors() {
                return Ok(ExitCode::FAILURE);
            }
            project::resolve(loaded.file().map_err(|e| e.to_string())?, &loaded.vars)
                .map_err(|e| e.to_string())?;
            out!("{} is valid.", loaded.path.display())?;
            Ok(ExitCode::SUCCESS)
        }
        ProjectCommand::Init { force, account } => init(force, account.as_deref()).await,
        ProjectCommand::List { json } => {
            let app = App::open().await?;
            let list = registry::list(app.store())
                .await
                .map_err(|e| e.to_string())?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string(&list).map_err(|e| e.to_string())?
                )?;
            } else if list.is_empty() {
                out!("No projects yet. Apply one with `teitunnel project apply`.")?;
            }
            if !json {
                for entry in &list {
                    out!("{}\t{}", entry.name, entry.path)?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ProjectCommand::Diff {
            file,
            account: wanted,
            json,
        } => {
            let loaded = load(file.file.as_deref())?;
            let app = App::open().await?;
            let account = account(&app, &loaded, wanted.as_deref()).await?;
            let api = app
                .accounts
                .client(&account.id)
                .await
                .map_err(|e| e.to_string())?;
            let connectors = app.connectors(&account).await;
            let plan = project::plan(
                &app.engine,
                &api,
                &connectors,
                app.context(&account),
                &loaded,
                &quick_origins(),
            )
            .await
            .map_err(|e| e.to_string())?;
            if json {
                out!(
                    "{}",
                    serde_json::to_string(&plan).map_err(|e| e.to_string())?
                )?;
            } else {
                print_plan(&plan, &account)?;
                if plan.is_empty() {
                    out!("Nothing to change.")?;
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        ProjectCommand::Apply {
            file,
            apply: options,
        } => {
            let loaded = load(file.file.as_deref())?;
            let app = App::open().await?;
            let Some(applied) = apply(&app, &loaded, &options).await? else {
                return Ok(ExitCode::SUCCESS);
            };
            if applied.shares.is_empty() {
                return Ok(ExitCode::SUCCESS);
            }
            let connectors = app.connectors(&applied.account).await;
            let running = start_shares(&app, applied, &connectors, options.strict).await?;
            status("Sharing until you press Ctrl-C.");
            crate::share::interrupted().await;
            running.stop(&app).await;
            status("Stopped the project's shares.");
            Ok(ExitCode::SUCCESS)
        }
        ProjectCommand::Down {
            file,
            remove_routes,
            account: wanted,
            yes,
        } => {
            let loaded = load(file.file.as_deref())?;
            let app = App::open().await?;
            down(&app, &loaded, wanted.as_deref(), remove_routes, yes).await
        }
    }
}

async fn init(force: bool, wanted: Option<&str>) -> Result<ExitCode, String> {
    let dir = std::env::current_dir().map_err(|e| e.to_string())?;
    let path = dir.join(project::FILE_NAME);
    if path.exists() && !force {
        return Err(format!(
            "{} exists already. Pass --force to replace it.",
            path.display()
        ));
    }
    let name =
        teitunnel_core::discovery::project_name(&dir).unwrap_or_else(|| "project".to_owned());
    let app = App::open_or_empty().await?;
    let accounts = app.accounts.list().await.unwrap_or_default();
    let mut input = project::init::InitInput {
        name,
        services: teitunnel_core::discovery::services().await,
        ..Default::default()
    };
    if !accounts.is_empty() {
        let account = app.account(wanted).await?;
        if accounts.len() > 1 {
            input.account = Some(account.name.clone());
        }
        let api = app
            .accounts
            .client(&account.id)
            .await
            .map_err(|e| e.to_string())?;
        let connectors = app.connectors(&account).await;
        if let Ok(overview) = app
            .engine
            .overview(&api, &connectors, app.context(&account))
            .await
        {
            input.routes = overview.routes;
        }
        input.shares = app
            .engine
            .local()
            .shares(Some(&account.id))
            .await
            .unwrap_or_default();
    }
    std::fs::write(&path, project::init::render(&input)).map_err(|e| e.to_string())?;
    out!(
        "Wrote {}. Review it, then run `teitunnel up`.",
        path.display()
    )?;
    Ok(ExitCode::SUCCESS)
}

async fn down(
    app: &App,
    loaded: &Loaded,
    wanted: Option<&str>,
    remove_routes: bool,
    yes: bool,
) -> Result<ExitCode, String> {
    let account = account(app, loaded, wanted).await?;
    let api = app
        .accounts
        .client(&account.id)
        .await
        .map_err(|e| e.to_string())?;
    let connectors = app.connectors(&account).await;
    let ctx = app.context(&account);
    let file = loaded.file().map_err(|e| e.to_string())?;
    let resolved = project::resolve(file, &loaded.vars).map_err(|e| e.to_string())?;
    let running = app
        .engine
        .local()
        .shares(Some(&account.id))
        .await
        .map_err(|e| e.to_string())?;
    let mut stopped = 0;
    for share in &resolved.shares {
        let Some(hostname) = &share.hostname else {
            continue;
        };
        if running
            .iter()
            .any(|s| s.hostname.eq_ignore_ascii_case(hostname))
        {
            domain_shares::stop(&app.engine, &api, &connectors, ctx, hostname)
                .await
                .map_err(|e| e.english())?;
            out!("Stopped sharing https://{hostname}.")?;
            stopped += 1;
        }
    }
    // Quick Shares run by a `teitunnel up` or `project apply`: that process ends.
    let runs = crate::context::data_dir()?.join("run-cli");
    let quick: Vec<&str> = resolved
        .shares
        .iter()
        .filter(|s| s.hostname.is_none())
        .map(|s| s.decl.origin.as_str())
        .collect();
    let mut owners: Vec<String> = cli_shares::list(&runs)
        .into_iter()
        .filter(|s| quick.iter().any(|o| same_origin(o, &s.origin)))
        .map(|s| s.owner)
        .collect();
    owners.dedup();
    for owner in owners {
        if cli_shares::stop(&runs, &owner).await {
            out!("Stopped the teitunnel process sharing the project's services ({owner}).")?;
            stopped += 1;
        }
    }
    if remove_routes {
        let entry = registry::list(app.store())
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|p| p.path == loaded.path.display().to_string());
        let created: Vec<registry::CreatedRoute> = entry
            .map(|p| p.created_routes)
            .unwrap_or_default()
            .into_iter()
            .filter(|r| r.account_id == account.id)
            .collect();
        if created.is_empty() {
            out!("The project created no routes (routes that were there before stay).")?;
        } else {
            for route in &created {
                out!(
                    "Remove {}{}",
                    route.hostname,
                    route
                        .path
                        .as_deref()
                        .map(|p| format!(" {p}"))
                        .unwrap_or_default()
                )?;
            }
            if !yes && !crate::confirm("Remove these routes?")? {
                out!("Routes kept.")?;
                return Ok(ExitCode::SUCCESS);
            }
            let mut gone = Vec::new();
            for route in created {
                let removed = remove_route(app, &api, &connectors, ctx, &route).await;
                match removed {
                    Ok(()) => {
                        out!("Removed {}.", route.hostname)?;
                        gone.push(route);
                    }
                    Err(message) => status(&format!("{}: {message}", route.hostname)),
                }
            }
            registry::removed(app.store(), &loaded.path.display().to_string(), gone)
                .await
                .map_err(|e| e.to_string())?;
        }
    } else if stopped == 0 {
        out!("None of the project's shares is running.")?;
    }
    Ok(ExitCode::SUCCESS)
}

fn same_origin(a: &str, b: &str) -> bool {
    match (
        teitunnel_core::domain::RouteOrigin::parse(a),
        teitunnel_core::domain::RouteOrigin::parse(b),
    ) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

async fn remove_route<K: Connectors>(
    app: &App,
    api: &cf_api::Client,
    connectors: &K,
    ctx: Context<'_>,
    route: &registry::CreatedRoute,
) -> Result<(), String> {
    let change = Change::RemoveRoute {
        hostname: route.hostname.clone(),
        path: route.path.clone(),
    };
    let tunnel = app
        .engine
        .local()
        .tunnel_routing(ctx.account, &route.hostname)
        .await
        .map_err(|e| e.to_string())?;
    let ctx = Context {
        tunnel: tunnel.as_deref(),
        ..ctx
    };
    let intent = app
        .engine
        .intent_for(api, ctx, &change)
        .await
        .map_err(|e| e.to_string())?;
    let plan = app
        .engine
        .preview(api, ctx, &intent)
        .await
        .map_err(|e| e.to_string())?;
    if plan.is_empty() {
        return Ok(());
    }
    // Never deletes a record Teitunnel didn't create.
    let outcome = app
        .engine
        .apply(
            api,
            connectors,
            ctx,
            &intent,
            teitunnel_core::engine::Approval {
                fingerprint: &plan.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .map_err(|e| e.to_string())?;
    match outcome {
        Outcome::Applied { .. } => Ok(()),
        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
            Err(error.english())
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser as _;

    use super::*;

    #[derive(Debug, clap::Parser)]
    struct Cli {
        #[command(subcommand)]
        command: ProjectCommand,
    }

    #[test]
    fn parses_project_commands() {
        let cli = Cli::try_parse_from([
            "project",
            "apply",
            "-f",
            "infra/teitunnel.yml",
            "-y",
            "--strict",
        ])
        .unwrap();
        let ProjectCommand::Apply { file, apply } = cli.command else {
            panic!("not apply");
        };
        assert_eq!(file.file.as_deref(), Some(Path::new("infra/teitunnel.yml")));
        assert!(apply.yes && apply.strict && !apply.replace);
        let cli = Cli::try_parse_from(["project", "down", "--remove-routes"]).unwrap();
        assert!(matches!(
            cli.command,
            ProjectCommand::Down {
                remove_routes: true,
                yes: false,
                ..
            }
        ));
    }

    #[test]
    fn host_header_choices() {
        assert_eq!(host_header(&HostHeaderDecl::Auto), HostHeaderChoice::Auto);
        assert_eq!(host_header(&HostHeaderDecl::Off), HostHeaderChoice::Off);
        assert_eq!(
            host_header(&HostHeaderDecl::Set("localhost:5173".into())),
            HostHeaderChoice::Set {
                value: "localhost:5173".into()
            }
        );
    }
}
