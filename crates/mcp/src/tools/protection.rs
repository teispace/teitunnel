//! Edge protection tools, a [`ToolProvider`] of their own: see what a hostname has,
//! plan rules at Cloudflare's edge (applied with `apply_plan`, like every change), and
//! create, list and revoke Access service tokens for machines.
//!
//! `service_token_create` is the one tool that hands an agent a secret: the token was
//! made for the agent's own use, after the person approved it. The secret is returned
//! once (never stored, never in Activity) and marked sensitive.

use std::sync::Arc;

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::{
    engine::{
        Change, Outcome,
        edge::{BotMode, EdgeHeaderOp, EdgeProtection, HeaderRule, LimitAction, RateLimitSpec},
    },
    protection::{ProtectionChange, ProtectionView, ServiceTokenView},
};

use super::{
    AccountRef, DEFAULT_TIMEOUT, Hints, account_for_hostname, plan_text,
    routes::{PlanOut, next_step, plan_and_store},
    spec,
};
use crate::{
    backend::{ApplyApproval, BackendError, BoxFuture, SharedBackend, Target},
    plans::Plans,
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolProvider,
        ToolResult, ToolSpec, arguments,
    },
};

/// The edge protection tools.
pub struct ProtectionTools {
    backend: SharedBackend,
    plans: Arc<Plans>,
}

impl std::fmt::Debug for ProtectionTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProtectionTools").finish_non_exhaustive()
    }
}

impl ProtectionTools {
    /// The tools over `backend`, keeping plans in `plans` (shared with `apply_plan`).
    pub fn new(backend: SharedBackend, plans: Arc<Plans>) -> Self {
        Self { backend, plans }
    }
}

const CHANGE: Hints = Hints {
    read_only: false,
    destructive: false,
    idempotent: false,
    open_world: true,
};

const REVOKE: Hints = Hints {
    read_only: false,
    destructive: true,
    idempotent: true,
    open_world: true,
};

impl ToolProvider for ProtectionTools {
    fn tools(&self) -> Vec<ToolSpec> {
        vec![
            spec::<HostArgs, ProtectionOut>(
                "get_protection",
                "Get edge protection",
                "What Teitunnel enforces at Cloudflare's edge for one hostname (bots, AI crawlers, rate limit, header rules), with the zone's plan and how much of each rule quota it uses.\n\
                 \n\
                 Call it before protect_hostname to change only what the person asked for.\n\
                 \n\
                 Example: {\"hostname\": \"app.teispace.com\"}",
                ToolClass::Read,
                Hints::READ_CLOUD,
                DEFAULT_TIMEOUT,
            ),
            spec::<ProtectArgs, PlanResult>(
                "protect_hostname",
                "Plan edge protection",
                "Plan rules at Cloudflare's edge for ONE hostname (never a whole domain): challenge or block automated clients, block AI crawlers, rate limit visitors, set or remove request and response headers. Returns a plan; nothing changes until apply_plan (the person approves it like any change).\n\
                 \n\
                 Fields you leave out keep their current value (see get_protection); `off: true` removes every rule Teitunnel added for the hostname. Rate limits need a Pro plan or higher (Free rate limits can't be limited to one hostname); periods are 10, 60, 120, 300, 600 or 3600 seconds. Teitunnel never touches rules it didn't create.\n\
                 \n\
                 Example: {\"hostname\": \"app.teispace.com\", \"bots\": \"challenge\", \"blockAiCrawlers\": true, \"responseHeaders\": [{\"name\": \"X-Robots-Tag\", \"op\": \"set\", \"value\": \"noindex\"}]}",
                // Like plan_change: it only reads Cloudflare and returns a plan.
                ToolClass::Read,
                Hints::READ_CLOUD,
                DEFAULT_TIMEOUT,
            ),
            spec::<HostArgs, TokensResult>(
                "service_token_list",
                "List service tokens",
                "Teitunnel's Access service tokens for a hostname (the ones machines use to pass its login), with their client id and expiry. Never their secrets.\n\
                 \n\
                 Example: {\"hostname\": \"api.teispace.com\"}",
                ToolClass::Read,
                Hints::READ_CLOUD,
                DEFAULT_TIMEOUT,
            ),
            spec::<CreateTokenArgs, CreateTokenResult>(
                "service_token_create",
                "Create a service token",
                "Create an Access service token so a machine (a CI job, a script, you) can pass the hostname's login by sending `CF-Access-Client-Id` and `CF-Access-Client-Secret` headers. If the hostname has no login yet, it gets one that only service tokens pass.\n\
                 \n\
                 The person approves it first. The secret is returned ONCE, marked sensitive: use it for the request at hand or hand it to the person; don't write it into files, logs or chat history, and don't call this again to see it (rotate instead in the app or with `teitunnel service-token rotate`).\n\
                 \n\
                 Example: {\"hostname\": \"api.teispace.com\", \"name\": \"CI\"}",
                ToolClass::Change,
                CHANGE,
                DEFAULT_TIMEOUT,
            ),
            spec::<RevokeTokenArgs, RevokeTokenResult>(
                "service_token_revoke",
                "Revoke a service token",
                "Revoke one of Teitunnel's service tokens for a hostname: it's taken out of the login and deleted, so machines using it are refused. This can't be undone (a new token has a new secret). The person approves it first.\n\
                 \n\
                 Example: {\"hostname\": \"api.teispace.com\", \"token\": \"CI\"}",
                ToolClass::Destructive,
                REVOKE,
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
                "get_protection" => get_protection(backend, arguments).await,
                "protect_hostname" => protect_hostname(backend, &self.plans, arguments, ctx).await,
                "service_token_list" => list_tokens(backend, arguments).await,
                "service_token_create" => create_token(backend, arguments, ctx).await,
                "service_token_revoke" => revoke_token(backend, arguments, ctx).await,
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

async fn get_protection(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: HostArgs = arguments(args)?;
    let account = account_for_hostname(backend, args.account.as_deref(), &args.hostname).await?;
    let view = backend
        .protection(&account.id, args.hostname.trim())
        .await?;
    Ok(ToolOutput::new(&ProtectionOut::from(view)))
}

/// A rate limit, as read.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RateLimitOut {
    /// Requests allowed per period per visitor.
    requests: u32,
    /// The period, in seconds.
    period_seconds: u32,
    /// `block` or `challenge` above it.
    action: String,
}

/// A header rule, as read.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct HeaderOut {
    /// Header name.
    name: String,
    /// `set`, `add` or `remove`.
    op: String,
    /// The value.
    value: Option<String>,
}

/// A rule quota.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct QuotaOut {
    /// `custom`, `rateLimit` or `transform`.
    kind: String,
    /// Rules the zone has (Teitunnel's and others').
    used: u32,
    /// What its plan allows.
    limit: u32,
}

/// A hostname's edge protection.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProtectionOut {
    /// The hostname.
    hostname: String,
    /// Its domain.
    zone: String,
    /// The domain's Cloudflare plan: `free`, `pro`, `business` or `enterprise`.
    plan: String,
    /// Automated clients: `off`, `challenge` or `block`.
    bots: String,
    /// AI crawlers are blocked.
    block_ai_crawlers: bool,
    /// The rate limit, if any.
    rate_limit: Option<RateLimitOut>,
    /// Whether the plan's rate limits can match one hostname (Pro and up).
    rate_limit_available: bool,
    /// The longest rate limit period the plan allows, in seconds.
    longest_period_seconds: u32,
    /// Other hostnames sharing its rate limit rule.
    shares_rate_limit_with: Vec<String>,
    /// Request header rules.
    request_headers: Vec<HeaderOut>,
    /// Response header rules.
    response_headers: Vec<HeaderOut>,
    /// The domain's rule quotas.
    quotas: Vec<QuotaOut>,
}

fn name_of(value: impl Serialize) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn headers_out(list: Vec<HeaderRule>) -> Vec<HeaderOut> {
    list.into_iter()
        .map(|h| HeaderOut {
            name: h.name,
            op: name_of(h.op),
            value: h.value,
        })
        .collect()
}

impl From<ProtectionView> for ProtectionOut {
    fn from(view: ProtectionView) -> Self {
        let p = view.protection;
        Self {
            hostname: view.hostname,
            zone: view.zone,
            plan: name_of(view.plan),
            bots: name_of(p.bots),
            block_ai_crawlers: p.ai_crawlers,
            rate_limit: p.rate_limit.map(|l| RateLimitOut {
                requests: l.requests,
                period_seconds: l.period,
                action: name_of(l.action),
            }),
            rate_limit_available: view.rate_limit_available,
            longest_period_seconds: view.longest_period,
            shares_rate_limit_with: view.shares_rate_limit_with,
            request_headers: headers_out(p.request_headers),
            response_headers: headers_out(p.response_headers),
            quotas: view
                .quotas
                .into_iter()
                .map(|q| QuotaOut {
                    kind: name_of(q.quota),
                    used: q.used,
                    limit: q.limit,
                })
                .collect(),
        }
    }
}

/// A service token (never its secret).
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TokenOut {
    /// Token id.
    id: String,
    /// Its name.
    name: String,
    /// The `CF-Access-Client-Id` value (not a secret).
    client_id: String,
    /// When it stops working.
    expires_at: Option<String>,
    /// Deleted in the Cloudflare dashboard.
    gone: bool,
}

impl From<ServiceTokenView> for TokenOut {
    fn from(token: ServiceTokenView) -> Self {
        Self {
            id: token.id,
            name: token.label,
            client_id: token.client_id,
            expires_at: token.expires_at,
            gone: token.gone,
        }
    }
}

/// What to do with automated clients.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Bots {
    /// Nothing.
    Off,
    /// A managed challenge (people pass, scripts don't).
    Challenge,
    /// Refused.
    Block,
}

/// A rate limit.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RateLimitArgs {
    /// Requests allowed per period per visitor (IP address).
    requests: u32,
    /// The period in seconds: 10, 60, 120, 300, 600 or 3600.
    period_seconds: u32,
    /// `block` (default) or `challenge` above the limit.
    #[serde(default)]
    challenge: bool,
}

/// A header change.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct HeaderArgs {
    /// Header name.
    name: String,
    /// `set`, `add` (response headers only) or `remove`.
    op: String,
    /// The value, for `set` and `add`.
    #[serde(default)]
    value: Option<String>,
}

/// The protection wanted.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ProtectArgs {
    /// The hostname.
    hostname: String,
    /// Account name or id; found from the hostname's domain when omitted.
    #[serde(default)]
    account: Option<String>,
    /// Remove every rule Teitunnel added for the hostname (other fields are ignored).
    #[serde(default)]
    off: bool,
    /// Automated clients: `off`, `challenge` or `block`.
    #[serde(default)]
    bots: Option<Bots>,
    /// Block Cloudflare's verified AI crawlers.
    #[serde(default)]
    block_ai_crawlers: Option<bool>,
    /// A rate limit; `null` keeps the current one. Use `removeRateLimit` to drop it.
    #[serde(default)]
    rate_limit: Option<RateLimitArgs>,
    /// Remove the rate limit.
    #[serde(default)]
    remove_rate_limit: bool,
    /// The full list of request header rules (replaces the current list when given).
    #[serde(default)]
    request_headers: Option<Vec<HeaderArgs>>,
    /// The full list of response header rules (replaces the current list when given).
    #[serde(default)]
    response_headers: Option<Vec<HeaderArgs>>,
}

fn header_rules(list: Vec<HeaderArgs>) -> Result<Vec<HeaderRule>, ToolError> {
    list.into_iter()
        .map(|h| {
            let op = match h.op.trim() {
                "set" => EdgeHeaderOp::Set,
                "add" => EdgeHeaderOp::Add,
                "remove" => EdgeHeaderOp::Remove,
                other => {
                    return Err(ToolError::new(format!(
                        "`{other}` isn't a header operation; use set, add or remove."
                    )));
                }
            };
            Ok(HeaderRule {
                name: h.name,
                op,
                value: h.value,
            })
        })
        .collect()
}

impl ProtectArgs {
    /// `current` with what these arguments name changed.
    fn apply_to(self, current: &EdgeProtection) -> Result<EdgeProtection, ToolError> {
        if self.off {
            return Ok(EdgeProtection::default());
        }
        let mut next = current.clone();
        if let Some(bots) = self.bots {
            next.bots = match bots {
                Bots::Off => BotMode::Off,
                Bots::Challenge => BotMode::Challenge,
                Bots::Block => BotMode::Block,
            };
        }
        if let Some(ai) = self.block_ai_crawlers {
            next.ai_crawlers = ai;
        }
        if let Some(limit) = self.rate_limit {
            next.rate_limit = Some(RateLimitSpec {
                requests: limit.requests,
                period: limit.period_seconds,
                action: if limit.challenge {
                    LimitAction::Challenge
                } else {
                    LimitAction::Block
                },
            });
        }
        if self.remove_rate_limit {
            next.rate_limit = None;
        }
        if let Some(list) = self.request_headers {
            next.request_headers = header_rules(list)?;
        }
        if let Some(list) = self.response_headers {
            next.response_headers = header_rules(list)?;
        }
        Ok(next)
    }
}

/// A planned protection change.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanResult {
    /// The plan (apply it with apply_plan).
    plan: PlanOut,
    /// The account.
    account: AccountRef,
    /// What to do next.
    next: String,
}

async fn protect_hostname(
    backend: &SharedBackend,
    plans: &Plans,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: ProtectArgs = arguments(args)?;
    let account = account_for_hostname(backend, args.account.as_deref(), &args.hostname).await?;
    let hostname = args.hostname.trim().to_owned();
    let current = backend.protection(&account.id, &hostname).await?;
    let protection = args.apply_to(&current.protection)?;
    let summary = if protection == EdgeProtection::default() {
        format!("Remove edge protection from {hostname}")
    } else {
        format!("Protect {hostname} at Cloudflare's edge")
    };
    let plan = plan_and_store(
        backend,
        plans,
        Target {
            account: account.id.clone(),
            tunnel: None,
        },
        Change::ProtectHostname {
            hostname,
            protection,
        },
        summary,
        ctx,
    )
    .await?;
    let next = next_step(&plan, ctx);
    let steps = plan.steps_len();
    Ok(ToolOutput::new(&PlanResult {
        plan,
        account: (&account).into(),
        next,
    })
    .with_summary(format!(
        "A plan of {steps} step(s); nothing has changed yet."
    )))
}

/// Service tokens.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TokensResult {
    /// The hostname.
    hostname: String,
    /// Teitunnel's tokens for it.
    tokens: Vec<TokenOut>,
}

async fn list_tokens(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: HostArgs = arguments(args)?;
    let account = account_for_hostname(backend, args.account.as_deref(), &args.hostname).await?;
    let tokens = backend
        .service_tokens(&account.id, args.hostname.trim())
        .await?;
    Ok(ToolOutput::new(&TokensResult {
        hostname: args.hostname.trim().to_owned(),
        tokens: tokens.into_iter().map(TokenOut::from).collect(),
    }))
}

/// A token to create.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CreateTokenArgs {
    /// The hostname it passes the login of.
    hostname: String,
    /// What it's for, e.g. `CI` (1–40 characters).
    name: String,
    /// Account name or id; found from the hostname's domain when omitted.
    #[serde(default)]
    account: Option<String>,
    /// The person reviewed this and agreed (only when this server can't ask them).
    #[serde(default)]
    confirmed: bool,
}

/// A new token's credentials.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Credentials {
    /// Send as the `CF-Access-Client-Id` header.
    client_id: String,
    /// Send as the `CF-Access-Client-Secret` header. Shown once; keep it out of files
    /// and logs.
    client_secret: String,
    /// When it stops working.
    expires_at: Option<String>,
    /// Always true: this value is a credential.
    sensitive: bool,
}

/// How creating a token ended.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct CreateTokenResult {
    /// `created`, `needsApproval`, `declined`, `nothingToDo` or `failed`.
    outcome: String,
    /// The credentials, once, when created.
    credentials: Option<Credentials>,
    /// What happened.
    message: String,
    /// The plan as the person should read it.
    plan: String,
}

fn failure(outcome: &Outcome) -> Option<String> {
    match outcome {
        Outcome::Applied { .. } => None,
        Outcome::RolledBack { error, .. } => Some(format!(
            "Failed: {}. Everything was undone.",
            error.english()
        )),
        Outcome::PartiallyApplied {
            error, leftovers, ..
        } => Some(format!(
            "Failed: {}. Left in place: {}.",
            error.english(),
            leftovers
                .iter()
                .map(teitunnel_core::text::Text::english)
                .collect::<Vec<_>>()
                .join("; ")
        )),
    }
}

/// Asks for approval; `Err` carries the answer to return instead of going ahead.
async fn approve(
    ctx: &ToolContext,
    title: String,
    details: String,
    confirmed: bool,
) -> Result<(), (String, String)> {
    match ctx
        .approve(&ApprovalRequest {
            title,
            details: details.clone(),
            confirmed,
        })
        .await
    {
        Approval::Granted { .. } => Ok(()),
        Approval::NeedsConfirmation => Err((
            "needsApproval".into(),
            format!(
                "Nothing changed. Show the person this plan and call again with \"confirmed\": true if they agree:\n{details}"
            ),
        )),
        Approval::Declined(why) => Err(("declined".into(), format!("{why} Nothing changed."))),
    }
}

async fn create_token(backend: &SharedBackend, args: JsonObject, ctx: &ToolContext) -> ToolResult {
    let args: CreateTokenArgs = arguments(args)?;
    let account = account_for_hostname(backend, args.account.as_deref(), &args.hostname).await?;
    let change = ProtectionChange::CreateToken {
        hostname: args.hostname.trim().to_owned(),
        label: args.name.clone(),
    };
    let plan = backend.preview_protection(&account.id, &change).await?;
    let details = plan_text(&plan);
    let title = format!(
        "Create service token “{}” for {}",
        args.name.trim(),
        args.hostname.trim()
    );
    if let Err((outcome, message)) = approve(ctx, title, details.clone(), args.confirmed).await {
        return Ok(ToolOutput::new(&CreateTokenResult {
            outcome,
            credentials: None,
            message,
            plan: details,
        }));
    }
    let (outcome, issued) = apply(backend, &account.id, &change, &plan.fingerprint, ctx).await?;
    if let Some(message) = failure(&outcome) {
        return Ok(ToolOutput::new(&CreateTokenResult {
            outcome: "failed".into(),
            credentials: None,
            message,
            plan: details,
        }));
    }
    let credentials = issued.into_iter().next().map(|t| Credentials {
        client_id: t.client_id,
        client_secret: t.client_secret.expose().clone(),
        expires_at: t.expires_at,
        sensitive: true,
    });
    Ok(ToolOutput::new(&CreateTokenResult {
        outcome: "created".into(),
        credentials,
        message: "Created. The secret is shown this once: use it, don't store or repeat it.".into(),
        plan: details,
    })
    .with_summary("Service token created (credentials below are sensitive; shown once)."))
}

async fn apply(
    backend: &SharedBackend,
    account: &str,
    change: &ProtectionChange,
    fingerprint: &str,
    ctx: &ToolContext,
) -> Result<(Outcome, Vec<teitunnel_core::engine::edge::IssuedToken>), ToolError> {
    backend
        .apply_protection(
            account,
            change,
            ApplyApproval {
                fingerprint: fingerprint.to_owned(),
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
        })
}

/// A token to revoke.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct RevokeTokenArgs {
    /// The hostname.
    hostname: String,
    /// The token's name (as listed) or id.
    token: String,
    /// Account name or id; found from the hostname's domain when omitted.
    #[serde(default)]
    account: Option<String>,
    /// The person reviewed this and agreed (only when this server can't ask them).
    #[serde(default)]
    confirmed: bool,
}

/// How revoking ended.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RevokeTokenResult {
    /// `revoked`, `needsApproval`, `declined` or `failed`.
    outcome: String,
    /// What happened.
    message: String,
    /// The plan as the person should read it.
    plan: String,
}

async fn revoke_token(backend: &SharedBackend, args: JsonObject, ctx: &ToolContext) -> ToolResult {
    let args: RevokeTokenArgs = arguments(args)?;
    let account = account_for_hostname(backend, args.account.as_deref(), &args.hostname).await?;
    let hostname = args.hostname.trim().to_owned();
    let tokens = backend.service_tokens(&account.id, &hostname).await?;
    let wanted = args.token.trim();
    let token = tokens
        .iter()
        .find(|t| t.id == wanted || t.label.eq_ignore_ascii_case(wanted))
        .ok_or_else(|| {
            ToolError::new(format!(
                "{hostname} has no service token called \"{wanted}\". Call service_token_list to see them."
            ))
        })?;
    let change = ProtectionChange::RevokeToken {
        hostname: hostname.clone(),
        token_id: token.id.clone(),
    };
    let plan = backend.preview_protection(&account.id, &change).await?;
    let details = plan_text(&plan);
    let title = format!("Revoke service token “{}” of {hostname}", token.label);
    if let Err((outcome, message)) = approve(ctx, title, details.clone(), args.confirmed).await {
        return Ok(ToolOutput::new(&RevokeTokenResult {
            outcome,
            message,
            plan: details,
        }));
    }
    let (outcome, _) = apply(backend, &account.id, &change, &plan.fingerprint, ctx).await?;
    let (outcome, message) = match failure(&outcome) {
        Some(message) => ("failed".to_owned(), message),
        None => (
            "revoked".to_owned(),
            format!("Revoked “{}”: machines using it are refused.", token.label),
        ),
    };
    Ok(ToolOutput::new(&RevokeTokenResult {
        outcome,
        message,
        plan: details,
    }))
}
