//! Teitunnel's own tools (one [`ToolProvider`]): sharing, routes, diagnostics, setup
//! and traffic. Descriptions are written for models: what the tool is for, when to use
//! it (and when another tool fits better), and an example.

mod diagnostics;
mod routes;
mod setup;
mod sharing;
mod traffic;

use std::{sync::Arc, time::Duration};

use rmcp::{
    handler::server::common::{schema_for_input, schema_for_output},
    model::{JsonObject, Tool, ToolAnnotations},
};
use schemars::JsonSchema;
use serde::Serialize;
use teitunnel_core::{
    accounts::Account,
    engine::{AccessRule, PlanView, Warning},
};

use crate::{
    backend::{BoxFuture, SharedBackend},
    plans::Plans,
    registry::{ToolClass, ToolContext, ToolError, ToolProvider, ToolResult, ToolSpec},
    traffic::TrafficSource,
};

/// The default time a tool may take.
pub(crate) const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Teitunnel's tools.
pub struct CoreTools {
    backend: SharedBackend,
    traffic: Arc<dyn TrafficSource>,
    plans: Arc<Plans>,
}

impl std::fmt::Debug for CoreTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreTools").finish_non_exhaustive()
    }
}

impl CoreTools {
    /// The tools over `backend`, with traffic from `traffic` and reviewed plans kept in
    /// `plans`.
    pub fn new(backend: SharedBackend, traffic: Arc<dyn TrafficSource>, plans: Arc<Plans>) -> Self {
        Self {
            backend,
            traffic,
            plans,
        }
    }
}

/// How a tool behaves, for its annotations.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Hints {
    pub(crate) read_only: bool,
    pub(crate) destructive: bool,
    pub(crate) idempotent: bool,
    /// Talks to Cloudflare or the internet (vs. only this machine).
    pub(crate) open_world: bool,
}

impl Hints {
    pub(crate) const READ_LOCAL: Self = Self {
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
    };
    pub(crate) const READ_CLOUD: Self = Self {
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
    };
}

/// A tool's listing, with schemas generated from its input and output types.
pub(crate) fn spec<I: JsonSchema + 'static, O: JsonSchema + 'static>(
    name: &'static str,
    title: &'static str,
    description: &'static str,
    class: ToolClass,
    hints: Hints,
    timeout: Duration,
) -> ToolSpec {
    let input = schema_for_input::<I>()
        .unwrap_or_else(|_| rmcp::handler::server::common::schema_for_empty_input());
    let annotations = ToolAnnotations::with_title(title)
        .read_only(hints.read_only)
        .destructive(hints.destructive)
        .idempotent(hints.idempotent)
        .open_world(hints.open_world);
    let tool = Tool::new(name, description, input)
        .with_title(title)
        .with_raw_output_schema(schema_for_output::<O>())
        .annotate(annotations);
    ToolSpec {
        tool,
        class,
        timeout,
    }
}

/// No arguments.
#[derive(Debug, Default, serde::Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NoArgs {}

impl ToolProvider for CoreTools {
    fn tools(&self) -> Vec<ToolSpec> {
        let mut tools = sharing::specs();
        tools.extend(routes::specs());
        tools.extend(diagnostics::specs());
        tools.extend(setup::specs());
        tools.extend(traffic::specs());
        tools
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
                "share_port" => sharing::share_port(backend, arguments, ctx).await,
                "stop_share" => sharing::stop_share(backend, arguments, ctx).await,
                "list_shares" => sharing::list_shares(backend, arguments).await,
                "list_local_services" => sharing::list_local_services(backend, arguments).await,
                "list_routes" => routes::list_routes(backend, arguments).await,
                "list_domains" => routes::list_domains(backend, arguments).await,
                "list_tunnels" => routes::list_tunnels(backend, arguments).await,
                "plan_change" => routes::plan_change(backend, &self.plans, arguments, ctx).await,
                "apply_plan" => routes::apply_plan(backend, &self.plans, arguments, ctx).await,
                "verify_route" => routes::verify_route(backend, arguments, ctx).await,
                "undo_last" => routes::undo_last(backend, &self.plans, arguments, ctx).await,
                "doctor" => diagnostics::doctor(backend, arguments).await,
                "fix_issue" => diagnostics::fix_issue(backend, &self.plans, arguments, ctx).await,
                "logs_tail" => diagnostics::logs_tail(backend, arguments).await,
                "remote_logs" => diagnostics::remote_logs(backend, arguments, ctx).await,
                "connector_status" => diagnostics::connector_status(backend, arguments).await,
                "export_config" => setup::export_config(backend, arguments).await,
                "import_scan" => setup::import_scan(backend, arguments).await,
                "accounts" => setup::accounts(backend, arguments).await,
                "recent_activity" => setup::recent_activity(backend, arguments).await,
                "traffic_list" => traffic::list(&*self.traffic, arguments, ctx).await,
                "traffic_get" => traffic::get(&*self.traffic, arguments, ctx).await,
                "traffic_replay" => traffic::replay(&*self.traffic, arguments, ctx).await,
                "wait_for_request" => {
                    traffic::wait_for_request(&*self.traffic, arguments, ctx).await
                }
                "traffic_stats" => traffic::stats(&*self.traffic, arguments).await,
                "traffic_export" => traffic::export(&*self.traffic, arguments, ctx).await,
                other => Err(ToolError::new(format!("Unknown tool {other}."))),
            }
        })
    }
}

/// An account in results.
#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccountRef {
    /// Account id.
    pub(crate) id: String,
    /// Its name.
    pub(crate) name: String,
}

impl From<&Account> for AccountRef {
    fn from(account: &Account) -> Self {
        Self {
            id: account.id.clone(),
            name: account.name.clone(),
        }
    }
}

/// The account named (id or name), or the only one.
pub(crate) async fn account(
    backend: &SharedBackend,
    wanted: Option<&str>,
) -> Result<Account, ToolError> {
    let accounts = backend.accounts().await?;
    match wanted.map(str::trim).filter(|w| !w.is_empty()) {
        Some(wanted) => accounts
            .into_iter()
            .find(|a| a.id == wanted || a.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| {
                ToolError::new(format!(
                    "No connected account called \"{wanted}\". Call the accounts tool to see them."
                ))
            }),
        None => match accounts.len() {
            0 => Err(ToolError::new(
                "No Cloudflare account is connected. The person connects one in the Teitunnel app (or with `teitunnel setup`). Quick Shares (share_port without a hostname) work without one.",
            )),
            1 => accounts
                .into_iter()
                .next()
                .ok_or_else(|| ToolError::new("No account.")),
            _ => Err(ToolError::new(format!(
                "Several accounts are connected; pass `account` (one of: {}).",
                accounts
                    .iter()
                    .map(|a| format!("{} ({})", a.name, a.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))),
        },
    }
}

/// The account whose domains include `hostname`'s zone, when none is named and several
/// are connected; otherwise as [`account`].
pub(crate) async fn account_for_hostname(
    backend: &SharedBackend,
    wanted: Option<&str>,
    hostname: &str,
) -> Result<Account, ToolError> {
    if wanted.is_some() {
        return account(backend, wanted).await;
    }
    let accounts = backend.accounts().await?;
    if accounts.len() <= 1 {
        return account(backend, None).await;
    }
    let host = hostname.trim().trim_end_matches('.').to_ascii_lowercase();
    for candidate in &accounts {
        let Ok(domains) = backend.domains(&candidate.id).await else {
            continue;
        };
        let owns = domains.as_array().is_some_and(|list| {
            list.iter()
                .filter_map(|d| d["name"].as_str())
                .any(|zone| host == zone || host.ends_with(&format!(".{zone}")))
        });
        if owns {
            return Ok(candidate.clone());
        }
    }
    account(backend, None).await
}

/// One of this machine's tunnels in `account`, by name or id.
pub(crate) async fn tunnel_id(
    backend: &SharedBackend,
    account: &str,
    wanted: Option<&str>,
) -> Result<Option<String>, ToolError> {
    let Some(wanted) = wanted.map(str::trim).filter(|w| !w.is_empty()) else {
        return Ok(None);
    };
    let overview = backend.overview(account).await?;
    overview
        .tunnels
        .iter()
        .find(|t| t.id == wanted || t.name.eq_ignore_ascii_case(wanted))
        .map(|t| Some(t.id.clone()))
        .ok_or_else(|| {
            ToolError::new(format!(
                "This machine has no tunnel called \"{wanted}\" (it has: {}). Call list_tunnels to see them.",
                overview
                    .tunnels
                    .iter()
                    .map(|t| t.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
}

/// `allow` values as a login rule: `me@xyz.com` is a person, `@xyz.com` (or
/// `xyz.com`) everyone at a domain. The engine validates them.
pub(crate) fn access_rule(allow: &[String]) -> Option<AccessRule> {
    if allow.is_empty() {
        return None;
    }
    let (emails, domains): (Vec<&String>, Vec<&String>) = allow
        .iter()
        .partition(|a| a.trim().find('@').is_some_and(|at| at > 0));
    Some(AccessRule {
        emails: emails.into_iter().map(|e| e.trim().to_owned()).collect(),
        email_domains: domains.into_iter().map(|d| d.trim().to_owned()).collect(),
    })
}

/// A plan warning in English.
pub(crate) fn warning_text(warning: &Warning) -> String {
    match warning {
        Warning::ReplacesForeignRecord {
            hostname,
            kind,
            content,
        } => format!(
            "{hostname} already has a {kind} record ({content}) that Teitunnel didn't create. It will be replaced."
        ),
        Warning::DeletesForeignRecord {
            hostname,
            kind,
            content,
        } => format!(
            "The {kind} record for {hostname} ({content}) wasn't created by Teitunnel. It will be deleted."
        ),
        Warning::KeepsForeignRecord { hostname } => format!(
            "The DNS record for {hostname} wasn't created by Teitunnel, so it's left in place."
        ),
        Warning::SingleEndpoint { hostname } => format!(
            "Only this machine serves {hostname} so far: add the same route on another machine for the load balancer to fail over to."
        ),
        Warning::TunnelEmpty => {
            "No routes will be left. The tunnel stays, so adding a route later is quick.".into()
        }
        Warning::RemoteOrigin { origin } => {
            format!("{origin} isn't on this machine. It must be reachable from here.")
        }
        Warning::PublicNetwork { network } => format!(
            "{network} isn't a private range. WARP clients would reach those addresses through this machine instead of the internet."
        ),
        Warning::OverlapsNetwork {
            network,
            other,
            tunnel,
        } => format!(
            "{network} overlaps {other}, which goes through tunnel \"{tunnel}\". For addresses in both, the narrower range wins."
        ),
        Warning::HeldBy {
            hostname,
            owner,
            until,
            kind,
        } => format!(
            "{} Applying it takes the name over, so it needs the person's confirmation.",
            teitunnel_core::reservations::describe(&teitunnel_core::engine::Hold {
                hostname: hostname.clone(),
                owner: owner.clone(),
                until: *until,
                kind: *kind,
            })
            .english()
        ),
    }
}

/// A plan as the person should read it (for approvals).
pub(crate) fn plan_text(view: &PlanView) -> String {
    let mut text = String::new();
    for warning in &view.warnings {
        text.push_str("! ");
        text.push_str(&warning_text(warning));
        text.push('\n');
    }
    for (index, step) in view.steps.iter().enumerate() {
        text.push_str(&format!("{}. {}\n", index + 1, step.description.english()));
    }
    if view.requires_confirmation {
        text.push_str(
            "\nThis replaces or deletes DNS records Teitunnel didn't create (see above).\n",
        );
    }
    text
}

#[cfg(test)]
pub(crate) mod tests;
