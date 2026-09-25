//! Diagnostic tools: the Doctor and its fixes, connector logs (this machine's and other
//! machines'), connector health.

use std::time::Duration;

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::{
    doctor::{Fix, Issue, Severity, safe_change},
    remote_logs::RemoteLogState,
};

use super::{
    AccountRef, Hints, account,
    routes::{ApplyResult, PlanOut, TunnelOut, apply_stored, plan_and_store, tunnel_out},
    spec, tunnel_id,
};
use crate::{
    backend::{LogLine, SharedBackend, Target},
    limits,
    plans::Plans,
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolResult,
        ToolSpec, arguments,
    },
};

/// Most log lines one call returns.
const MAX_LINES: usize = 500;
/// Longest a remote_logs call listens.
const MAX_LISTEN: u64 = 30;

pub(super) fn specs() -> Vec<ToolSpec> {
    vec![
        spec::<DoctorArgs, DoctorResult>(
            "doctor",
            "Find problems",
            "Run Teitunnel's Doctor: every check the app runs, over cloudflared, every connected account, this machine's tunnels and connectors, DNS records, logins and private networks. Returns issues (most severe first), each with a stable `id`, what's wrong, the evidence, and its fixes.\n\
             \n\
             Use it first when something doesn't work (\"my site is down\", a 502, error 1033), then fix_issue with the issue's id. `safe` fixes touch only what Teitunnel created.\n\
             \n\
             Example: {\"severity\": \"error\"}",
            ToolClass::Read,
            Hints::READ_CLOUD,
            Duration::from_secs(120),
        ),
        spec::<FixArgs, FixResult>(
            "fix_issue",
            "Fix a problem",
            "Fix an issue the doctor found, by its `id` (and `fixIndex` when it offers several; 0 is the recommended one).\n\
             \n\
             A safe fix (it changes only what Teitunnel created: a missing DNS record, an orphan record or login) is applied right away through a plan (after the person's approval in `ask` mode). Any other Cloudflare fix returns a plan (`planned`) to show the person and apply with apply_plan. Fixes outside Cloudflare (start this machine's connector, accept an outside edit, clean stale connections) run after approval; installing cloudflared or reconnecting an account is for the person to do in the app (`guidance`).\n\
             \n\
             Example: {\"issueId\": \"dns.missing:abc123:app.example.com\"}",
            ToolClass::Destructive,
            Hints {
                read_only: false,
                destructive: true,
                idempotent: false,
                open_world: true,
            },
            Duration::from_secs(300),
        ),
        spec::<LogsArgs, LogsResult>(
            "logs_tail",
            "Read connector logs",
            "Read the newest log lines of this machine's cloudflared connector for a tunnel, oldest first (secrets are redacted). With `hostname` (and `path`), only the request lines of that route: errors reaching the local service, timeouts, bad gateways.\n\
             \n\
             Use it when a route answers 502/504 or the connector keeps reconnecting. Lines come from the connector this server can see (an Always-on service's log file, or connectors run by this server); for another machine's connector use remote_logs.\n\
             \n\
             Example: {\"hostname\": \"app.example.com\", \"level\": \"warn\", \"limit\": 50}",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<RemoteLogsArgs, RemoteLogsResult>(
            "remote_logs",
            "Read another machine's logs",
            "Listen to the live logs of a connector on another machine (a server, a teammate's laptop), relayed by Cloudflare, for up to `seconds` (1 to 30), then return what arrived. Call again to keep listening; the stream stops by itself when nobody reads it.\n\
             \n\
             Needs the tunnel (name or id, from list_tunnels) and optionally which of its connectors (its `id`); by default the first one that isn't this machine.\n\
             \n\
             Example: {\"tunnel\": \"prod-server\", \"seconds\": 10}",
            ToolClass::Wait,
            Hints::READ_CLOUD,
            Duration::from_secs(MAX_LISTEN + 15),
        ),
        spec::<StatusArgs, StatusResult>(
            "connector_status",
            "Connector health",
            "Show the health of this machine's connectors (the cloudflared processes serving its tunnels) in every connected account: running or not, edge connections and locations, the cloudflared version, and what Cloudflare reports for each tunnel.\n\
             \n\
             Use it when every route of a tunnel is down at once (the connector, not the route, is the problem).",
            ToolClass::Read,
            Hints::READ_CLOUD,
            super::DEFAULT_TIMEOUT,
        ),
    ]
}

/// Filters.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DoctorArgs {
    /// Only this severity or worse: `error`, `warning` or `info` (default: all).
    #[serde(default)]
    severity: Option<String>,
    /// Only issues in this account (name or id).
    #[serde(default)]
    account: Option<String>,
    /// From a previous answer's `nextCursor`.
    #[serde(default)]
    cursor: Option<String>,
    /// At most this many (default 50, up to 200).
    #[serde(default)]
    limit: Option<usize>,
}

/// A fix on offer.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FixOut {
    /// Pass as `fixIndex`.
    index: usize,
    /// What it does.
    label: String,
    /// `change` (a Cloudflare plan), `startConnector`, `keepTheirs`,
    /// `cleanConnections`, `installBinary` or `reconnect`.
    kind: String,
    /// Touches only what Teitunnel created; applied without a separate plan review.
    safe: bool,
}

/// An issue.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IssueOut {
    /// Pass to fix_issue.
    id: String,
    /// The check that found it, e.g. `dns.missing`.
    check: String,
    /// `error`, `warning` or `info`.
    severity: String,
    /// The account it's in.
    account_id: Option<String>,
    /// What it's about (a hostname, a tunnel…).
    subject: String,
    /// One line.
    title: String,
    /// What it means and what to do.
    detail: String,
    /// Supporting facts.
    evidence: Vec<String>,
    /// Fixes, recommended first.
    fixes: Vec<FixOut>,
}

fn fix_label(fix: &Fix) -> (String, &'static str) {
    match fix {
        Fix::Change { label, .. } => (label.english(), "change"),
        Fix::InstallBinary => ("Install cloudflared".into(), "installBinary"),
        Fix::StartConnector { .. } => ("Start this machine's connector".into(), "startConnector"),
        Fix::KeepTheirs { .. } => ("Keep the outside edit".into(), "keepTheirs"),
        Fix::Reconnect => (
            "Reconnect the account with the right permissions".into(),
            "reconnect",
        ),
        Fix::CleanConnections { .. } => ("Remove stale connections".into(), "cleanConnections"),
        Fix::LocalDomains { .. } => ("Fix local domains in Teitunnel".into(), "localDomains"),
    }
}

fn issue_out(issue: &Issue) -> IssueOut {
    IssueOut {
        id: issue.id.clone(),
        check: issue.check.clone(),
        severity: match issue.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
        .into(),
        account_id: issue.account_id.clone(),
        subject: issue.subject.clone(),
        title: issue.title.english(),
        detail: issue.detail.english(),
        evidence: issue
            .evidence
            .iter()
            .map(teitunnel_core::text::Text::english)
            .collect(),
        fixes: issue
            .fixes
            .iter()
            .enumerate()
            .map(|(index, fix)| {
                let (label, kind) = fix_label(fix);
                FixOut {
                    index,
                    label,
                    kind: kind.into(),
                    safe: index == 0 && safe_change(issue).is_some(),
                }
            })
            .collect(),
    }
}

/// Issues.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DoctorResult {
    /// Most severe first.
    issues: Vec<IssueOut>,
    /// Errors among all matching issues.
    errors: usize,
    /// How many match in all.
    total: usize,
    /// Pass as `cursor` for the next page.
    next_cursor: Option<String>,
}

pub(super) async fn doctor(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: DoctorArgs = arguments(args)?;
    let floor = match args
        .severity
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        None | Some("info" | "all") => Severity::Info,
        Some("warning" | "warn") => Severity::Warning,
        Some("error") => Severity::Error,
        Some(other) => {
            return Err(ToolError::new(format!(
                "\"{other}\" isn't a severity; use error, warning or info."
            )));
        }
    };
    let account_id = match args.account.as_deref() {
        Some(wanted) => Some(account(backend, Some(wanted)).await?.id),
        None => None,
    };
    let issues: Vec<Issue> = backend
        .doctor()
        .await?
        .into_iter()
        .filter(|i| i.severity <= floor)
        .filter(|i| account_id.is_none() || i.account_id == account_id)
        .collect();
    let errors = issues
        .iter()
        .filter(|i| i.severity == Severity::Error)
        .count();
    let (issues, next_cursor, total) =
        limits::page(issues, args.cursor.as_deref(), args.limit).map_err(ToolError::new)?;
    let summary = if total == 0 {
        "No problems found.".to_owned()
    } else {
        format!("{total} issue(s), {errors} error(s).")
    };
    Ok(ToolOutput::new(&DoctorResult {
        issues: issues.iter().map(issue_out).collect(),
        errors,
        total,
        next_cursor,
    })
    .with_summary(summary))
}

/// Which fix.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FixArgs {
    /// The issue's id, from doctor.
    issue_id: String,
    /// Which of its fixes (default 0, the recommended one).
    #[serde(default)]
    fix_index: usize,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// The result of fix_issue.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FixResult {
    /// `applied` (a safe plan was applied), `planned` (review `plan`, then apply_plan),
    /// `done` (a fix outside Cloudflare ran), `guidance` (for the person to do),
    /// `needsApproval`, `declined`, or `failed`.
    outcome: String,
    /// What happened or what to do.
    message: String,
    /// With `planned`: the plan.
    plan: Option<PlanOut>,
    /// With `applied`: how applying went.
    result: Option<ApplyResult>,
}

fn only(outcome: &str, message: impl Into<String>) -> ToolResult {
    Ok(ToolOutput::new(&FixResult {
        outcome: outcome.into(),
        message: message.into(),
        plan: None,
        result: None,
    }))
}

pub(super) async fn fix_issue(
    backend: &SharedBackend,
    plans: &Plans,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: FixArgs = arguments(args)?;
    let issues = backend.doctor().await?;
    let issue = issues
        .iter()
        .find(|i| i.id == args.issue_id.trim())
        .ok_or_else(|| {
            ToolError::new(format!(
                "No current issue with id \"{}\": it may be fixed already. Run doctor again.",
                args.issue_id
            ))
        })?;
    let fix = issue.fixes.get(args.fix_index).ok_or_else(|| {
        ToolError::new(format!(
            "Issue \"{}\" has {} fix(es); fixIndex {} doesn't exist.",
            issue.id,
            issue.fixes.len(),
            args.fix_index
        ))
    })?;
    match fix {
        Fix::Change { change, label } => {
            let account = issue.account_id.clone().ok_or_else(|| {
                ToolError::new(
                    "This issue isn't in an account, so there's nothing to change in Cloudflare.",
                )
            })?;
            let safe = args.fix_index == 0 && safe_change(issue).is_some();
            let plan = plan_and_store(
                backend,
                plans,
                Target {
                    account,
                    tunnel: issue.tunnel_id.clone(),
                },
                change.clone(),
                format!("{} ({})", label.english(), issue.subject),
                ctx,
            )
            .await?;
            if !safe || plan.requires_confirmation {
                return Ok(ToolOutput::new(&FixResult {
                    outcome: "planned".into(),
                    message: "This fix is a plan: show the person its steps and apply it with apply_plan.".into(),
                    plan: Some(plan),
                    result: None,
                }));
            }
            let stored = plans
                .get(&plan.plan_id)
                .ok_or_else(|| ToolError::new("The plan expired; try again."))?;
            let result = apply_stored(backend, plans, stored, args.confirmed, true, ctx).await?;
            Ok(ToolOutput::new(&FixResult {
                outcome: "applied".into(),
                message: result.message.clone(),
                plan: None,
                result: Some(result),
            }))
        }
        Fix::InstallBinary => only(
            "guidance",
            "cloudflared is missing or too old. Ask the person to open Teitunnel (it installs and updates its own copy), or install it with their package manager (e.g. `brew install cloudflared`).",
        ),
        Fix::Reconnect => only(
            "guidance",
            "The account's credential can't do this. Ask the person to reconnect it in Teitunnel (Settings → Accounts) or create a token from Teitunnel's template; credentials never pass through agents.",
        ),
        Fix::LocalDomains { .. } => only(
            "guidance",
            "This is about local HTTPS domains on this computer (trust, ports or .test names). Ask the person to open Local Domains in Teitunnel, or run `teitunnel local-domain status`; trusting certificates needs their confirmation.",
        ),
        Fix::StartConnector { .. } | Fix::KeepTheirs { .. } | Fix::CleanConnections { .. } => {
            let (label, _) = fix_label(fix);
            let details = format!("{label} ({}).", issue.title.english());
            match ctx
                .approve(&ApprovalRequest {
                    title: label.clone(),
                    details: details.clone(),
                    confirmed: args.confirmed,
                })
                .await
            {
                Approval::Granted { .. } => {}
                Approval::NeedsConfirmation => {
                    return only(
                        "needsApproval",
                        format!(
                            "Nothing was done. Show the person this and call fix_issue again with \"confirmed\": true if they agree:\n{details}"
                        ),
                    );
                }
                Approval::Declined(why) => {
                    return only("declined", format!("{why} Nothing was done."));
                }
            }
            match backend.run_fix(issue, fix, Some(ctx.actor().clone())).await {
                Ok(message) => only("done", message),
                Err(err) => only("failed", err.to_string()),
            }
        }
    }
}

/// Which logs.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct LogsArgs {
    /// Account name or id; needed only when several accounts are connected.
    #[serde(default)]
    account: Option<String>,
    /// One of this machine's tunnels (name or id). Default: the one carrying `hostname`,
    /// or the default tunnel.
    #[serde(default)]
    tunnel: Option<String>,
    /// Only request lines of this route.
    #[serde(default)]
    hostname: Option<String>,
    /// With `hostname`: the route's path rule, if it has one.
    #[serde(default)]
    path: Option<String>,
    /// Only this level or worse: `debug`, `info`, `warn` or `error`.
    #[serde(default)]
    level: Option<String>,
    /// Only lines containing this text (case-insensitive).
    #[serde(default)]
    contains: Option<String>,
    /// At most this many lines, newest (default 100, up to 500).
    #[serde(default)]
    #[schemars(range(min = 1, max = 500))]
    limit: Option<usize>,
}

/// A log line.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LineOut {
    /// When (RFC 3339).
    time: Option<String>,
    /// Level.
    level: String,
    /// The message.
    message: String,
    /// The `error` field.
    error: Option<String>,
    /// Other fields (e.g. `ingressRule`, `originService`, `connIndex`, `location`).
    fields: serde_json::Value,
}

impl From<LogLine> for LineOut {
    fn from(line: LogLine) -> Self {
        Self {
            time: line.time,
            level: line.level,
            message: line.message,
            error: line.error,
            fields: serde_json::Value::Object(line.fields),
        }
    }
}

fn rank(level: &str) -> u8 {
    match level {
        "debug" | "trace" => 0,
        "warn" | "warning" => 2,
        "error" | "fatal" | "panic" => 3,
        _ => 1,
    }
}

fn keep(
    lines: Vec<LogLine>,
    level: Option<&str>,
    contains: Option<&str>,
    limit: usize,
) -> Vec<LineOut> {
    let floor = level.map_or(0, |l| rank(&l.to_ascii_lowercase()));
    let needle = contains.map(str::to_ascii_lowercase);
    let mut kept: Vec<LineOut> = lines
        .into_iter()
        .filter(|l| rank(&l.level) >= floor)
        .filter(|l| {
            needle.as_deref().is_none_or(|n| {
                l.message.to_ascii_lowercase().contains(n)
                    || l.error
                        .as_deref()
                        .is_some_and(|e| e.to_ascii_lowercase().contains(n))
            })
        })
        .map(LineOut::from)
        .collect();
    kept.drain(..kept.len().saturating_sub(limit));
    kept
}

/// Log lines.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LogsResult {
    /// Where they came from.
    source: String,
    /// Oldest first.
    lines: Vec<LineOut>,
    /// Why there are none, if known.
    note: Option<String>,
}

pub(super) async fn logs_tail(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: LogsArgs = arguments(args)?;
    let account = match args.hostname.as_deref() {
        Some(host) => super::account_for_hostname(backend, args.account.as_deref(), host).await?,
        None => account(backend, args.account.as_deref()).await?,
    };
    let tunnel = tunnel_id(backend, &account.id, args.tunnel.as_deref()).await?;
    let limit = args.limit.unwrap_or(100).clamp(1, MAX_LINES);
    let route = args
        .hostname
        .as_deref()
        .map(|h| (h.trim(), args.path.as_deref()));
    // Read generously, then filter.
    let batch = backend
        .logs(
            &account.id,
            tunnel.as_deref(),
            route,
            MAX_LINES.max(limit * 4),
        )
        .await?;
    let lines = keep(
        batch.lines,
        args.level.as_deref(),
        args.contains.as_deref(),
        limit,
    );
    Ok(ToolOutput::new(&LogsResult {
        note: batch
            .note
            .or_else(|| lines.is_empty().then(|| "No matching lines.".to_owned())),
        source: batch.source,
        lines,
    }))
}

/// Which connector, how long.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RemoteLogsArgs {
    /// The tunnel (name or id), from list_tunnels.
    tunnel: String,
    /// Which connector (its id, from list_tunnels). Default: the first that isn't this
    /// machine.
    #[serde(default)]
    connector: Option<String>,
    /// Account name or id; needed only when several accounts are connected.
    #[serde(default)]
    account: Option<String>,
    /// Listen this long, 1 to 30 seconds (default 5).
    #[serde(default)]
    #[schemars(range(min = 1, max = 30))]
    seconds: Option<u64>,
    /// At most this many lines, newest (default 200, up to 500).
    #[serde(default)]
    limit: Option<usize>,
}

/// Another machine's logs.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RemoteLogsResult {
    /// The tunnel.
    tunnel: String,
    /// The connector listened to.
    connector: String,
    /// `connecting`, `streaming` or `ended`.
    state: String,
    /// Why it ended, if it did.
    message: Option<String>,
    /// Oldest first.
    lines: Vec<LineOut>,
}

pub(super) async fn remote_logs(
    backend: &SharedBackend,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: RemoteLogsArgs = arguments(args)?;
    let account = account(backend, args.account.as_deref()).await?;
    let tunnels = backend.tunnels(&account.id).await?;
    let wanted = args.tunnel.trim();
    let tunnel = tunnels
        .iter()
        .find(|t| t.id == wanted || t.name.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| {
            ToolError::new(format!(
                "The account has no tunnel called \"{wanted}\". Call list_tunnels."
            ))
        })?;
    let connector = match args.connector.as_deref().map(str::trim) {
        Some(id) => tunnel
            .connectors
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| {
                ToolError::new(format!(
                    "Tunnel {} has no connector {id} right now.",
                    tunnel.name
                ))
            })?,
        None => tunnel
            .connectors
            .iter()
            .find(|c| !c.this_mac)
            .or_else(|| tunnel.connectors.first())
            .ok_or_else(|| {
                ToolError::new(format!(
                    "No machine runs {} right now, so there are no logs to stream.",
                    tunnel.name
                ))
            })?,
    };
    let seconds = args.seconds.unwrap_or(5).clamp(1, MAX_LISTEN);
    let limit = args.limit.unwrap_or(200).clamp(1, MAX_LINES);
    let mut batch = backend
        .remote_logs(&account.id, &tunnel.id, &connector.id, limit)
        .await?;
    for elapsed in 1..=seconds {
        if matches!(batch.state, RemoteLogState::Ended { .. }) {
            break;
        }
        tokio::select! {
            () = tokio::time::sleep(Duration::from_secs(1)) => {}
            () = ctx.cancelled().cancelled() => break,
        }
        ctx.progress(
            elapsed as f64,
            Some(seconds as f64),
            format!("{} line(s) so far", batch.lines.len()),
        )
        .await;
        batch = backend
            .remote_logs(&account.id, &tunnel.id, &connector.id, limit)
            .await?;
    }
    let (state, message) = match &batch.state {
        RemoteLogState::Connecting => ("connecting", None),
        RemoteLogState::Streaming => ("streaming", None),
        RemoteLogState::Ended { message } => ("ended", Some(message.english())),
    };
    Ok(ToolOutput::new(&RemoteLogsResult {
        tunnel: tunnel.name.clone(),
        connector: connector.id.clone(),
        state: state.into(),
        message,
        lines: keep(batch.lines, None, None, limit),
    }))
}

/// Which accounts.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StatusArgs {
    /// Only this account (name or id). Default: every connected account.
    #[serde(default)]
    account: Option<String>,
}

/// One account's connectors.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountStatus {
    /// The account.
    account: AccountRef,
    /// This machine's tunnels.
    tunnels: Vec<TunnelOut>,
    /// Why it couldn't be read, if it couldn't.
    error: Option<String>,
}

/// Connector health.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StatusResult {
    /// This machine's name.
    machine: String,
    /// Per account.
    accounts: Vec<AccountStatus>,
}

pub(super) async fn connector_status(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: StatusArgs = arguments(args)?;
    let accounts = match args.account.as_deref() {
        Some(wanted) => vec![account(backend, Some(wanted)).await?],
        None => backend.accounts().await?,
    };
    let mut out = Vec::new();
    for account in &accounts {
        let (tunnels, error) = match backend.tunnels(&account.id).await {
            Ok(list) => (
                list.iter().filter(|t| t.this_mac).map(tunnel_out).collect(),
                None,
            ),
            Err(err) => (Vec::new(), Some(err.to_string())),
        };
        out.push(AccountStatus {
            account: account.into(),
            tunnels,
            error,
        });
    }
    Ok(ToolOutput::new(&StatusResult {
        machine: backend.machine_name(),
        accounts: out,
    }))
}
