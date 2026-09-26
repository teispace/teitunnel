//! Snapshots: a static copy of what the user is sharing, hosted as a Worker with static
//! assets on their own Cloudflare account, so it stays online while their computer
//! sleeps.
//!
//! The files come from a folder, from a project's build, or from crawling a running
//! site. They're collected and hashed first ("prepared"); publishing is then a plan
//! like any other change: previewed, applied step by step, undone on failure. Only
//! changed files are uploaded, a new version goes live only once complete, and the last
//! versions stay available to roll back to.

pub mod build;
pub mod content;
pub mod crawl;
pub mod password;
mod prepare;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use prepare::{Preparations, Prepared, PreparedView};

use crate::{
    domain::Hostname,
    engine::{
        AccessRule, Approval, CloudApi, Connectors, Context, Engine, EngineError, Intent,
        ObserveError, Outcome, Password, PlanView, Progress, SiteAddress, SiteRow, SiteSettings,
        SiteSpec,
    },
    text::{Text, UserText, english_display, msg},
};

/// Longest Snapshot name.
pub const MAX_NAME: usize = 40;
/// Worker names start with this, so Teitunnel's are recognisable in the dashboard.
pub const SCRIPT_PREFIX: &str = "teitunnel-";

/// Why a Snapshot couldn't be prepared or changed. Messages are shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    /// A name must be 1–40 letters, digits, spaces, `-` or `_`.
    InvalidName,
    /// Another Snapshot in the account has the name.
    NameTaken(String),
    /// The hostname isn't valid (or is a wildcard).
    InvalidHostname(String),
    /// Not a folder.
    NotAFolder(String),
    /// Nothing to publish in the folder.
    Empty(String),
    /// More files than Cloudflare accepts in one version.
    TooManyFiles(usize),
    /// A file over 25 MiB.
    FileTooLarge(String),
    /// Reading or writing a file failed.
    Io {
        /// The path.
        path: String,
        /// What went wrong.
        detail: String,
    },
    /// No web project or `index.html` in the folder.
    NoProject(String),
    /// The build failed; its last lines of output.
    Build {
        /// The command, e.g. `pnpm run build`.
        command: String,
        /// Its output.
        output: String,
    },
    /// The build took too long.
    BuildTimeout(String),
    /// The build finished but its output folder is missing.
    NoOutput(String),
    /// Not an `http(s)://` address.
    InvalidUrl(String),
    /// The site to capture didn't answer.
    Unreachable(String),
    /// Only sites on this computer or its network can be captured.
    NotLocal(String),
    /// The crawl couldn't start.
    Crawl(String),
    /// A password is too short.
    PasswordTooShort(usize),
    /// No randomness for a salt (should never happen).
    Random,
    /// The prepared files are gone (prepared too long ago, or the app restarted).
    NotPrepared,
    /// No Snapshot with that id.
    NotFound,
    /// No kept version with that number.
    NoSuchVersion(u32),
    /// A settings-only change needs the live version's files, which aren't known.
    NoLiveFiles,
    /// The same files and settings are already live.
    Unchanged,
    /// The engine refused or failed before changing anything.
    #[error(transparent)]
    Engine(#[from] EngineError),
}

impl SnapshotError {
    pub(crate) fn io(path: &Path, err: &std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            detail: err.to_string(),
        }
    }
}

impl From<crate::store::StoreError> for SnapshotError {
    fn from(err: crate::store::StoreError) -> Self {
        Self::Engine(EngineError::Observe(ObserveError::Store(err)))
    }
}

impl From<cf_api::Error> for SnapshotError {
    fn from(err: cf_api::Error) -> Self {
        Self::Engine(EngineError::Observe(ObserveError::Api(err)))
    }
}

impl UserText for SnapshotError {
    fn text(&self) -> Text {
        use msg::snapshot::error as m;
        match self {
            Self::InvalidName => m::invalid_name(MAX_NAME as u64),
            Self::NameTaken(name) => m::name_taken(name),
            Self::InvalidHostname(hostname) => m::invalid_hostname(hostname),
            Self::NotAFolder(path) => m::not_a_folder(path),
            Self::Empty(path) => m::empty(path),
            Self::TooManyFiles(max) => m::too_many_files(*max as u64),
            Self::FileTooLarge(path) => m::too_large(path),
            Self::Io { path, detail } => m::read(path, detail),
            Self::NoProject(path) => m::no_project(path),
            Self::Build { command, output } => m::build(command, output),
            Self::BuildTimeout(command) => m::build_timeout(command),
            Self::NoOutput(path) => m::no_output(path),
            Self::InvalidUrl(url) => m::invalid_url(url),
            Self::Unreachable(detail) => m::unreachable(detail),
            Self::NotLocal(url) => m::not_local(url),
            Self::Crawl(detail) => m::crawl(detail),
            Self::PasswordTooShort(min) => m::password_too_short(*min as u64),
            Self::Random => m::random(),
            Self::NotPrepared => m::not_prepared(),
            Self::NotFound => m::not_found(),
            Self::NoSuchVersion(number) => m::no_such_version(u64::from(*number)),
            Self::NoLiveFiles => m::files_missing(),
            Self::Unchanged => m::unchanged(),
            Self::Engine(err) => err.text(),
        }
    }
}

english_display!(SnapshotError);

/// Where a Snapshot's files come from, so "Update" can get them again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SnapshotSource {
    /// A folder, published as it is.
    Folder {
        /// The folder.
        path: String,
    },
    /// A project, built first.
    Build {
        /// The project folder.
        project: String,
        /// The build command shown, e.g. `pnpm run build`.
        command: String,
        /// Its output folder.
        output: String,
    },
    /// A running site, crawled.
    Crawl {
        /// Where it was captured from, e.g. `http://localhost:5173/`.
        url: String,
    },
}

/// Where a new Snapshot answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AddressInput {
    /// A hostname on one of the account's domains.
    Domain {
        /// E.g. `preview.example.com`.
        hostname: String,
    },
    /// The account's `workers.dev` subdomain.
    WorkersDev,
}

/// The password of a new version.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PasswordInput {
    /// Keep what the live version has (none for a new Snapshot).
    Keep,
    /// No password.
    Remove,
    /// Require this password (hashed at once; never stored or echoed).
    Set {
        /// The password.
        password: String,
    },
}

impl std::fmt::Debug for PasswordInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Keep => f.write_str("Keep"),
            Self::Remove => f.write_str("Remove"),
            Self::Set { .. } => f.write_str("Set([redacted])"),
        }
    }
}

/// Settings for a new Snapshot, or a new version of one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotOptions {
    /// Serve `index.html` for unknown paths (single-page apps).
    pub spa: bool,
    /// A password checked by the Snapshot's Worker.
    pub password: PasswordInput,
    /// A Cloudflare Access login for these people (custom hostnames only).
    pub access: Option<AccessRule>,
    /// Delete it after this many days.
    pub expires_in_days: Option<u32>,
    /// Let reviewers comment (kept in the account's D1 database); left out: as it is
    /// now (off for a new Snapshot).
    #[serde(default)]
    pub comments: Option<bool>,
}

/// A change to Snapshots, as the UI and CLI ask for it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SnapshotChange {
    /// Publish prepared files as a new Snapshot.
    Publish {
        /// From [`Preparations`].
        prepared: String,
        /// Its name.
        name: String,
        /// Where it answers.
        address: AddressInput,
        /// Settings.
        options: SnapshotOptions,
    },
    /// Publish a new version: new files (`prepared`) and/or new settings.
    Update {
        /// The Snapshot.
        snapshot: String,
        /// New files; `None` keeps the live version's.
        prepared: Option<String>,
        /// Settings.
        options: SnapshotOptions,
    },
    /// Make an earlier version live again.
    Rollback {
        /// The Snapshot.
        snapshot: String,
        /// The version's number.
        version: u32,
    },
    /// Delete a Snapshot everywhere.
    Delete {
        /// The Snapshot.
        snapshot: String,
    },
}

/// A Snapshot, for lists.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    /// Local id.
    pub id: String,
    /// Account id.
    pub account_id: String,
    /// Name.
    pub name: String,
    /// The address people open, e.g. `https://preview.example.com`.
    pub url: String,
    /// The hostname.
    pub hostname: String,
    /// On workers.dev rather than one of the account's domains.
    pub workers_dev: bool,
    /// The Worker.
    pub script: String,
    /// Where its files come from.
    pub source: Option<SnapshotSource>,
    /// Single-page app fallback.
    pub spa: bool,
    /// Password protected.
    pub password: bool,
    /// Access login.
    pub access: Option<AccessRule>,
    /// When it deletes itself (ms since the epoch).
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub expires_at: Option<u64>,
    /// Created (ms).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: u64,
    /// Last published (ms).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub updated_at: u64,
    /// The live version's number (`None`: publishing never finished).
    pub live_version: Option<u32>,
    /// Versions kept.
    pub versions: u32,
    /// Files in the live version.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub files: u64,
    /// Its size.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub bytes: u64,
    /// Reviewers can comment.
    pub comments: bool,
}

/// A version, for the version list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotVersionView {
    /// 1, 2, 3…
    pub number: u32,
    /// Published (ms).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: u64,
    /// Files.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub files: u64,
    /// Bytes.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub bytes: u64,
    /// Serving now.
    pub live: bool,
    /// Single-page app fallback.
    pub spa: bool,
    /// Password protected.
    pub password: bool,
}

/// A name's URL-safe form: `My Demo!` → `my-demo`.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// Checks a name and returns it trimmed.
///
/// # Errors
/// [`SnapshotError::InvalidName`].
pub fn valid_name(name: &str) -> Result<String, SnapshotError> {
    let name = name.trim();
    let ok = !name.is_empty()
        && name.chars().count() <= MAX_NAME
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
        && !slug(name).is_empty();
    if ok {
        Ok(name.to_owned())
    } else {
        Err(SnapshotError::InvalidName)
    }
}

/// The Worker for a Snapshot name: `teitunnel-<slug>` (at most 63 characters, a DNS
/// label, since it's part of the workers.dev address).
pub fn script_for(name: &str) -> String {
    let mut script = format!("{SCRIPT_PREFIX}{}", slug(name));
    script.truncate(63);
    script.trim_end_matches('-').to_owned()
}

fn is_workers_dev(hostname: &str) -> bool {
    hostname.ends_with(".workers.dev")
}

fn address_of(row: &SiteRow) -> SiteAddress {
    match &row.hostname {
        Some(host) if !is_workers_dev(host) => Hostname::parse(host)
            .map_or(SiteAddress::WorkersDev, |hostname| SiteAddress::Domain {
                hostname,
            }),
        _ => SiteAddress::WorkersDev,
    }
}

fn spec_of(row: &SiteRow, access: Option<AccessRule>) -> SiteSpec {
    SiteSpec {
        id: row.id.clone(),
        name: row.name.clone(),
        script: row.script.clone(),
        address: address_of(row),
        access,
    }
}

fn days_from_now(days: Option<u32>) -> Option<u64> {
    days.filter(|d| *d > 0)
        .map(|d| crate::domain_shares::now_ms() + u64::from(d) * 24 * 60 * 60 * 1000)
}

fn password_setting(input: &PasswordInput, has_password: bool) -> Result<Password, SnapshotError> {
    Ok(match input {
        PasswordInput::Keep if has_password => Password::Keep,
        PasswordInput::Keep | PasswordInput::Remove => Password::Off,
        PasswordInput::Set { password } => Password::Set {
            hash: password::hash(password)?,
        },
    })
}

/// Comments bound to the account's database (resolved by the planner); the email
/// Access vouches for is trusted only behind Teitunnel's own login.
fn comments_setting(login: bool) -> crate::engine::SiteComments {
    crate::engine::SiteComments {
        database: crate::engine::front::DatabaseRef::Created,
        identity: login,
    }
}

fn access_of(input: Option<&AccessRule>) -> Result<Option<AccessRule>, SnapshotError> {
    input
        .map(|rule| {
            rule.normalized().map_err(|e| {
                SnapshotError::Engine(EngineError::Input(crate::engine::InputError {
                    field: "access",
                    message: e.text(),
                }))
            })
        })
        .transpose()
}

/// A change turned into the engine's intent, with the row to remember first for a new
/// Snapshot.
#[derive(Debug)]
pub struct Planned {
    /// For the engine.
    pub intent: Intent,
    /// A new Snapshot, saved before applying.
    pub new_row: Option<SiteRow>,
}

/// Turns `change` into an intent. Reads the local store and, for a new workers.dev
/// Snapshot, the account's subdomain.
///
/// # Errors
/// Invalid input, unknown Snapshots or versions, prepared files that are gone.
pub async fn plan_change<C: CloudApi>(
    engine: &Engine,
    api: &C,
    preparations: &Preparations,
    account: &str,
    owner: &str,
    change: &SnapshotChange,
) -> Result<Planned, SnapshotError> {
    let local = engine.local();
    match change {
        SnapshotChange::Publish {
            prepared,
            name,
            address,
            options,
        } => {
            let name = valid_name(name)?;
            let prepared = preparations
                .get(prepared)
                .ok_or(SnapshotError::NotPrepared)?;
            let script = script_for(&name);
            let existing = local.sites(Some(account)).await?;
            if existing
                .iter()
                .any(|s| s.name.eq_ignore_ascii_case(&name) || s.script == script)
            {
                return Err(SnapshotError::NameTaken(name));
            }
            let (address, hostname) = match address {
                AddressInput::Domain { hostname } => {
                    let parsed = Hostname::parse(hostname)
                        .ok()
                        .filter(|h| !h.as_str().starts_with("*.") && !is_workers_dev(h.as_str()))
                        .ok_or_else(|| SnapshotError::InvalidHostname(hostname.clone()))?;
                    let host = parsed.to_string();
                    (SiteAddress::Domain { hostname: parsed }, Some(host))
                }
                AddressInput::WorkersDev => {
                    let subdomain = api.workers_subdomain(account).await?;
                    (
                        SiteAddress::WorkersDev,
                        subdomain.map(|s| format!("{script}.{s}.workers.dev")),
                    )
                }
            };
            let access = access_of(options.access.as_ref())?;
            let mut settings = SiteSettings {
                spa: options.spa,
                password: password_setting(&options.password, false)?,
                overlay: None,
                comments: None,
            };
            if options.comments == Some(true) {
                settings = settings.with_comments(comments_setting(access.is_some()));
            }
            let now = crate::domain_shares::now_ms();
            let row = SiteRow {
                id: uuid::Uuid::new_v4().to_string(),
                account_id: account.to_owned(),
                name: name.clone(),
                script: script.clone(),
                hostname,
                source: serde_json::to_string(&prepared.source).unwrap_or_default(),
                spa: options.spa,
                password: settings.password != Password::Off,
                access: access.clone(),
                expires_at: days_from_now(options.expires_in_days),
                owner: owner.to_owned(),
                live_version: None,
                created_at: now,
                updated_at: now,
            };
            Ok(Planned {
                intent: Intent::PublishSnapshot {
                    site: SiteSpec {
                        id: row.id.clone(),
                        name,
                        script,
                        address,
                        access,
                    },
                    settings,
                    content: prepared.content.clone(),
                },
                new_row: Some(row),
            })
        }
        SnapshotChange::Update {
            snapshot,
            prepared,
            options,
        } => {
            let row = local.site(snapshot).await?.ok_or(SnapshotError::NotFound)?;
            let live = match &row.live_version {
                Some(version) => local.site_version_content(&row.id, version).await?,
                None => None,
            };
            let content = match prepared {
                Some(id) => preparations
                    .get(id)
                    .ok_or(SnapshotError::NotPrepared)?
                    .content
                    .clone(),
                None => live.clone().ok_or(SnapshotError::NoLiveFiles)?,
            };
            let mut settings = SiteSettings {
                spa: options.spa,
                password: password_setting(&options.password, row.password)?,
                overlay: None,
                comments: None,
            };
            let access = access_of(options.access.as_ref())?;
            let had_comments = local.site_comments(&row.id).await?;
            let comments = options.comments.unwrap_or(had_comments);
            if comments {
                settings = settings.with_comments(comments_setting(access.is_some()));
            }
            let unchanged = live.as_ref().is_some_and(|l| {
                l.files == content.files
                    && l.headers == content.headers
                    && l.redirects == content.redirects
            }) && row.spa == settings.spa
                && matches!(settings.password, Password::Keep | Password::Off)
                && row.password == (settings.password == Password::Keep)
                && row.access == access
                && comments == had_comments;
            if unchanged {
                return Err(SnapshotError::Unchanged);
            }
            Ok(Planned {
                intent: Intent::UpdateSnapshot {
                    site: spec_of(&row, access),
                    settings,
                    content,
                    previous: live.map(|l| l.files).unwrap_or_default(),
                },
                new_row: None,
            })
        }
        SnapshotChange::Rollback { snapshot, version } => {
            let row = local.site(snapshot).await?.ok_or(SnapshotError::NotFound)?;
            let target = local
                .site_versions(&row.id)
                .await?
                .into_iter()
                .find(|v| v.number == *version)
                .ok_or(SnapshotError::NoSuchVersion(*version))?;
            Ok(Planned {
                intent: Intent::RollbackSnapshot {
                    site: spec_of(&row, row.access.clone()),
                    version_id: target.version_id,
                    number: target.number,
                },
                new_row: None,
            })
        }
        SnapshotChange::Delete { snapshot } => {
            let row = local.site(snapshot).await?.ok_or(SnapshotError::NotFound)?;
            Ok(Planned {
                intent: Intent::DeleteSnapshot {
                    site: spec_of(&row, row.access.clone()),
                },
                new_row: None,
            })
        }
    }
}

/// Previews a change.
///
/// # Errors
/// See [`plan_change`] and the engine's preview.
pub async fn preview<C: CloudApi>(
    engine: &Engine,
    api: &C,
    preparations: &Preparations,
    ctx: Context<'_>,
    change: &SnapshotChange,
) -> Result<PlanView, SnapshotError> {
    let planned = plan_change(engine, api, preparations, ctx.account, "app", change).await?;
    let plan = engine.preview(api, ctx, &planned.intent).await?;
    Ok(plan.view(ctx.account))
}

/// Applies a reviewed change and keeps the local record in step: a new Snapshot is
/// remembered before anything is published (and forgotten if it all rolled back), a
/// deleted one is forgotten once it's gone.
///
/// # Errors
/// See [`plan_change`]; failures while applying are in the [`Outcome`].
#[allow(clippy::too_many_arguments)]
pub async fn apply<C, K, P>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    preparations: &Preparations,
    ctx: Context<'_>,
    owner: &str,
    change: &SnapshotChange,
    approval: Approval<'_>,
    progress: P,
) -> Result<Outcome, SnapshotError>
where
    C: CloudApi,
    K: Connectors,
    P: FnMut(Progress) + Send,
{
    let local = engine.local();
    let planned = plan_change(engine, api, preparations, ctx.account, owner, change).await?;
    if let Some(row) = &planned.new_row {
        local.save_site(row).await?;
    }
    let outcome = engine
        .apply(api, connectors, ctx, &planned.intent, approval, progress)
        .await;
    match (&planned.intent, &outcome) {
        (Intent::DeleteSnapshot { site }, Ok(Outcome::Applied { .. })) => {
            forget_comments(local, api, ctx.account, site).await;
            local.forget_site(&site.id).await?;
        }
        (Intent::PublishSnapshot { site, .. }, Ok(Outcome::RolledBack { .. }) | Err(_)) => {
            local.forget_site(&site.id).await?;
        }
        (Intent::UpdateSnapshot { site, .. }, Ok(Outcome::Applied { .. })) => {
            if let Some(mut row) = local.site(&site.id).await? {
                if let SnapshotChange::Update {
                    prepared, options, ..
                } = change
                {
                    if let Some(prepared) = prepared.as_deref().and_then(|id| preparations.get(id))
                    {
                        row.source = serde_json::to_string(&prepared.source).unwrap_or_default();
                    }
                    row.expires_at = days_from_now(options.expires_in_days).or(row.expires_at);
                }
                row.access.clone_from(&site.access);
                row.updated_at = crate::domain_shares::now_ms();
                local.save_site(&row).await?;
            }
        }
        _ => {}
    }
    if let (
        Intent::PublishSnapshot { site, settings, .. }
        | Intent::UpdateSnapshot { site, settings, .. },
        Ok(Outcome::Applied { .. }),
    ) = (&planned.intent, &outcome)
    {
        let on = settings.comments.is_some();
        local.set_site_comments(&site.id, on).await?;
        if on && let Some(row) = local.site(&site.id).await? {
            let url = row
                .hostname
                .as_deref()
                .map(|h| format!("https://{h}"))
                .unwrap_or_default();
            let subject =
                crate::comments::Subject::snapshot(&row.id, &row.account_id, &row.name, &url);
            if let Err(err) = crate::comments::register_subject(local.store(), &subject).await {
                tracing::warn!(%err, "couldn't list the Snapshot's comments");
            }
        }
    }
    if let SnapshotChange::Publish { prepared, .. }
    | SnapshotChange::Update {
        prepared: Some(prepared),
        ..
    } = change
        && matches!(outcome, Ok(Outcome::Applied { .. }))
    {
        preparations.remove(prepared);
    }
    Ok(outcome?)
}

/// Deletes a deleted Snapshot's comments from the account's database and the app's
/// list (best effort: the database may be gone or unreadable).
async fn forget_comments<C: CloudApi>(
    local: &crate::engine::Local,
    api: &C,
    account: &str,
    site: &SiteSpec,
) {
    if let Ok(Some(database)) = local.cloud_database(account).await
        && let Err(err) =
            crate::comments::remote::delete_site(api, account, &database, &site.script).await
    {
        tracing::debug!(%err, "couldn't delete a Snapshot's comments");
    }
    let key = crate::comments::Subject::snapshot(&site.id, account, &site.name, "").key;
    if let Err(err) = crate::comments::forget_subject(local.store(), &key).await {
        tracing::debug!(%err, "couldn't forget a Snapshot's comments");
    }
}

/// Snapshots in `account` (or every account), with their live version's size.
///
/// # Errors
/// Database errors.
pub async fn list(
    engine: &Engine,
    account: Option<&str>,
) -> Result<Vec<SnapshotView>, SnapshotError> {
    let local = engine.local();
    let mut out = Vec::new();
    for row in local.sites(account).await? {
        let versions = local.site_versions(&row.id).await?;
        let live = versions
            .iter()
            .find(|v| Some(&v.version_id) == row.live_version.as_ref());
        let hostname = row.hostname.clone().unwrap_or_default();
        out.push(SnapshotView {
            url: if hostname.is_empty() {
                String::new()
            } else {
                format!("https://{hostname}")
            },
            workers_dev: is_workers_dev(&hostname),
            hostname,
            id: row.id.clone(),
            account_id: row.account_id.clone(),
            name: row.name.clone(),
            script: row.script.clone(),
            source: serde_json::from_str(&row.source).ok(),
            spa: row.spa,
            password: row.password,
            access: row.access.clone(),
            expires_at: row.expires_at,
            created_at: row.created_at,
            updated_at: row.updated_at,
            live_version: live.map(|v| v.number),
            versions: u32::try_from(versions.len()).unwrap_or(u32::MAX),
            files: live.map_or(0, |v| v.files),
            bytes: live.map_or(0, |v| v.bytes),
            comments: local.site_comments(&row.id).await?,
        });
    }
    Ok(out)
}

/// A Snapshot's kept versions, newest first.
///
/// # Errors
/// Database errors, or [`SnapshotError::NotFound`].
pub async fn versions(
    engine: &Engine,
    snapshot: &str,
) -> Result<Vec<SnapshotVersionView>, SnapshotError> {
    let local = engine.local();
    let row = local.site(snapshot).await?.ok_or(SnapshotError::NotFound)?;
    Ok(local
        .site_versions(&row.id)
        .await?
        .into_iter()
        .map(|v| SnapshotVersionView {
            live: Some(&v.version_id) == row.live_version.as_ref(),
            number: v.number,
            created_at: v.created_at,
            files: v.files,
            bytes: v.bytes,
            spa: v.spa,
            password: v.password,
        })
        .collect())
}

/// Finds a Snapshot by id, name or hostname (the CLI's argument).
///
/// # Errors
/// Database errors, or [`SnapshotError::NotFound`].
pub async fn find(
    engine: &Engine,
    account: &str,
    key: &str,
) -> Result<SnapshotView, SnapshotError> {
    let key = key
        .trim()
        .trim_start_matches("https://")
        .trim_end_matches('/');
    list(engine, Some(account))
        .await?
        .into_iter()
        .find(|s| {
            s.id == key
                || s.name.eq_ignore_ascii_case(key)
                || s.hostname.eq_ignore_ascii_case(key)
                || s.script == key
        })
        .ok_or(SnapshotError::NotFound)
}

/// Finds a Snapshot this computer doesn't remember but the account has: published from
/// another computer or an earlier CI job (a Worker named `teitunnel-<name>`, or the
/// Worker serving `key` as a hostname). Remembers it here so it can be updated, rolled
/// forward or deleted; its earlier versions' manifests aren't known here, and a password
/// it had is kept only if given again on the next update. `Ok(None)`: no such Worker.
///
/// # Errors
/// API and database errors.
pub async fn adopt<C: CloudApi>(
    engine: &Engine,
    api: &C,
    account: &str,
    key: &str,
    owner: &str,
) -> Result<Option<SnapshotView>, SnapshotError> {
    use crate::engine::sites::{SiteNeed, observe};
    let key = key
        .trim()
        .trim_start_matches("https://")
        .trim_end_matches('/');
    let script = if key.contains('.') {
        let served = api.worker_domains(account, None, Some(key)).await?;
        match served
            .into_iter()
            .find(|d| d.hostname.eq_ignore_ascii_case(key))
        {
            Some(domain) if domain.service.starts_with(SCRIPT_PREFIX) => domain.service,
            _ => return Ok(None),
        }
    } else if key.starts_with(SCRIPT_PREFIX) {
        key.to_owned()
    } else {
        script_for(&valid_name(key)?)
    };
    let need = SiteNeed {
        script: Some(script.clone()),
        hostname: None,
    };
    let Some(state) = observe(api, account, &need).await? else {
        return Ok(None);
    };
    if !state.exists {
        return Ok(None);
    }
    let name = script
        .strip_prefix(SCRIPT_PREFIX)
        .unwrap_or(&script)
        .to_owned();
    let hostname = state
        .domains
        .first()
        .map(|d| d.hostname.clone())
        .or_else(|| {
            state
                .subdomain
                .as_deref()
                .filter(|_| state.workers_dev)
                .map(|s| format!("{script}.{s}.workers.dev"))
        });
    let now = crate::domain_shares::now_ms();
    let row = SiteRow {
        id: uuid::Uuid::new_v4().to_string(),
        account_id: account.to_owned(),
        name: name.clone(),
        script,
        hostname,
        source: String::new(),
        spa: false,
        password: false,
        access: None,
        expires_at: None,
        owner: owner.to_owned(),
        live_version: state.active_version,
        created_at: now,
        updated_at: now,
    };
    engine.local().save_site(&row).await?;
    Ok(Some(find(engine, account, &row.id).await?))
}

/// [`find`], or else [`adopt`] from the account.
///
/// # Errors
/// See both; [`SnapshotError::NotFound`] when neither finds it.
pub async fn find_or_adopt<C: CloudApi>(
    engine: &Engine,
    api: &C,
    account: &str,
    key: &str,
    owner: &str,
) -> Result<SnapshotView, SnapshotError> {
    match find(engine, account, key).await {
        Err(SnapshotError::NotFound) => adopt(engine, api, account, key, owner)
            .await?
            .ok_or(SnapshotError::NotFound),
        found => found,
    }
}

/// Deletes every Snapshot whose time is up, in every account (swept like domain shares).
/// Returns what couldn't be deleted; they're tried again next time.
pub async fn sweep_expired<K: Connectors>(
    accounts: &crate::accounts::Accounts,
    engine: &Engine,
    connectors: &K,
    machine_name: &str,
) -> Vec<Text> {
    let now = crate::domain_shares::now_ms();
    let mut failures = Vec::new();
    let preparations = Preparations::default();
    let rows = engine.local().sites(None).await.unwrap_or_default();
    for row in rows
        .into_iter()
        .filter(|r| r.expires_at.is_some_and(|at| at <= now))
    {
        let Ok(api) = accounts.client(&row.account_id).await else {
            continue;
        };
        let ctx = Context {
            account: &row.account_id,
            machine_name,
            tunnel: None,
        };
        if let Err(message) =
            delete_now(engine, &api, connectors, &preparations, ctx, &row.id).await
        {
            tracing::warn!(snapshot = %row.name, "couldn't delete an expired Snapshot: {}", message.english());
            failures.push(message);
        }
    }
    failures
}

/// Deletes a Snapshot without a review step (expiry, `rm -y`): only what Teitunnel
/// made is ever removed, and nothing needs confirming.
///
/// # Errors
/// A message; the Snapshot stays remembered.
pub async fn delete_now<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    preparations: &Preparations,
    ctx: Context<'_>,
    snapshot: &str,
) -> Result<(), Text> {
    let change = SnapshotChange::Delete {
        snapshot: snapshot.to_owned(),
    };
    let plan = preview(engine, api, preparations, ctx, &change)
        .await
        .map_err(|e| e.text())?;
    let approval = Approval {
        fingerprint: &plan.fingerprint,
        confirmed: false,
    };
    match apply(
        engine,
        api,
        connectors,
        preparations,
        ctx,
        "app",
        &change,
        approval,
        |_| {},
    )
    .await
    .map_err(|e| e.text())?
    {
        Outcome::Applied { .. } => Ok(()),
        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => Err(error),
    }
}

/// A folder for a crawl's capture under `base` (the app's data folder).
pub fn capture_dir(base: &Path) -> PathBuf {
    base.join("snapshots")
        .join(uuid::Uuid::new_v4().to_string())
}

#[cfg(test)]
mod tests;
