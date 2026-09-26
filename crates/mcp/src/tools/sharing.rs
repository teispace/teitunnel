//! Sharing tools: put a local service online, list and stop shares, find services.

use std::time::Duration;

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Hints, account_for_hostname, spec};
use crate::{
    backend::{DomainShareRequest, ShareInfo, ShareKind, SharedBackend},
    limits,
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolResult,
        ToolSpec, arguments,
    },
};

/// The longest a share may run by itself.
const MAX_MINUTES: u32 = 7 * 24 * 60;

pub(super) fn specs() -> Vec<ToolSpec> {
    vec![
        spec::<ShareArgs, ShareResult>(
            "share_port",
            "Share a local service",
            "Put a service running on this machine on the internet and get its public HTTPS URL.\n\
             \n\
             Use it to show a dev server to someone, test on a phone, or receive webhooks (Stripe, GitHub, Slack…). Two kinds:\n\
             - Without `hostname`: a Quick Share at a random https://….trycloudflare.com address. No Cloudflare account needed; public to anyone with the link.\n\
             - With `hostname` (e.g. `demo.teispace.com`, on one of the connected account's domains): a temporary route on the person's own domain, optionally behind a login (`allow`). It never takes over a hostname that already has a DNS record Teitunnel didn't create.\n\
             \n\
             The share lasts until stop_share, until `expiresInMinutes`, or until this MCP server stops (the agent session ends), whichever comes first; the Teitunnel app lists it and can stop it too. For a permanent route use plan_change with `addRoute` instead.\n\
             Call list_local_services first if you don't know the port. In `ask` mode the person approves before anything goes online.\n\
             \n\
             Examples: {\"target\": \"3000\"} · {\"target\": \"localhost:5173\", \"expiresInMinutes\": 60} · {\"target\": \"8080\", \"hostname\": \"demo.teispace.com\", \"allow\": [\"team@teispace.com\", \"@teispace.com\"]}",
            ToolClass::Change,
            Hints {
                read_only: false,
                destructive: false,
                idempotent: false,
                open_world: true,
            },
            Duration::from_secs(120),
        ),
        spec::<StopArgs, StopResult>(
            "stop_share",
            "Stop a share",
            "Stop a share: its public URL stops working at once. For a share on your domain, its route, DNS record and login (all created by Teitunnel) are removed through a plan.\n\
             \n\
             Pass the share's `id`, its URL or its hostname, as list_shares or share_port returned them. Shares this server started stop without asking; stopping one started by the person (in the app or a terminal) needs their approval in `ask` mode.\n\
             \n\
             Example: {\"share\": \"https://calm-river-1234.trycloudflare.com\"}",
            ToolClass::Destructive,
            Hints {
                read_only: false,
                destructive: true,
                idempotent: true,
                open_world: true,
            },
            Duration::from_secs(90),
        ),
        spec::<ListSharesArgs, SharesResult>(
            "list_shares",
            "List shares",
            "List every running share: Quick Shares and shares on your domains, whether started by this agent, the Teitunnel app or a terminal, with URL, local service, status and when each ends.\n\
             \n\
             Use it before share_port to reuse an existing share of the same port, and to find what to pass to stop_share.",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<ServicesArgs, ServicesResult>(
            "list_local_services",
            "List local services",
            "List services listening on this machine (TCP ports), likely dev servers first, with what each looks like (Vite, Next.js, Django, Rails, Docker…), its process and project folder, and the origin URL to share or route.\n\
             \n\
             Use it to find the port of the person's dev server before share_port or an addRoute plan. Filter by `kind` (e.g. `vite`, `docker`) or free `text` (process or project name).\n\
             \n\
             Example: {\"text\": \"my-app\"}",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
    ]
}

/// What to share.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ShareArgs {
    /// The local service: a port (`3000`), `host:port` (`127.0.0.1:8080`) or a URL
    /// (`http://localhost:5173`, `https://localhost:8443`).
    target: String,
    /// Share at this hostname on one of the account's domains (e.g.
    /// `demo.teispace.com`) instead of a random trycloudflare.com address.
    #[serde(default)]
    hostname: Option<String>,
    /// With `hostname`: the account (name or id), when several are connected.
    #[serde(default)]
    account: Option<String>,
    /// With `hostname`: require a login. Email addresses (`team@teispace.com`), whole
    /// domains (`@teispace.com`), or GitHub organizations and teams (`github:teispace/devs`).
    #[serde(default)]
    allow: Vec<String>,
    /// With `allow`: how people log in, `github` or `google` (the account's login method of
    /// that kind), or `any` (the default).
    #[serde(default)]
    sign_in: Option<String>,
    /// Stop by itself after this many minutes (1 to 10080).
    #[serde(default)]
    #[schemars(range(min = 1, max = 10080))]
    expires_in_minutes: Option<u32>,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person saw what will be shared and agreed.
    #[serde(default)]
    confirmed: bool,
}

/// A share as listed.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShareOut {
    /// Pass to stop_share.
    id: String,
    /// `quick` or `domain`.
    kind: ShareKind,
    /// The public URL.
    url: Option<String>,
    /// The local service.
    origin: String,
    /// `live`, `starting`, `reconnecting`, `failed: …` or `unknown`.
    status: String,
    /// `this agent`, `the app` or `a terminal`.
    started_by: String,
    /// Started by this server (it ends with it).
    mine: bool,
    /// The account, for shares on a domain.
    account_id: Option<String>,
    /// Started (milliseconds since the epoch).
    started_at: u64,
    /// Ends by itself (milliseconds since the epoch).
    expires_at: Option<u64>,
    /// Visitors get a "paused" page (resume_share serves it again).
    paused: bool,
}

impl From<ShareInfo> for ShareOut {
    fn from(share: ShareInfo) -> Self {
        Self {
            id: share.id,
            kind: share.kind,
            url: share.url,
            origin: share.origin,
            status: share.status,
            started_by: share.started_by,
            mine: share.mine,
            account_id: share.account_id,
            started_at: share.started_at,
            expires_at: share.expires_at,
            paused: share.paused,
        }
    }
}

/// The result of share_port.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShareResult {
    /// `shared`, `needsApproval` (ask the person, then call again with `confirmed:
    /// true`) or `declined`.
    outcome: String,
    /// The share, once shared.
    share: Option<ShareOut>,
    /// What happened, and anything to tell the person.
    message: String,
}

fn needs_approval(details: &str) -> ToolResult {
    Ok(ToolOutput::new(&ShareResult {
        outcome: "needsApproval".into(),
        share: None,
        message: format!(
            "Nothing was shared yet. This server can't ask the person directly, so show them this and call share_port again with the same arguments plus \"confirmed\": true if they agree:\n{details}"
        ),
    }))
}

pub(super) async fn share_port(
    backend: &SharedBackend,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: ShareArgs = arguments(args)?;
    let expires = args
        .expires_in_minutes
        .map(|m| {
            if (1..=MAX_MINUTES).contains(&m) {
                Ok(Duration::from_secs(u64::from(m) * 60))
            } else {
                Err(ToolError::new(
                    "expiresInMinutes must be between 1 and 10080 (7 days).",
                ))
            }
        })
        .transpose()?;
    let until = args.expires_in_minutes.map_or_else(
        || "until stopped or this agent session ends".to_owned(),
        |m| format!("for {m} min (or until stopped)"),
    );
    match args
        .hostname
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        None => {
            if !args.allow.is_empty() {
                return Err(ToolError::new(
                    "`allow` (a login) needs `hostname`: Quick Shares are public. Pass a hostname on one of the account's domains.",
                ));
            }
            let details = format!(
                "Share {} at a public trycloudflare.com URL (anyone with the link can open it), {until}.",
                args.target
            );
            match ctx
                .approve(&ApprovalRequest {
                    title: format!("Share {} publicly", args.target),
                    details: details.clone(),
                    confirmed: args.confirmed,
                })
                .await
            {
                Approval::Granted { .. } => {}
                Approval::NeedsConfirmation => return needs_approval(&details),
                Approval::Declined(why) => return declined(&why),
            }
            ctx.progress(0.0, None, "Starting a Quick Share…").await;
            let share = backend.start_quick_share(&args.target, expires).await?;
            let url = share.url.clone().unwrap_or_default();
            Ok(ToolOutput::new(&ShareResult {
                outcome: "shared".into(),
                message: format!(
                    "{} is public at {url} {until}. Anyone with the link can open it.",
                    share.origin
                ),
                share: Some(share.into()),
            })
            .with_summary(url))
        }
        Some(hostname) => {
            let account = account_for_hostname(backend, args.account.as_deref(), hostname).await?;
            let access =
                super::access_rule(&args.allow, &[], super::sign_in(args.sign_in.as_deref())?);
            let who = access.as_ref().map_or_else(
                || "public (no login)".to_owned(),
                |rule| format!("login required: {}", rule.people()),
            );
            let details = format!(
                "Share {} at https://{hostname} ({who}), {until}, in account {}. Teitunnel adds a route, a DNS record{} and removes them when the share ends.",
                args.target,
                account.name,
                if access.is_some() { " and a login" } else { "" },
            );
            match ctx
                .approve(&ApprovalRequest {
                    title: format!("Share {} at https://{hostname}", args.target),
                    details: details.clone(),
                    confirmed: args.confirmed,
                })
                .await
            {
                Approval::Granted { .. } => {}
                Approval::NeedsConfirmation => return needs_approval(&details),
                Approval::Declined(why) => return declined(&why),
            }
            ctx.progress(0.0, None, format!("Adding https://{hostname}…"))
                .await;
            let (share, outcome) = backend
                .start_domain_share(
                    DomainShareRequest {
                        account: account.id.clone(),
                        hostname: hostname.to_owned(),
                        origin: args.target.clone(),
                        access,
                        expires_in: expires,
                    },
                    Some(ctx.actor().clone()),
                )
                .await?;
            let note = match &outcome {
                teitunnel_core::engine::Outcome::Applied {
                    connector_error: Some(error),
                    ..
                } => format!(
                    " Note: {} The route is set up in Cloudflare but only answers while a connector runs on this machine.",
                    error.english()
                ),
                _ => String::new(),
            };
            let url = share.url.clone().unwrap_or_default();
            Ok(ToolOutput::new(&ShareResult {
                outcome: "shared".into(),
                message: format!(
                    "{} is at {url} ({who}) {until}. It can take a few seconds to answer while DNS settles; verify_route checks it.{note}",
                    share.origin
                ),
                share: Some(share.into()),
            })
            .with_summary(url))
        }
    }
}

fn declined(why: &str) -> ToolResult {
    Ok(ToolOutput::new(&ShareResult {
        outcome: "declined".into(),
        share: None,
        message: format!("{why} Nothing was shared."),
    }))
}

/// Which share to stop.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StopArgs {
    /// The share's id, public URL or hostname (from list_shares or share_port).
    share: String,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person agreed to stop it.
    #[serde(default)]
    confirmed: bool,
}

/// The result of stop_share.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StopResult {
    /// `stopped`, `needsApproval` or `declined`.
    outcome: String,
    /// The share.
    share: Option<ShareOut>,
    /// What happened.
    message: String,
}

/// Finds a share by id, URL or hostname.
pub(super) fn find<'a>(shares: &'a [ShareInfo], wanted: &str) -> Option<&'a ShareInfo> {
    let wanted = wanted.trim().trim_end_matches('/');
    let host = wanted
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .to_ascii_lowercase();
    shares.iter().find(|s| {
        s.id == wanted
            || s.url.as_deref().map(|u| u.trim_end_matches('/')) == Some(wanted)
            || s.url.as_deref().is_some_and(|u| {
                u.trim_start_matches("https://")
                    .trim_end_matches('/')
                    .eq_ignore_ascii_case(&host)
            })
    })
}

pub(super) async fn stop_share(
    backend: &SharedBackend,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: StopArgs = arguments(args)?;
    let shares = backend.shares().await?;
    let share = find(&shares, &args.share).cloned().ok_or_else(|| {
        ToolError::new(format!(
            "No running share matches \"{}\". Call list_shares to see them.",
            args.share
        ))
    })?;
    if !share.mine {
        let details = format!(
            "Stop the share of {} at {} (started by {}).{}",
            share.origin,
            share.url.as_deref().unwrap_or("?"),
            share.started_by,
            if share.kind == ShareKind::Domain {
                " Its route, DNS record and login are removed."
            } else {
                ""
            }
        );
        match ctx
            .approve(&ApprovalRequest {
                title: format!(
                    "Stop sharing {}",
                    share.url.as_deref().unwrap_or(&share.origin)
                ),
                details: details.clone(),
                confirmed: args.confirmed,
            })
            .await
        {
            Approval::Granted { .. } => {}
            Approval::NeedsConfirmation => {
                return Ok(ToolOutput::new(&StopResult {
                    outcome: "needsApproval".into(),
                    share: Some(share.into()),
                    message: format!(
                        "Nothing was stopped. The person started this share; show them this and call stop_share again with \"confirmed\": true if they agree:\n{details}"
                    ),
                }));
            }
            Approval::Declined(why) => {
                return Ok(ToolOutput::new(&StopResult {
                    outcome: "declined".into(),
                    share: Some(share.into()),
                    message: format!("{why} The share keeps running."),
                }));
            }
        }
    }
    backend
        .stop_share(&share, Some(ctx.actor().clone()))
        .await?;
    let url = share.url.clone().unwrap_or_default();
    Ok(ToolOutput::new(&StopResult {
        outcome: "stopped".into(),
        message: format!("Stopped sharing {} ({url}).", share.origin),
        share: Some(share.into()),
    }))
}

/// Paging.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListSharesArgs {
    /// From a previous answer's `nextCursor`.
    #[serde(default)]
    cursor: Option<String>,
    /// At most this many (default 50, up to 200).
    #[serde(default)]
    limit: Option<usize>,
}

/// Shares.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SharesResult {
    /// Running shares.
    shares: Vec<ShareOut>,
    /// How many there are in all.
    total: usize,
    /// Pass as `cursor` for the next page.
    next_cursor: Option<String>,
}

pub(super) async fn list_shares(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: ListSharesArgs = arguments(args)?;
    let shares = backend.shares().await?;
    let (shares, next_cursor, total) =
        limits::page(shares, args.cursor.as_deref(), args.limit).map_err(ToolError::new)?;
    Ok(ToolOutput::new(&SharesResult {
        shares: shares.into_iter().map(ShareOut::from).collect(),
        total,
        next_cursor,
    }))
}

/// Filters and paging.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ServicesArgs {
    /// Only this kind, e.g. `vite`, `next`, `django`, `rails`, `node`, `python`,
    /// `docker`, `database`.
    #[serde(default)]
    kind: Option<String>,
    /// Text the process, project or origin must contain.
    #[serde(default)]
    text: Option<String>,
    /// From a previous answer's `nextCursor`.
    #[serde(default)]
    cursor: Option<String>,
    /// At most this many (default 50, up to 200).
    #[serde(default)]
    limit: Option<usize>,
}

/// A listening service.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServiceOut {
    /// The port.
    port: u16,
    /// What to share or route, e.g. `http://localhost:5173`.
    origin: String,
    /// What it looks like, e.g. `vite`, `next`, `docker`, `database`, `other`.
    kind: String,
    /// The process, e.g. `node`.
    process: String,
    /// The project folder's name, if known.
    project: Option<String>,
    /// Process id.
    pid: u32,
    /// Listens on every interface (not only this machine).
    all_interfaces: bool,
}

/// Services.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServicesResult {
    /// Likely dev servers first.
    services: Vec<ServiceOut>,
    /// How many match in all.
    total: usize,
    /// Pass as `cursor` for the next page.
    next_cursor: Option<String>,
}

pub(super) async fn list_local_services(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: ServicesArgs = arguments(args)?;
    let kind = args.kind.as_deref().map(str::to_ascii_lowercase);
    let text = args.text.as_deref().map(str::to_ascii_lowercase);
    let services: Vec<ServiceOut> = backend
        .services()
        .await?
        .into_iter()
        .map(|s| ServiceOut {
            kind: serde_json::to_value(s.kind)
                .ok()
                .and_then(|v| v.as_str().map(str::to_ascii_lowercase))
                .unwrap_or_else(|| "other".into()),
            port: s.port,
            origin: s.origin,
            process: s.process,
            project: s.project,
            pid: s.pid,
            all_interfaces: s.all_interfaces,
        })
        .filter(|s| kind.as_deref().is_none_or(|k| s.kind == k))
        .filter(|s| {
            text.as_deref().is_none_or(|t| {
                s.process.to_ascii_lowercase().contains(t)
                    || s.origin.contains(t)
                    || s.project
                        .as_deref()
                        .is_some_and(|p| p.to_ascii_lowercase().contains(t))
            })
        })
        .collect();
    let (services, next_cursor, total) =
        limits::page(services, args.cursor.as_deref(), args.limit).map_err(ToolError::new)?;
    Ok(ToolOutput::new(&ServicesResult {
        services,
        total,
        next_cursor,
    }))
}
