//! What the MCP server needs from its host: Teitunnel's accounts, engine, shares,
//! discovery, doctor and logs, as one object-safe trait. The CLI (`teitunnel mcp`,
//! `teitunnel serve`) hosts it with [`crate::CoreBackend`]; the desktop app can host it
//! with the same type (or its own implementation) later. Tests use an in-memory one.
//!
//! Everything returned here is already free of secrets: the core never hands out
//! tokens, and log lines are redacted again by the server before they reach an agent.

use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

use serde::Serialize;
use teitunnel_core::{
    accounts::Account,
    comments::{SubjectView, Thread},
    doctor::{Fix, Issue},
    engine::edge::IssuedToken,
    engine::{
        AccessRule, ActivityEntry, Actor, Change, Outcome, PlanView, Progress, RoutesOverview,
        TunnelSummary, Verification,
    },
    export::{ExportFile, ExportFormat},
    import::LocalSetup,
    protection::{ProtectionChange, ProtectionView, ServiceTokenView},
    remote_logs::RemoteLogState,
};

/// A boxed, sendable future (the trait is object-safe).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The result of a backend call.
pub type BackendResult<T> = Result<T, BackendError>;

/// Why a backend call failed. Messages are English sentences for the agent (and the
/// person reading along).
#[derive(Debug, Clone, thiserror::Error)]
pub enum BackendError {
    /// Something went wrong; the message says what and, when known, how to fix it.
    #[error("{0}")]
    Message(String),
    /// Something changed in Cloudflare since the plan was reviewed; here is the new plan.
    #[error("Something changed in Cloudflare since this plan was made. Review the new plan.")]
    Stale(Box<PlanView>),
    /// The plan replaces or deletes records Teitunnel didn't create and wasn't confirmed.
    #[error(
        "This plan replaces or deletes DNS records Teitunnel didn't create; it needs an explicit confirmation."
    )]
    NeedsConfirmation,
    /// What was asked for doesn't exist.
    #[error("{0}")]
    NotFound(String),
    /// This host can't do that (e.g. start connectors from a terminal-hosted server).
    #[error("{0}")]
    Unsupported(String),
}

impl BackendError {
    /// A plain message.
    pub fn message(text: impl Into<String>) -> Self {
        Self::Message(text.into())
    }
}

/// Where a change applies: an account and, optionally, one of this machine's tunnels
/// (by id; `None` is the default tunnel, or the one carrying the route).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Account id.
    pub account: String,
    /// Tunnel id.
    pub tunnel: Option<String>,
}

/// What the person approved: the plan (by fingerprint) and whether records Teitunnel
/// didn't create may be replaced or deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyApproval {
    /// Fingerprint of the reviewed plan.
    pub fingerprint: String,
    /// Confirmed changes to records Teitunnel doesn't own.
    pub confirmed: bool,
}

/// Receives progress while a plan applies (called from inside the engine).
pub type ProgressSink = Box<dyn FnMut(Progress) + Send>;

/// A share, whoever started it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    /// Pass this to stop it: a Quick Share's id, or a domain share's hostname.
    pub id: String,
    /// `quick` (a random trycloudflare.com address) or `domain` (your own domain).
    pub kind: ShareKind,
    /// The public URL.
    pub url: Option<String>,
    /// The local service shared.
    pub origin: String,
    /// `live`, `starting`, `reconnecting`, `failed: …`, or `unknown` for shares run by
    /// another process.
    pub status: String,
    /// Who runs it: `this server`, `the app` or `a terminal`.
    pub started_by: String,
    /// Started by this MCP server (it ends when the server ends).
    pub mine: bool,
    /// The account, for domain shares.
    pub account_id: Option<String>,
    /// When it started (milliseconds since the epoch).
    pub started_at: u64,
    /// When it ends by itself (milliseconds since the epoch).
    pub expires_at: Option<u64>,
    /// Visitors get a "paused" page (shares on your domain).
    pub paused: bool,
}

/// The two kinds of share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ShareKind {
    /// A Quick Share at a random `trycloudflare.com` address, no account needed.
    Quick,
    /// A temporary route on one of the account's domains.
    Domain,
}

/// A request to share on your own domain.
#[derive(Debug, Clone)]
pub struct DomainShareRequest {
    /// Account id.
    pub account: String,
    /// Hostname on one of the account's domains.
    pub hostname: String,
    /// Port, `host:port` or URL.
    pub origin: String,
    /// Require a login.
    pub access: Option<AccessRule>,
    /// Ends by itself after this long.
    pub expires_in: Option<Duration>,
}

/// A local service found by discovery.
pub use teitunnel_core::discovery::LocalService;

/// One log line, parsed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// RFC 3339 time, if the line had one.
    pub time: Option<String>,
    /// `debug`, `info`, `warn`, `error`, `fatal` or `raw`.
    pub level: String,
    /// The message.
    pub message: String,
    /// The `error` field, if any.
    pub error: Option<String>,
    /// Other structured fields (e.g. `ingressRule`, `originService`, `connIndex`).
    pub fields: serde_json::Map<String, serde_json::Value>,
}

impl LogLine {
    /// A line from a parsed cloudflared event.
    pub fn from_event(event: &cloudflared::LogEvent) -> Self {
        Self {
            time: event.time.clone(),
            level: format!("{:?}", event.level).to_ascii_lowercase(),
            message: event.message.clone(),
            error: event.error.clone(),
            fields: event.fields.clone(),
        }
    }
}

/// A connector's log lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogBatch {
    /// Where they came from, in a few words (e.g. `the Always-on connector's log file`).
    pub source: String,
    /// Oldest first.
    pub lines: Vec<LogLine>,
    /// Why there are none, when that's knowable (e.g. the app runs the connector).
    pub note: Option<String>,
}

/// Another machine's connector logs, relayed by Cloudflare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteLogBatch {
    /// Where the stream is.
    pub state: RemoteLogState,
    /// Oldest first.
    pub lines: Vec<LogLine>,
}

/// What changed, for resource notifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ChangeEvent {
    /// Routes, tunnels or DNS (after an apply).
    Routes,
    /// Shares started or stopped.
    Shares,
}

/// Teitunnel, as the MCP server sees it.
pub trait Backend: Send + Sync + 'static {
    /// This machine's name (what new tunnels are called).
    fn machine_name(&self) -> String;

    /// Connected Cloudflare accounts.
    fn accounts(&self) -> BoxFuture<'_, BackendResult<Vec<Account>>>;

    /// What an account's credential can do (JSON; never the credential).
    fn capabilities<'a>(
        &'a self,
        account: &'a str,
    ) -> BoxFuture<'a, BackendResult<serde_json::Value>>;

    /// The account's domains (JSON list of `{id, name, status, …}`).
    fn domains<'a>(&'a self, account: &'a str) -> BoxFuture<'a, BackendResult<serde_json::Value>>;

    /// This machine's tunnels and routes in an account, with DNS and connector state.
    fn overview<'a>(&'a self, account: &'a str) -> BoxFuture<'a, BackendResult<RoutesOverview>>;

    /// Every tunnel in the account (this machine's first), with connectors.
    fn tunnels<'a>(&'a self, account: &'a str) -> BoxFuture<'a, BackendResult<Vec<TunnelSummary>>>;

    /// Plans a change for review. Nothing changes.
    fn preview<'a>(
        &'a self,
        target: &'a Target,
        change: &'a Change,
    ) -> BoxFuture<'a, BackendResult<PlanView>>;

    /// Applies a reviewed plan, on behalf of `actor` (recorded in Activity).
    fn apply<'a>(
        &'a self,
        target: &'a Target,
        change: &'a Change,
        approval: ApplyApproval,
        actor: Option<Actor>,
        progress: ProgressSink,
    ) -> BoxFuture<'a, BackendResult<Outcome>>;

    /// Checks a hostname end to end through Cloudflare's edge, retrying transient
    /// failures for up to `patience`.
    fn verify<'a>(
        &'a self,
        account: &'a str,
        hostname: &'a str,
        patience: Duration,
    ) -> BoxFuture<'a, BackendResult<Verification>>;

    /// Runs the Doctor's checks (issues the person ignored in the app are left out).
    fn doctor(&self) -> BoxFuture<'_, BackendResult<Vec<Issue>>>;

    /// Runs a fix that isn't a Cloudflare change (start a connector, keep an outside
    /// edit, clean stale connections). Returns what happened, in a sentence.
    fn run_fix<'a>(
        &'a self,
        issue: &'a Issue,
        fix: &'a Fix,
        actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<String>>;

    /// Starts a Quick Share run by this server and waits for its URL.
    fn start_quick_share<'a>(
        &'a self,
        origin: &'a str,
        stop_after: Option<Duration>,
    ) -> BoxFuture<'a, BackendResult<ShareInfo>>;

    /// Shares on one of the account's domains (a temporary route through the plan →
    /// apply engine), removed when it expires, is stopped, or this server ends.
    fn start_domain_share(
        &self,
        request: DomainShareRequest,
        actor: Option<Actor>,
    ) -> BoxFuture<'_, BackendResult<(ShareInfo, Outcome)>>;

    /// Every share: this server's, the app's and terminals'.
    fn shares(&self) -> BoxFuture<'_, BackendResult<Vec<ShareInfo>>>;

    /// Stops a share by id, URL or hostname.
    fn stop_share<'a>(
        &'a self,
        share: &'a ShareInfo,
        actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<()>>;

    /// Listening TCP services on this machine, likely dev servers first.
    fn services(&self) -> BoxFuture<'_, BackendResult<Vec<LocalService>>>;

    /// One of this machine's tunnels as a file. `None` if it has none in the account.
    fn export<'a>(
        &'a self,
        account: &'a str,
        tunnel: Option<&'a str>,
        format: ExportFormat,
    ) -> BoxFuture<'a, BackendResult<Option<ExportFile>>>;

    /// Existing cloudflared setups on this machine (config files, running processes).
    fn import_scan(&self) -> BoxFuture<'_, BackendResult<Vec<LocalSetup>>>;

    /// This machine's connector logs for a tunnel, optionally only one route's request
    /// lines. `limit` is already bounded by the caller.
    fn logs<'a>(
        &'a self,
        account: &'a str,
        tunnel: Option<&'a str>,
        route: Option<(&'a str, Option<&'a str>)>,
        limit: usize,
    ) -> BoxFuture<'a, BackendResult<LogBatch>>;

    /// Another machine's connector logs (starts a stream relayed by Cloudflare; call
    /// again to read more; it stops by itself when nobody reads it).
    fn remote_logs<'a>(
        &'a self,
        account: &'a str,
        tunnel: &'a str,
        connector: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, BackendResult<RemoteLogBatch>>;

    /// Recent changes in an account, newest first.
    fn activity<'a>(
        &'a self,
        account: &'a str,
        limit: u32,
    ) -> BoxFuture<'a, BackendResult<Vec<ActivityEntry>>>;

    /// Changes this backend made (for resource notifications).
    fn subscribe(&self) -> tokio::sync::broadcast::Receiver<ChangeEvent>;

    /// Stops everything this server started (its Quick Shares and domain shares).
    /// Returns how many were stopped.
    fn stop_own_shares(&self) -> BoxFuture<'_, usize>;

    /// The account's reserved hostnames and who holds them (M12-11). Backends without
    /// Cloudflare access say so.
    fn reservations<'a>(
        &'a self,
        _account: &'a str,
    ) -> BoxFuture<'a, BackendResult<teitunnel_core::reservations::Reservations>> {
        Box::pin(async {
            Err(BackendError::Message(
                "Reservations aren't available here.".into(),
            ))
        })
    }

    /// What Teitunnel enforces for a hostname at Cloudflare's edge, with the zone's
    /// quotas.
    fn protection<'a>(
        &'a self,
        _account: &'a str,
        _hostname: &'a str,
    ) -> BoxFuture<'a, BackendResult<ProtectionView>> {
        Box::pin(async { Err(unsupported_protection()) })
    }

    /// Teitunnel's service tokens for a hostname (never their secrets).
    fn service_tokens<'a>(
        &'a self,
        _account: &'a str,
        _hostname: &'a str,
    ) -> BoxFuture<'a, BackendResult<Vec<ServiceTokenView>>> {
        Box::pin(async { Err(unsupported_protection()) })
    }

    /// Plans a protection change (service tokens) for review. Nothing changes.
    fn preview_protection<'a>(
        &'a self,
        _account: &'a str,
        _change: &'a ProtectionChange,
    ) -> BoxFuture<'a, BackendResult<PlanView>> {
        Box::pin(async { Err(unsupported_protection()) })
    }

    /// Applies a reviewed protection change, on behalf of `actor`; a created or
    /// rotated token's credentials come back once.
    fn apply_protection<'a>(
        &'a self,
        _account: &'a str,
        _change: &'a ProtectionChange,
        _approval: ApplyApproval,
        _actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<(Outcome, Vec<IssuedToken>)>> {
        Box::pin(async { Err(unsupported_protection()) })
    }

    /// Pauses (`paused`) or resumes a share on your domain or a route: the address stays
    /// and visitors get a "paused" page, served by whichever Teitunnel process serves it
    /// (M12-06).
    fn set_paused<'a>(
        &'a self,
        _account: &'a str,
        _hostname: &'a str,
        _paused: bool,
    ) -> BoxFuture<'a, BackendResult<()>> {
        Box::pin(async { Err(unsupported_extras()) })
    }

    /// Sets (or, with `None`, removes) when a share on your domain or a route is on.
    fn set_schedule<'a>(
        &'a self,
        _account: &'a str,
        _hostname: &'a str,
        _schedule: Option<teitunnel_core::schedule::Schedule>,
    ) -> BoxFuture<'a, BackendResult<()>> {
        Box::pin(async { Err(unsupported_extras()) })
    }

    /// Schedules of shares and routes.
    fn schedules(
        &self,
    ) -> BoxFuture<'_, BackendResult<Vec<teitunnel_core::schedule::RouteSchedule>>> {
        Box::pin(async { Err(unsupported_extras()) })
    }

    /// Shares a folder (static files served by this process's inspector): a Quick Share,
    /// or at `(account, hostname)`. Ends like any share this server started.
    fn share_folder(
        &self,
        _folder: teitunnel_core::folder_share::FolderShare,
        _domain: Option<(String, String)>,
        _expires_in: Option<Duration>,
        _actor: Option<Actor>,
    ) -> BoxFuture<'_, BackendResult<ShareInfo>> {
        Box::pin(async { Err(unsupported_extras()) })
    }

    /// Shares, routes and Snapshots with comments, with their counts.
    fn comment_subjects(&self) -> BoxFuture<'_, BackendResult<Vec<SubjectView>>> {
        Box::pin(async { Err(unsupported_comments()) })
    }

    /// A subject's threads (Snapshots' read from Cloudflare).
    fn comment_threads<'a>(&'a self, _key: &'a str) -> BoxFuture<'a, BackendResult<Vec<Thread>>> {
        Box::pin(async { Err(unsupported_comments()) })
    }

    /// The owner's reply on a thread.
    fn comment_reply<'a>(
        &'a self,
        _key: &'a str,
        _thread: &'a str,
        _body: &'a str,
    ) -> BoxFuture<'a, BackendResult<Thread>> {
        Box::pin(async { Err(unsupported_comments()) })
    }

    /// Resolves or reopens a thread.
    fn comment_resolve<'a>(
        &'a self,
        _key: &'a str,
        _thread: &'a str,
        _resolved: bool,
    ) -> BoxFuture<'a, BackendResult<Thread>> {
        Box::pin(async { Err(unsupported_comments()) })
    }
}

fn unsupported_extras() -> BackendError {
    BackendError::Unsupported(
        "Pausing, schedules and folder shares aren't available from this host.".into(),
    )
}

fn unsupported_comments() -> BackendError {
    BackendError::Unsupported("Comments aren't available from this host.".into())
}

fn unsupported_protection() -> BackendError {
    BackendError::Unsupported("Edge protection isn't available from this host.".into())
}

/// A backend shared between sessions.
pub type SharedBackend = Arc<dyn Backend>;
