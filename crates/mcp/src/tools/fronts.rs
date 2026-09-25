//! The offline page and webhook inboxes, a [`ToolProvider`] of their own: see what a
//! hostname has, and set or remove either. Each change is planned, shown to the person
//! and applied like every Cloudflare change (a Worker and a Worker route in front of the
//! hostname, created to fail open).

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::{
    engine::{
        Outcome,
        front::{FrontKind, InboxSettings, InboxVerify, OfflinePage},
    },
    fronts::{FrontChange, FrontView},
};

use super::{DEFAULT_TIMEOUT, Hints, account_for_hostname, plan_text, spec};
use crate::{
    backend::{ApplyApproval, BackendError, BoxFuture, SharedBackend},
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolProvider,
        ToolResult, ToolSpec, arguments,
    },
};

/// The offline page and webhook inbox tools.
pub struct FrontTools {
    backend: SharedBackend,
}

impl std::fmt::Debug for FrontTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FrontTools").finish_non_exhaustive()
    }
}

impl FrontTools {
    /// The tools over `backend`.
    pub fn new(backend: SharedBackend) -> Self {
        Self { backend }
    }
}

const CHANGE: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: true,
    open_world: true,
};

impl ToolProvider for FrontTools {
    fn tools(&self) -> Vec<ToolSpec> {
        vec![
            spec::<HostArgs, FrontsResult>(
                "get_offline_page_and_inboxes",
                "Get offline page and inboxes",
                "What stands in front of a hostname when this computer is off: its offline page (shown instead of Cloudflare's error) and webhook inboxes (webhooks kept and delivered later, in order). Read from this computer's records; nothing is fetched.\n\
                 \n\
                 Example: {\"hostname\": \"app.teispace.com\"}",
                ToolClass::Read,
                Hints::READ_LOCAL,
                DEFAULT_TIMEOUT,
            ),
            spec::<OfflineArgs, ChangeResult>(
                "set_offline_page",
                "Set the offline page",
                "Show a page of the person's own instead of Cloudflare's error while this computer is off (and, with `whenAppDown`, while the local app doesn't answer). A small Worker in front of the hostname does it; it fails open, so the site never goes down because of it, and the Workers free plan's daily requests count. `off: true` removes it. The person approves the plan first.\n\
                 \n\
                 Example: {\"hostname\": \"app.teispace.com\", \"title\": \"Back soon\", \"message\": \"We're updating the demo. Try again in an hour.\"}",
                ToolClass::Change,
                CHANGE,
                DEFAULT_TIMEOUT,
            ),
            spec::<InboxArgs, ChangeResult>(
                "set_webhook_inbox",
                "Set a webhook inbox",
                "Keep webhooks sent to a path while this computer is off or the app doesn't answer: they're stored (up to `maxItems`, for `retentionDays`) and delivered in order once it's back, and the sender gets 202 meanwhile. `verify` keeps only correctly signed ones (github, stripe or standard; the signing secret must be saved in the inspector first, e.g. `teitunnel inbox secret`). `off: true` removes the inbox. The person approves the plan first.\n\
                 \n\
                 Example: {\"hostname\": \"api.teispace.com\", \"path\": \"/webhooks/\", \"verify\": \"github\"}",
                ToolClass::Change,
                CHANGE,
                DEFAULT_TIMEOUT,
            ),
        ]
    }

    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: JsonObject,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            let backend = &self.backend;
            match name {
                "get_offline_page_and_inboxes" => get(backend, arguments).await,
                "set_offline_page" => set_offline(backend, arguments, ctx).await,
                "set_webhook_inbox" => set_inbox(backend, arguments, ctx).await,
                other => Err(ToolError::new(format!("Unknown tool {other}."))),
            }
        })
    }
}

/// A hostname.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HostArgs {
    /// The hostname, e.g. `app.teispace.com`.
    hostname: String,
    /// Account name or id; found from the hostname's domain when omitted.
    #[serde(default)]
    account: Option<String>,
}

/// A hostname's offline page and inboxes.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FrontsResult {
    /// The hostname.
    hostname: String,
    /// Its offline page, if any.
    offline_page: Option<OfflineOut>,
    /// Its webhook inboxes.
    inboxes: Vec<InboxOut>,
}

/// An offline page.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OfflineOut {
    /// Heading.
    title: String,
    /// The text.
    message: String,
    /// Also shown while the local app doesn't answer.
    when_app_down: bool,
    /// Its Worker route is in place.
    active: bool,
}

/// A webhook inbox.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct InboxOut {
    /// The path it keeps webhooks for.
    path: String,
    /// Most webhooks kept.
    max_items: u32,
    /// Days kept.
    retention_days: u32,
    /// Signature checked: `github`, `stripe` or `standard`.
    verify: Option<String>,
    /// Its Worker route is in place.
    active: bool,
}

fn verify_name(verify: Option<InboxVerify>) -> Option<String> {
    verify.map(|v| {
        match v {
            InboxVerify::Github => "github",
            InboxVerify::Stripe => "stripe",
            InboxVerify::Standard => "standard",
        }
        .to_owned()
    })
}

async fn get(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: HostArgs = arguments(args)?;
    let hostname = args.hostname.trim().to_ascii_lowercase();
    let account = account_for_hostname(backend, args.account.as_deref(), &hostname).await?;
    let fronts: Vec<FrontView> = backend
        .fronts(&account.id)
        .await?
        .into_iter()
        .filter(|f| f.hostname == hostname)
        .collect();
    let offline_page = fronts
        .iter()
        .find(|f| f.kind == FrontKind::Offline)
        .and_then(|f| {
            f.page.as_ref().map(|p| OfflineOut {
                title: p.title.clone(),
                message: p.message.clone(),
                when_app_down: p.when_app_down,
                active: f.routed,
            })
        });
    let inboxes = fronts
        .iter()
        .filter_map(|f| {
            f.inbox.as_ref().map(|i| InboxOut {
                path: f.path.clone(),
                max_items: i.max_items,
                retention_days: i.retention_days,
                verify: verify_name(i.verify),
                active: f.routed,
            })
        })
        .collect();
    Ok(ToolOutput::new(&FrontsResult {
        hostname,
        offline_page,
        inboxes,
    }))
}

/// An offline page to set, or `off`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OfflineArgs {
    /// The hostname, e.g. `app.teispace.com`.
    hostname: String,
    /// Heading (default "Back soon").
    #[serde(default)]
    title: Option<String>,
    /// A line or two for visitors.
    #[serde(default)]
    message: Option<String>,
    /// Also show it while the local app doesn't answer (502/504), not only while the
    /// computer is off.
    #[serde(default)]
    when_app_down: bool,
    /// Remove the offline page.
    #[serde(default)]
    off: bool,
    /// Account name or id; found from the hostname's domain when omitted.
    #[serde(default)]
    account: Option<String>,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// A webhook inbox to set, or `off`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct InboxArgs {
    /// The hostname, e.g. `api.teispace.com`.
    hostname: String,
    /// The path webhooks are sent to, e.g. `/webhooks/` (a prefix).
    path: String,
    /// Most webhooks kept (default 500, up to 1000).
    #[serde(default)]
    max_items: Option<u32>,
    /// Days a webhook is kept (default 7, 1 to 30).
    #[serde(default)]
    retention_days: Option<u32>,
    /// Keep only correctly signed webhooks.
    #[serde(default)]
    verify: Option<Verify>,
    /// Remove the inbox.
    #[serde(default)]
    off: bool,
    /// Account name or id; found from the hostname's domain when omitted.
    #[serde(default)]
    account: Option<String>,
    /// Only when this server can't ask the person itself: the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// Whose signatures an inbox checks.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Verify {
    /// GitHub's `X-Hub-Signature-256`.
    Github,
    /// Stripe's `Stripe-Signature`.
    Stripe,
    /// Standard Webhooks (Svix, Clerk, Resend…).
    Standard,
}

impl From<Verify> for InboxVerify {
    fn from(verify: Verify) -> Self {
        match verify {
            Verify::Github => Self::Github,
            Verify::Stripe => Self::Stripe,
            Verify::Standard => Self::Standard,
        }
    }
}

/// What a change did.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChangeResult {
    /// `applied`, `needsApproval`, `declined` or `failed`.
    outcome: String,
    /// What happened, in a sentence.
    message: String,
    /// The plan, step by step.
    plan: String,
}

async fn set_offline(backend: &SharedBackend, args: JsonObject, ctx: &ToolContext) -> ToolResult {
    let args: OfflineArgs = arguments(args)?;
    let hostname = args.hostname.trim().to_ascii_lowercase();
    let page = (!args.off).then(|| {
        let default = OfflinePage::default();
        OfflinePage {
            title: args.title.unwrap_or(default.title),
            message: args.message.unwrap_or(default.message),
            when_app_down: args.when_app_down,
        }
    });
    let title = if page.is_some() {
        format!("Show an offline page for {hostname}")
    } else {
        format!("Remove the offline page from {hostname}")
    };
    let change = FrontChange::Offline {
        hostname: hostname.clone(),
        page,
    };
    run(
        backend,
        ctx,
        args.account.as_deref(),
        &hostname,
        change,
        title,
        args.confirmed,
    )
    .await
}

async fn set_inbox(backend: &SharedBackend, args: JsonObject, ctx: &ToolContext) -> ToolResult {
    let args: InboxArgs = arguments(args)?;
    let hostname = args.hostname.trim().to_ascii_lowercase();
    let inbox = (!args.off).then(|| {
        let default = InboxSettings::default();
        InboxSettings {
            max_items: args.max_items.unwrap_or(default.max_items),
            retention_days: args.retention_days.unwrap_or(default.retention_days),
            verify: args.verify.map(Into::into),
        }
    });
    let title = if inbox.is_some() {
        format!(
            "Keep webhooks to {hostname}{} while offline",
            args.path.trim()
        )
    } else {
        format!("Remove the webhook inbox of {hostname}{}", args.path.trim())
    };
    let change = FrontChange::Inbox {
        hostname: hostname.clone(),
        path: args.path.trim().to_owned(),
        inbox,
    };
    run(
        backend,
        ctx,
        args.account.as_deref(),
        &hostname,
        change,
        title,
        args.confirmed,
    )
    .await
}

/// Plans `change`, asks the person, applies it.
async fn run(
    backend: &SharedBackend,
    ctx: &ToolContext,
    account: Option<&str>,
    hostname: &str,
    change: FrontChange,
    title: String,
    confirmed: bool,
) -> ToolResult {
    let account = account_for_hostname(backend, account, hostname).await?;
    let plan = backend.preview_front(&account.id, &change).await?;
    let details = plan_text(&plan);
    if plan.steps.is_empty() {
        return Ok(ToolOutput::new(&ChangeResult {
            outcome: "applied".into(),
            message: "Nothing to change: it's already like that.".into(),
            plan: details,
        }));
    }
    let answer = ctx
        .approve(&ApprovalRequest {
            title,
            details: details.clone(),
            confirmed,
        })
        .await;
    let refused = match answer {
        Approval::Granted { .. } => None,
        Approval::NeedsConfirmation => Some((
            "needsApproval",
            format!(
                "Nothing changed. Show the person this plan and call again with \"confirmed\": true if they agree:\n{details}"
            ),
        )),
        Approval::Declined(why) => Some(("declined", format!("{why} Nothing changed."))),
    };
    if let Some((outcome, message)) = refused {
        return Ok(ToolOutput::new(&ChangeResult {
            outcome: outcome.into(),
            message,
            plan: details,
        }));
    }
    let outcome = backend
        .apply_front(
            &account.id,
            &change,
            ApplyApproval {
                fingerprint: plan.fingerprint.clone(),
                confirmed: false,
            },
            Some(ctx.actor().clone()),
        )
        .await
        .map_err(|e| match e {
            BackendError::Stale(_) => ToolError::new(
                "Something changed in Cloudflare meanwhile. Call the tool again to review the new plan.",
            ),
            other => other.into(),
        })?;
    let (outcome, message) = match outcome {
        Outcome::Applied { .. } => ("applied", "Done.".to_owned()),
        Outcome::RolledBack { error, .. } => (
            "failed",
            format!("Failed: {}. Everything was undone.", error.english()),
        ),
        Outcome::PartiallyApplied { error, .. } => (
            "failed",
            format!(
                "Failed: {}. Some steps stayed; see Activity.",
                error.english()
            ),
        ),
    };
    Ok(ToolOutput::new(&ChangeResult {
        outcome: outcome.into(),
        message,
        plan: details,
    }))
}
