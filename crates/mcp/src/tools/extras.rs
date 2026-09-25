//! Sharing extras (M12-06): pause and resume a share on your domain or a route, run it
//! on a schedule, and share a folder.

use std::time::Duration;

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::{
    folder_share::FolderShare,
    pause::hostname_of,
    schedule::{self, Schedule},
};

use super::{Hints, account_for_hostname, sharing::ShareOut, spec};
use crate::{
    backend::SharedBackend,
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolResult,
        ToolSpec, arguments,
    },
};

/// The longest a folder share may run by itself.
const MAX_MINUTES: u32 = 7 * 24 * 60;

pub(super) fn specs() -> Vec<ToolSpec> {
    let change = Hints {
        read_only: false,
        destructive: false,
        idempotent: true,
        open_world: true,
    };
    vec![
        spec::<PauseArgs, ExtraResult>(
            "pause_share",
            "Pause a share",
            "Pause a share on the person's domain (or one of this machine's routes) without giving up its address: the route and DNS record stay, and visitors get a friendly \"paused\" page (HTTP 503 with Retry-After) from Teitunnel's inspector until resume_share.\n\
             \n\
             Use it to take a demo offline for a moment, or while fixing something, and bring it back at the same URL. The Teitunnel process serving the route shows the page (the app, `teitunnel up`/`serve`, or the terminal running the share); a route that isn't inspected is pointed at the inspector first through a plan, and back on resume.\n\
             \n\
             A Quick Share this agent started (its trycloudflare.com URL or id) pauses the same way, at once and without asking: its inspector shows the page.\n\
             \n\
             Example: {\"share\": \"demo.teispace.com\"}",
            ToolClass::Change,
            change,
            Duration::from_secs(90),
        ),
        spec::<PauseArgs, ExtraResult>(
            "resume_share",
            "Resume a paused share",
            "Serve a paused share or route again at the same address (see pause_share).\n\
             \n\
             Example: {\"share\": \"https://demo.teispace.com\"}",
            ToolClass::Change,
            change,
            Duration::from_secs(90),
        ),
        spec::<ScheduleArgs, ExtraResult>(
            "schedule_share",
            "Run a share on a schedule",
            "Make a share on the person's domain (or a route) available only during set hours: outside them visitors get the \"paused\" page. Times are wall-clock times in a time zone (this computer's unless `timeZone` names one), so daylight saving keeps 09:00 at 09:00; a window ending before it starts runs past midnight. `off: true` removes the schedule.\n\
             \n\
             The process serving the route applies it (the app, `teitunnel up`/`serve`, or the terminal running the share). Pausing or resuming by hand holds until the schedule's next change.\n\
             \n\
             Examples: {\"share\": \"demo.teispace.com\", \"days\": \"mon-fri\", \"from\": \"09:00\", \"to\": \"18:00\"} · {\"share\": \"demo.teispace.com\", \"off\": true}",
            ToolClass::Change,
            change,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<FolderArgs, FolderResult>(
            "share_folder",
            "Share a folder",
            "Put a folder of static files (a built site like ./dist, docs, a design export) on the internet: Teitunnel's inspector serves it, with an optional file listing and single-page-app fallback. Nothing outside the folder is ever served, and secrets and tooling (dotfiles, .env files, keys, .git, node_modules) never are.\n\
             \n\
             Without `hostname` it's a Quick Share at a random trycloudflare.com URL; with `hostname` it's a temporary route on the person's domain. It ends like any share this server started (stop_share, `expiresInMinutes`, or when this session ends).\n\
             \n\
             Examples: {\"path\": \"~/site/dist\", \"spa\": true} · {\"path\": \"./public\", \"hostname\": \"docs.teispace.com\", \"listing\": true}",
            ToolClass::Change,
            Hints {
                read_only: false,
                destructive: false,
                idempotent: false,
                open_world: true,
            },
            Duration::from_secs(120),
        ),
    ]
}

/// Which share to pause or resume.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PauseArgs {
    /// The share's hostname or URL (a share on your domain or a route of this machine),
    /// or a Quick Share this agent started (its URL or id).
    share: String,
    /// The account (name or id), when several are connected.
    #[serde(default)]
    account: Option<String>,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// When a share is on.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ScheduleArgs {
    /// The share's hostname or URL.
    share: String,
    /// The account (name or id), when several are connected.
    #[serde(default)]
    account: Option<String>,
    /// Days a window starts on: `mon-fri`, `sat,sun`, `weekdays`, `weekends`, `daily`.
    #[serde(default)]
    days: Option<String>,
    /// Start, `HH:MM` (24-hour).
    #[serde(default)]
    from: Option<String>,
    /// End, `HH:MM`.
    #[serde(default)]
    to: Option<String>,
    /// An IANA time zone, e.g. `Europe/Berlin` (default: this computer's).
    #[serde(default)]
    time_zone: Option<String>,
    /// Remove the schedule.
    #[serde(default)]
    off: bool,
    /// Only when this server can't ask the person itself: the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// What happened.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExtraResult {
    /// `paused`, `resumed`, `scheduled`, `unscheduled`, `needsApproval` or `declined`.
    outcome: String,
    /// The hostname.
    hostname: String,
    /// What happened, and anything to tell the person.
    message: String,
}

/// A folder to share.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct FolderArgs {
    /// The folder (absolute, or relative to where this server runs).
    path: String,
    /// Share at this hostname on one of the account's domains instead of a random
    /// trycloudflare.com address.
    #[serde(default)]
    hostname: Option<String>,
    /// With `hostname`: the account (name or id), when several are connected.
    #[serde(default)]
    account: Option<String>,
    /// List the files of folders that have no index.html. Default: only when the folder
    /// itself has none, so its address always shows something.
    #[serde(default)]
    listing: Option<bool>,
    /// A single-page app: unknown paths get /index.html.
    #[serde(default)]
    spa: bool,
    /// Stop by itself after this many minutes (1 to 10080).
    #[serde(default)]
    #[schemars(range(min = 1, max = 10080))]
    expires_in_minutes: Option<u32>,
    /// Only when this server can't ask the person itself: the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// The result of share_folder.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FolderResult {
    /// `shared`, `needsApproval` or `declined`.
    outcome: String,
    /// The share, once shared.
    share: Option<ShareOut>,
    /// What happened.
    message: String,
}

fn result(outcome: &str, hostname: &str, message: String) -> ToolResult {
    Ok(ToolOutput::new(&ExtraResult {
        outcome: outcome.into(),
        hostname: hostname.to_owned(),
        message,
    }))
}

/// Asks for the go-ahead; `Err` carries the answer to return when it isn't given.
async fn approved(
    ctx: &ToolContext,
    title: String,
    details: &str,
    confirmed: bool,
    hostname: &str,
    tool: &str,
) -> Result<(), ToolResult> {
    match ctx
        .approve(&ApprovalRequest {
            title,
            details: details.to_owned(),
            confirmed,
        })
        .await
    {
        Approval::Granted { .. } => Ok(()),
        Approval::NeedsConfirmation => Err(result(
            "needsApproval",
            hostname,
            format!(
                "Nothing changed yet. This server can't ask the person directly, so show them this and call {tool} again with \"confirmed\": true if they agree:\n{details}"
            ),
        )),
        Approval::Declined(why) => Err(result(
            "declined",
            hostname,
            format!("{why} Nothing changed."),
        )),
    }
}

pub(super) async fn pause(
    backend: &SharedBackend,
    args: JsonObject,
    ctx: &ToolContext,
    paused: bool,
) -> ToolResult {
    let args: PauseArgs = arguments(args)?;
    let hostname = hostname_of(&args.share);
    // A Quick Share: its inspector tap shows the paused page.
    let shares = backend.shares().await?;
    if let Some(share) = super::sharing::find(&shares, &args.share)
        .filter(|s| s.kind == crate::backend::ShareKind::Quick)
    {
        if !share.mine {
            return Err(ToolError::new(format!(
                "That Quick Share was started by {}; pause it where it runs (the app's Quick Share view, or `teitunnel shares --pause`).",
                share.started_by
            )));
        }
        backend.set_quick_paused(&share.id, paused).await?;
        let url = share
            .url
            .clone()
            .unwrap_or_else(|| format!("https://{hostname}"));
        let message = if paused {
            format!(
                "{url} now shows a paused page (HTTP 503). resume_share serves it again at the same address."
            )
        } else {
            format!("{url} is served again.")
        };
        return result(
            if paused { "paused" } else { "resumed" },
            &hostname,
            message,
        );
    }
    let account = account_for_hostname(backend, args.account.as_deref(), &hostname).await?;
    let (title, details, tool) = if paused {
        (
            format!("Pause https://{hostname}"),
            format!(
                "Pause https://{hostname}: its address stays, and visitors see a \"paused\" page until it's resumed."
            ),
            "pause_share",
        )
    } else {
        (
            format!("Resume https://{hostname}"),
            format!("Serve https://{hostname} again."),
            "resume_share",
        )
    };
    if let Err(answer) = approved(ctx, title, &details, args.confirmed, &hostname, tool).await {
        return answer;
    }
    backend.set_paused(&account.id, &hostname, paused).await?;
    if paused {
        result(
            "paused",
            &hostname,
            format!(
                "https://{hostname} now shows a paused page (HTTP 503). resume_share serves it again at the same address."
            ),
        )
    } else {
        result(
            "resumed",
            &hostname,
            format!("https://{hostname} is served again."),
        )
    }
}

pub(super) async fn schedule_share(
    backend: &SharedBackend,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: ScheduleArgs = arguments(args)?;
    let hostname = hostname_of(&args.share);
    let account = account_for_hostname(backend, args.account.as_deref(), &hostname).await?;
    let schedule = if args.off {
        None
    } else {
        let (Some(days), Some(from), Some(to)) = (&args.days, &args.from, &args.to) else {
            return Err(ToolError::new(
                "Pass `days`, `from` and `to` (e.g. mon-fri, 09:00, 18:00), or `off: true` to remove the schedule.",
            ));
        };
        let days = schedule::parse_days(days).map_err(|e| ToolError::new(e.to_string()))?;
        Some(
            Schedule::new(days, from, to, args.time_zone.as_deref())
                .map_err(|e| ToolError::new(e.to_string()))?,
        )
    };
    let details = match &schedule {
        Some(s) => format!(
            "Serve https://{hostname} only on {} from {} to {} ({}); visitors see a paused page the rest of the time.",
            s.days
                .iter()
                .map(|d| d.name())
                .collect::<Vec<_>>()
                .join(", "),
            s.from,
            s.to,
            s.time_zone
                .as_deref()
                .unwrap_or("this computer's time zone"),
        ),
        None => format!("Remove the schedule of https://{hostname} (it stays as it is now)."),
    };
    if let Err(answer) = approved(
        ctx,
        format!("Schedule https://{hostname}"),
        &details,
        args.confirmed,
        &hostname,
        "schedule_share",
    )
    .await
    {
        return answer;
    }
    let scheduled = schedule.is_some();
    backend
        .set_schedule(&account.id, &hostname, schedule)
        .await?;
    result(
        if scheduled {
            "scheduled"
        } else {
            "unscheduled"
        },
        &hostname,
        details,
    )
}

pub(super) async fn share_folder(
    backend: &SharedBackend,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: FolderArgs = arguments(args)?;
    let folder = FolderShare::resolve(&args.path, args.listing, args.spa)
        .map_err(|e| ToolError::new(e.to_string()))?;
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
    let domain = match args
        .hostname
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
    {
        Some(hostname) => {
            let hostname = hostname_of(hostname);
            let account = account_for_hostname(backend, args.account.as_deref(), &hostname).await?;
            Some((account.id, hostname))
        }
        None => None,
    };
    let where_ = domain.as_ref().map_or_else(
        || "a public trycloudflare.com URL (anyone with the link can open it)".to_owned(),
        |(_, hostname)| format!("https://{hostname}"),
    );
    let details = format!(
        "Share the folder {} at {where_}. Dotfiles, .env files, keys, .git and node_modules are never served.",
        folder.path
    );
    let title = format!("Share the folder {}", folder.name());
    match ctx
        .approve(&ApprovalRequest {
            title,
            details: details.clone(),
            confirmed: args.confirmed,
        })
        .await
    {
        Approval::Granted { .. } => {}
        Approval::NeedsConfirmation => {
            return Ok(ToolOutput::new(&FolderResult {
                outcome: "needsApproval".into(),
                share: None,
                message: format!(
                    "Nothing was shared yet. Show the person this and call share_folder again with \"confirmed\": true if they agree:\n{details}"
                ),
            }));
        }
        Approval::Declined(why) => {
            return Ok(ToolOutput::new(&FolderResult {
                outcome: "declined".into(),
                share: None,
                message: format!("{why} Nothing was shared."),
            }));
        }
    }
    ctx.progress(0.0, None, "Sharing the folder…").await;
    let share = backend
        .share_folder(folder, domain, expires, Some(ctx.actor().clone()))
        .await?;
    let url = share.url.clone().unwrap_or_default();
    Ok(ToolOutput::new(&FolderResult {
        outcome: "shared".into(),
        message: format!("{} is at {url}.", share.origin),
        share: Some(share.into()),
    })
    .with_summary(url))
}
