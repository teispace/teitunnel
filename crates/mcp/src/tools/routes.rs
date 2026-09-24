//! Route tools: read this machine's routes, domains and tunnels; change them through
//! reviewed plans (plan_change → apply_plan); check them end to end; undo.

use std::time::Duration;

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::{
    domain::OriginOptions,
    engine::{ActivityEntry, ActivityKind, Change, DeltaArea, Outcome, RouteInput, StepState},
};

use super::{AccountRef, Hints, account, account_for_hostname, plan_text, spec, tunnel_id};
use crate::{
    backend::{ApplyApproval, BackendError, SharedBackend, Target},
    config::Mode,
    limits,
    plans::{PendingPlan, Plans},
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolResult,
        ToolSpec, arguments,
    },
};

/// How long a fresh route may take to answer when checked after applying.
const VERIFY_AFTER_APPLY: Duration = Duration::from_secs(20);

pub(super) fn specs() -> Vec<ToolSpec> {
    vec![
        spec::<ListRoutesArgs, RoutesResult>(
            "list_routes",
            "List routes",
            "List this machine's routes (public hostname → local service) in a Cloudflare account, with each route's live status (live, noDns, dnsElsewhere, connecting, stopped…), the tunnel carrying it, its login (who may open it) and origin settings.\n\
             \n\
             Start here to see what's online. A route that isn't `live` has a reason in `statusText`; doctor and verify_route explain more. Filter with `status: \"down\"` to see only broken ones.\n\
             \n\
             Example: {\"status\": \"down\"}",
            ToolClass::Read,
            Hints::READ_CLOUD,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<AccountArgs, DomainsResult>(
            "list_domains",
            "List domains",
            "List the domains (Cloudflare zones) of an account: the hostnames routes and shares can use are these domains and their subdomains (e.g. `app.example.com` in `example.com`). A domain whose status isn't `active` can't serve routes yet.",
            ToolClass::Read,
            Hints::READ_CLOUD,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<AccountArgs, TunnelsResult>(
            "list_tunnels",
            "List tunnels",
            "List every Cloudflare Tunnel in the account (this machine's first), with Cloudflare's status, how many routes each has, which machines run it (connectors with version and edge locations), and whether this machine runs it.\n\
             \n\
             Routes go on this machine's default tunnel unless a plan names another `tunnel`. Use connector_status for the health of this machine's connectors, remote_logs for another machine's.",
            ToolClass::Read,
            Hints::READ_CLOUD,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<PlanArgs, PlanResult>(
            "plan_change",
            "Plan a change",
            "Plan a change to routes, logins, load balancing, private networks or tunnels, and show exactly what would happen. Nothing changes: this only reads Cloudflare and returns the plan (ordered steps, warnings, whether it touches records Teitunnel didn't create) with a `planId` and `fingerprint`. Apply it with apply_plan.\n\
             \n\
             Always show the person the plan's steps and warnings before applying. An empty plan means nothing needs to change.\n\
             \n\
             Change types (field `type`):\n\
             - addRoute {hostname, origin, path?, allow?, options?}: route a hostname on the account's domains to a local service (port, host:port or URL, also ssh://, rdp://, tcp://). `allow` requires a login (emails or @domains).\n\
             - updateRoute {hostname, path?, origin?, newHostname?, newPath?, allow?, options?}: change a route; omitted fields stay as they are.\n\
             - removeRoute {hostname, path?}: remove a route and the DNS record Teitunnel created for it.\n\
             - requireLogin {hostname, path?, allow}: put a route behind a login (Cloudflare Access, free with Zero Trust). removeLogin {hostname, path?}: make it public again.\n\
             - balanceRoute / unbalanceRoute {hostname}: load balance a hostname across every machine routing it (Cloudflare Load Balancing, paid).\n\
             - addNetwork / removeNetwork {network}: let WARP clients reach a private range (e.g. 192.168.1.0/24) through this machine.\n\
             - createTunnel {name}: another tunnel for this machine. deleteTunnel {tunnel}: delete one of this machine's tunnels with its routes.\n\
             - importRoutes {routes: [{hostname, origin, path?}]}: add several routes at once (e.g. from import_scan).\n\
             - restoreConfig {}: undo an outside edit of this machine's routes (made in the dashboard).\n\
             - deleteDnsRecord {zoneId, hostname, recordId}: delete one DNS record (orphans the doctor found).\n\
             \n\
             Example: {\"change\": {\"type\": \"addRoute\", \"hostname\": \"app.example.com\", \"origin\": \"3000\", \"allow\": [\"@example.com\"]}}",
            ToolClass::Read,
            Hints::READ_CLOUD,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<ApplyArgs, ApplyResult>(
            "apply_plan",
            "Apply a plan",
            "Apply a plan from plan_change (by `planId` and `fingerprint`), step by step with progress, then check the affected routes end to end. If anything fails, completed steps are undone. If Cloudflare changed since the plan was made, nothing is applied and a new plan is returned (`stale`) to review again.\n\
             \n\
             In `ask` mode the person approves first: the client asks them when it can; otherwise the answer is `needsApproval` and you must show them the plan and call again with `confirmed: true` only after they agree. A plan that replaces or deletes DNS records Teitunnel didn't create (`requiresConfirmation`) always needs `confirmed: true` (or the person's approval). Every change is recorded in Teitunnel's Activity with this agent's name.\n\
             \n\
             Example: {\"planId\": \"plan_1a2b3c4d5e6f\", \"fingerprint\": \"9f86d081…\"}",
            ToolClass::Destructive,
            Hints {
                read_only: false,
                destructive: true,
                idempotent: false,
                open_world: true,
            },
            Duration::from_secs(300),
        ),
        spec::<VerifyArgs, VerifyResult>(
            "verify_route",
            "Check a route",
            "Check a hostname end to end through Cloudflare's edge, stage by stage: its DNS record points at this machine's tunnel, the edge answers, the tunnel reaches this machine's connector, and the connector reaches the local service. Says where it breaks (e.g. `noRecord`, `noConnector` (error 1033), `originUnreachable` (502)) and why.\n\
             \n\
             Use it after a change, or when the person says a URL doesn't work. Transient failures (a connector still connecting) are retried for up to `patienceSeconds`.\n\
             \n\
             Example: {\"hostname\": \"app.example.com\", \"patienceSeconds\": 20}",
            ToolClass::Read,
            Hints::READ_CLOUD,
            Duration::from_secs(90),
        ),
        spec::<UndoArgs, UndoResult>(
            "undo_last",
            "Plan an undo",
            "Plan the reverse of the most recent change (by default the most recent one made by an agent), from Teitunnel's Activity. Nothing changes: it returns a plan to review and apply with apply_plan, like plan_change.\n\
             \n\
             Reversible: adding or removing a route (a removed route comes back with its service, not its origin settings or login), adding or removing a private network, load balancing, creating a tunnel. Other changes are explained instead.\n\
             \n\
             Example: {} · {\"entryId\": 42}",
            ToolClass::Read,
            Hints::READ_CLOUD,
            super::DEFAULT_TIMEOUT,
        ),
    ]
}

/// Account and paging.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct AccountArgs {
    /// Account name or id; needed only when several accounts are connected.
    #[serde(default)]
    account: Option<String>,
}

/// Which routes.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ListRoutesArgs {
    /// Account name or id; needed only when several accounts are connected.
    #[serde(default)]
    account: Option<String>,
    /// Only routes on this tunnel of this machine (name or id).
    #[serde(default)]
    tunnel: Option<String>,
    /// `live` for working routes only, `down` for the others.
    #[serde(default)]
    status: Option<String>,
    /// From a previous answer's `nextCursor`.
    #[serde(default)]
    cursor: Option<String>,
    /// At most this many (default 50, up to 200).
    #[serde(default)]
    limit: Option<usize>,
}

/// A route.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RouteOut {
    /// Public hostname.
    hostname: String,
    /// Path regex, if the route only takes some paths.
    path: Option<String>,
    /// The URL to open (HTTP routes).
    url: Option<String>,
    /// Where traffic goes, e.g. `http://localhost:3000`.
    origin: String,
    /// The origin is on this machine.
    local: bool,
    /// `live`, `noDns`, `dnsElsewhere`, `connecting`, `restarting`, `connectionLost`,
    /// `keepsStopping` or `stopped`.
    status: String,
    /// The status in words.
    status_text: String,
    /// The tunnel carrying it (this machine's).
    tunnel: Option<String>,
    /// Who may open it, when it's behind a login (emails and @domains).
    login: Option<Vec<String>>,
    /// A share on your domain (removed when the share stops).
    temporary: bool,
    /// Load balanced across machines.
    balanced: bool,
    /// What visitors run to reach an SSH, RDP, SMB or TCP route.
    client_command: Option<String>,
    /// Origin settings (cloudflared `originRequest`), only those set.
    options: serde_json::Value,
}

/// A tunnel of this machine.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct LocalTunnelOut {
    /// Tunnel id.
    id: String,
    /// Name.
    name: String,
    /// Where routes go unless a plan names another tunnel.
    is_default: bool,
    /// Connector on this machine: `healthy (4 connections)`, `stopped`…
    connector: String,
}

/// Routes.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RoutesResult {
    /// The account.
    account: AccountRef,
    /// This machine's tunnels in it.
    tunnels: Vec<LocalTunnelOut>,
    /// Routes, by domain then hostname.
    routes: Vec<RouteOut>,
    /// How many match in all.
    total: usize,
    /// Pass as `cursor` for the next page.
    next_cursor: Option<String>,
}

/// A connector state in a few words.
pub(crate) fn connector_text(state: Option<&teitunnel_core::runtime::ConnectorState>) -> String {
    use teitunnel_core::runtime::ConnectorState as S;
    match state {
        None | Some(S::Stopped) => "stopped (not running on this machine)".into(),
        Some(S::Starting) => "starting".into(),
        Some(S::Connecting) => "connecting".into(),
        Some(S::Healthy { connections }) => format!("healthy ({connections} connections)"),
        Some(S::Degraded) => "degraded (lost its connections)".into(),
        Some(S::Crashed { attempt, .. }) => format!("crashed, restarting (attempt {attempt})"),
        Some(S::CrashLoop { .. }) => "keeps crashing (not restarted; see doctor)".into(),
        Some(other) => format!("{other:?}").to_ascii_lowercase(),
    }
}

pub(super) async fn list_routes(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: ListRoutesArgs = arguments(args)?;
    let account = account(backend, args.account.as_deref()).await?;
    let tunnel = tunnel_id(backend, &account.id, args.tunnel.as_deref()).await?;
    let overview = backend.overview(&account.id).await?;
    let statuses = overview.statuses();
    let name_of = |id: Option<&str>| {
        overview
            .tunnels
            .iter()
            .find(|t| Some(t.id.as_str()) == id)
            .map(|t| t.name.clone())
    };
    let wanted = args.status.as_deref().map(str::to_ascii_lowercase);
    let routes: Vec<RouteOut> = overview
        .routes
        .iter()
        .zip(&statuses)
        .filter(|(route, _)| tunnel.is_none() || route.tunnel_id == tunnel)
        .filter(|(_, (_, health))| match wanted.as_deref() {
            Some("live") => health.is_live(),
            Some("down") => !health.is_live(),
            _ => true,
        })
        .map(|(route, (_, health))| RouteOut {
            url: (route.client.is_none()).then(|| format!("https://{}", route.hostname)),
            hostname: route.hostname.clone(),
            path: route.path.clone(),
            origin: route.origin.clone(),
            local: route.local,
            status: serde_json::to_value(health)
                .ok()
                .and_then(|v| v.as_str().map(ToOwned::to_owned))
                .unwrap_or_default(),
            status_text: health.text().english(),
            tunnel: name_of(route.tunnel_id.as_deref()),
            login: route.access.as_ref().map(|rule| {
                rule.emails
                    .iter()
                    .cloned()
                    .chain(rule.email_domains.iter().map(|d| format!("@{d}")))
                    .collect()
            }),
            temporary: route.temporary,
            balanced: route.balanced,
            client_command: route.client.as_ref().map(|c| c.command.clone()),
            options: serde_json::to_value(&route.options).unwrap_or_default(),
        })
        .collect();
    let (routes, next_cursor, total) =
        limits::page(routes, args.cursor.as_deref(), args.limit).map_err(ToolError::new)?;
    let live = routes.iter().filter(|r| r.status == "live").count();
    Ok(ToolOutput::new(&RoutesResult {
        account: (&account).into(),
        tunnels: overview
            .tunnels
            .iter()
            .map(|t| LocalTunnelOut {
                id: t.id.clone(),
                name: t.name.clone(),
                is_default: t.is_default,
                connector: connector_text(t.connector.as_ref()),
            })
            .collect(),
        routes,
        total,
        next_cursor,
    })
    .with_summary(format!(
        "{total} route(s) in {}; {live} live on this page.",
        account.name
    )))
}

/// A domain.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DomainOut {
    /// Zone id.
    id: String,
    /// Domain name.
    name: String,
    /// `active`, `pending` (nameservers not switched yet), …
    status: String,
    /// Cloudflare plan.
    plan: Option<String>,
    /// Cloudflare's proxy is paused.
    paused: bool,
}

/// Domains.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DomainsResult {
    /// The account.
    account: AccountRef,
    /// Its domains.
    domains: Vec<DomainOut>,
}

pub(super) async fn list_domains(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: AccountArgs = arguments(args)?;
    let account = account(backend, args.account.as_deref()).await?;
    let raw = backend.domains(&account.id).await?;
    let domains = raw
        .as_array()
        .into_iter()
        .flatten()
        .map(|d| DomainOut {
            id: d["id"].as_str().unwrap_or_default().to_owned(),
            name: d["name"].as_str().unwrap_or_default().to_owned(),
            status: match &d["status"] {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Object(o) => o
                    .get("state")
                    .or_else(|| o.get("status"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_owned(),
                _ => "unknown".into(),
            },
            plan: d["plan"].as_str().map(ToOwned::to_owned),
            paused: d["paused"].as_bool().unwrap_or(false),
        })
        .collect();
    Ok(ToolOutput::new(&DomainsResult {
        account: (&account).into(),
        domains,
    }))
}

/// A machine running a tunnel.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConnectorOut {
    /// Connector id (for remote_logs).
    pub(crate) id: String,
    /// cloudflared version.
    pub(crate) version: String,
    /// It's this machine.
    pub(crate) this_machine: bool,
    /// Edge locations it's connected to, e.g. `ams01`.
    pub(crate) locations: Vec<String>,
}

/// A tunnel.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TunnelOut {
    /// Tunnel id.
    pub(crate) id: String,
    /// Name.
    pub(crate) name: String,
    /// Cloudflare's status: `healthy`, `degraded`, `down` or `inactive`.
    pub(crate) status: String,
    /// Routes in its remote configuration (absent when configured in a local file).
    pub(crate) routes: Option<u32>,
    /// One of this machine's tunnels.
    pub(crate) this_machine: bool,
    /// This machine's default tunnel.
    pub(crate) is_default: bool,
    /// This machine's connector for it.
    pub(crate) connector: Option<String>,
    /// Machines running it.
    pub(crate) connectors: Vec<ConnectorOut>,
}

/// Tunnels.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TunnelsResult {
    /// The account.
    account: AccountRef,
    /// This machine's first.
    tunnels: Vec<TunnelOut>,
}

pub(crate) fn tunnel_out(t: &teitunnel_core::engine::TunnelSummary) -> TunnelOut {
    TunnelOut {
        id: t.id.clone(),
        name: t.name.clone(),
        status: t.status.clone(),
        routes: t.routes,
        this_machine: t.this_mac,
        is_default: t.is_default,
        connector: t.this_mac.then(|| connector_text(t.connector.as_ref())),
        connectors: t
            .connectors
            .iter()
            .map(|c| ConnectorOut {
                id: c.id.clone(),
                version: c.version.clone(),
                this_machine: c.this_mac,
                locations: c.connections.iter().map(|x| x.colo.clone()).collect(),
            })
            .collect(),
    }
}

pub(super) async fn list_tunnels(backend: &SharedBackend, args: JsonObject) -> ToolResult {
    let args: AccountArgs = arguments(args)?;
    let account = account(backend, args.account.as_deref()).await?;
    let tunnels = backend.tunnels(&account.id).await?;
    Ok(ToolOutput::new(&TunnelsResult {
        account: (&account).into(),
        tunnels: tunnels.iter().map(tunnel_out).collect(),
    }))
}

/// Origin settings for a route (cloudflared `originRequest`). Only what's given changes.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct OptionsInput {
    /// Host header sent to the service (dev servers that check it, e.g. Vite's
    /// "Blocked request" answer: send `localhost`).
    #[serde(default)]
    http_host_header: Option<String>,
    /// Hostname expected on the service's TLS certificate.
    #[serde(default)]
    origin_server_name: Option<String>,
    /// Accept any certificate from an HTTPS service (self-signed).
    #[serde(default)]
    no_tls_verify: Option<bool>,
    /// Speak HTTP/2 to an HTTPS service.
    #[serde(default)]
    http2_origin: Option<bool>,
    /// Don't use chunked transfer encoding (some WSGI servers need this).
    #[serde(default)]
    disable_chunked_encoding: Option<bool>,
    /// Seconds to wait for a connection to the service.
    #[serde(default)]
    connect_timeout: Option<u32>,
    /// Don't fall back between IPv4 and IPv6.
    #[serde(default)]
    no_happy_eyeballs: Option<bool>,
}

impl OptionsInput {
    /// These settings on top of `base`.
    fn apply(self, mut base: OriginOptions) -> OriginOptions {
        if let Some(v) = self.http_host_header {
            base.http_host_header = Some(v).filter(|v| !v.is_empty());
        }
        if let Some(v) = self.origin_server_name {
            base.origin_server_name = Some(v).filter(|v| !v.is_empty());
        }
        if let Some(v) = self.no_tls_verify {
            base.no_tls_verify = v;
        }
        if let Some(v) = self.http2_origin {
            base.http2_origin = v;
        }
        if let Some(v) = self.disable_chunked_encoding {
            base.disable_chunked_encoding = v;
        }
        if let Some(v) = self.connect_timeout {
            base.connect_timeout = Some(v);
        }
        if let Some(v) = self.no_happy_eyeballs {
            base.no_happy_eyeballs = v;
        }
        base
    }
}

/// One route to import.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ImportRoute {
    /// Public hostname.
    hostname: String,
    /// Local service.
    origin: String,
    /// Path regex.
    #[serde(default)]
    path: Option<String>,
}

/// A change to plan. `type` says which.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum ChangeInput {
    /// Route a hostname to a local service.
    AddRoute {
        /// Public hostname on one of the account's domains, e.g. `app.example.com`
        /// (or `*.example.com`).
        hostname: String,
        /// The service: a port (`3000`), `host:port`, or a URL (`http://…`, `https://…`,
        /// `ssh://localhost:22`, `rdp://…`, `tcp://…`, `unix:/path`).
        origin: String,
        /// Only requests whose path matches this regex, e.g. `^/api`.
        #[serde(default)]
        path: Option<String>,
        /// Require a login: emails (`me@example.com`) or domains (`@example.com`).
        #[serde(default)]
        allow: Vec<String>,
        /// Origin settings.
        #[serde(default)]
        options: Option<OptionsInput>,
    },
    /// Change a route. Omitted fields stay as they are.
    UpdateRoute {
        /// The route's hostname now.
        hostname: String,
        /// The route's path rule now, if it has one.
        #[serde(default)]
        path: Option<String>,
        /// Another service.
        #[serde(default)]
        origin: Option<String>,
        /// Rename it.
        #[serde(default)]
        new_hostname: Option<String>,
        /// Another path rule (empty string: none).
        #[serde(default)]
        new_path: Option<String>,
        /// Replace who may log in (use removeLogin to make it public).
        #[serde(default)]
        allow: Option<Vec<String>>,
        /// Origin settings to change.
        #[serde(default)]
        options: Option<OptionsInput>,
    },
    /// Remove a route (and the DNS record Teitunnel created for it).
    RemoveRoute {
        /// Hostname.
        hostname: String,
        /// Path rule, if the route has one.
        #[serde(default)]
        path: Option<String>,
    },
    /// Put a route behind a login.
    RequireLogin {
        /// Hostname.
        hostname: String,
        /// Path rule, if the route has one.
        #[serde(default)]
        path: Option<String>,
        /// Who may log in: emails or @domains.
        allow: Vec<String>,
    },
    /// Remove the login Teitunnel added to a route (it becomes public).
    RemoveLogin {
        /// Hostname.
        hostname: String,
        /// Path rule, if the route has one.
        #[serde(default)]
        path: Option<String>,
    },
    /// Load balance a hostname across every machine routing it.
    BalanceRoute {
        /// Hostname.
        hostname: String,
    },
    /// Stop load balancing a hostname.
    UnbalanceRoute {
        /// Hostname.
        hostname: String,
    },
    /// Let WARP clients reach a private range through this machine.
    AddNetwork {
        /// An IP address or CIDR range, e.g. `192.168.1.0/24`.
        network: String,
    },
    /// Stop routing a range through this machine.
    RemoveNetwork {
        /// The range.
        network: String,
    },
    /// Create another tunnel for this machine.
    CreateTunnel {
        /// Its name, unique in the account.
        name: String,
    },
    /// Delete one of this machine's tunnels with its routes and the DNS records
    /// Teitunnel created for them.
    DeleteTunnel {
        /// The tunnel (name or id).
        tunnel: String,
    },
    /// Add several routes at once.
    ImportRoutes {
        /// The routes.
        routes: Vec<ImportRoute>,
    },
    /// Undo an outside edit of this machine's routes.
    RestoreConfig {},
    /// Delete one DNS record (an orphan the doctor found).
    DeleteDnsRecord {
        /// Zone id.
        zone_id: String,
        /// The record's name.
        hostname: String,
        /// Record id.
        record_id: String,
    },
}

/// What to plan.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PlanArgs {
    /// The change.
    change: ChangeInput,
    /// Account name or id; needed only when several accounts are connected (for
    /// hostnames, the account owning the domain is found by itself).
    #[serde(default)]
    account: Option<String>,
    /// One of this machine's tunnels (name or id). Default: the tunnel carrying the
    /// route, or the default tunnel for a new one.
    #[serde(default)]
    tunnel: Option<String>,
}

/// A step of a plan.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StepOut {
    /// What kind: `createTunnel`, `putConfig`, `createRecord`, `updateRecord`,
    /// `deleteRecord`, `accessApp`, `networkRoute`, `loadBalancer`, `verify`, …
    kind: String,
    /// What it does.
    description: String,
    /// The equivalent command, when there is one.
    command: Option<String>,
}

/// A plan.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanOut {
    /// Pass to apply_plan.
    pub(crate) plan_id: String,
    /// Pass to apply_plan (it proves the plan is the one reviewed).
    fingerprint: String,
    /// What it does, in a line.
    summary: String,
    /// Nothing needs to change.
    empty: bool,
    /// Steps, in order.
    steps: Vec<StepOut>,
    /// Things the person should know before approving.
    warnings: Vec<String>,
    /// Replaces or deletes DNS records Teitunnel didn't create: apply_plan needs
    /// `confirmed: true` or the person's approval.
    pub(crate) requires_confirmation: bool,
    /// How applying will be approved.
    approval: String,
    /// The plan as the person should read it.
    text: String,
}

/// A planned change.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PlanResult {
    /// The plan.
    plan: PlanOut,
    /// The account.
    account: AccountRef,
    /// What to do next.
    next: String,
}

pub(crate) fn plan_out(plan: &PendingPlan, ctx: &ToolContext) -> PlanOut {
    let view = &plan.view;
    let approval = match ctx.mode() {
        Mode::Full if view.requires_confirmation => {
            "full mode, but this plan touches records Teitunnel didn't create: apply_plan needs \"confirmed\": true after the person agrees".to_owned()
        }
        Mode::Full => "not needed (full mode)".to_owned(),
        Mode::ReadOnly => "impossible: this server is read-only".to_owned(),
        Mode::Ask if ctx.can_ask() => {
            "the person is asked to approve when you call apply_plan".to_owned()
        }
        Mode::Ask => "show the person the plan; call apply_plan with \"confirmed\": true only after they agree".to_owned(),
    };
    PlanOut {
        plan_id: plan.id.clone(),
        fingerprint: view.fingerprint.clone(),
        summary: plan.summary.clone(),
        empty: view.steps.is_empty(),
        steps: view
            .steps
            .iter()
            .map(|s| StepOut {
                kind: serde_json::to_value(s.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(ToOwned::to_owned))
                    .unwrap_or_default(),
                description: s.description.english(),
                command: s.command.clone(),
            })
            .collect(),
        warnings: view.warnings.iter().map(super::warning_text).collect(),
        requires_confirmation: view.requires_confirmation,
        approval,
        text: plan_text(view),
    }
}

/// The route with `hostname` and `path` in `account`, for edits.
async fn current_route(
    backend: &SharedBackend,
    account: &str,
    hostname: &str,
    path: Option<&str>,
) -> Result<teitunnel_core::engine::RouteView, ToolError> {
    let overview = backend.overview(account).await?;
    let host = hostname.trim().to_ascii_lowercase();
    let path = path.map(str::trim).filter(|p| !p.is_empty());
    overview
        .routes
        .into_iter()
        .find(|r| r.hostname == host && r.path.as_deref() == path)
        .ok_or_else(|| {
            ToolError::new(format!(
                "This machine has no route {host}{} in this account. Call list_routes to see them.",
                path.map(|p| format!(" {p}")).unwrap_or_default()
            ))
        })
}

fn hostname_of(change: &ChangeInput) -> Option<&str> {
    match change {
        ChangeInput::AddRoute { hostname, .. }
        | ChangeInput::UpdateRoute { hostname, .. }
        | ChangeInput::RemoveRoute { hostname, .. }
        | ChangeInput::RequireLogin { hostname, .. }
        | ChangeInput::RemoveLogin { hostname, .. }
        | ChangeInput::BalanceRoute { hostname }
        | ChangeInput::UnbalanceRoute { hostname }
        | ChangeInput::DeleteDnsRecord { hostname, .. } => Some(hostname),
        ChangeInput::ImportRoutes { routes } => routes.first().map(|r| r.hostname.as_str()),
        _ => None,
    }
}

/// Turns the agent's change into the engine's, with a one-line summary. Edits read the
/// route first so what isn't mentioned stays as it is.
async fn to_change(
    backend: &SharedBackend,
    account: &str,
    input: ChangeInput,
) -> Result<(Change, Option<String>, String), ToolError> {
    let people = |allow: &[String]| super::access_rule(allow).map(|r| r.people());
    Ok(match input {
        ChangeInput::AddRoute {
            hostname,
            origin,
            path,
            allow,
            options,
        } => {
            let summary = format!(
                "Add {hostname}{} → {origin}{}",
                path.as_deref().map(|p| format!(" {p}")).unwrap_or_default(),
                people(&allow)
                    .map(|p| format!(" (login: {p})"))
                    .unwrap_or_default()
            );
            let options = options
                .map(|o| o.apply(OriginOptions::default()))
                .filter(|o| !o.is_default())
                .map(Box::new);
            (
                Change::AddRoute {
                    route: RouteInput {
                        hostname,
                        path,
                        origin,
                        access: super::access_rule(&allow),
                        options,
                    },
                },
                None,
                summary,
            )
        }
        ChangeInput::UpdateRoute {
            hostname,
            path,
            origin,
            new_hostname,
            new_path,
            allow,
            options,
        } => {
            let current = current_route(backend, account, &hostname, path.as_deref()).await?;
            let access = match &allow {
                Some(allow) if allow.is_empty() => {
                    return Err(ToolError::new(
                        "`allow` can't be empty; use removeLogin to make the route public.",
                    ));
                }
                Some(allow) => super::access_rule(allow),
                None => current.access.clone(),
            };
            let new_path = match new_path {
                Some(p) if p.trim().is_empty() => None,
                Some(p) => Some(p),
                None => current.path.clone(),
            };
            let summary = format!(
                "Change {hostname}{}",
                path.as_deref().map(|p| format!(" {p}")).unwrap_or_default()
            );
            (
                Change::UpdateRoute {
                    hostname: current.hostname.clone(),
                    path: current.path.clone(),
                    route: RouteInput {
                        hostname: new_hostname.unwrap_or_else(|| current.hostname.clone()),
                        path: new_path,
                        origin: origin.unwrap_or_else(|| current.origin.clone()),
                        access,
                        options: options.map(|o| Box::new(o.apply(current.options.clone()))),
                    },
                },
                current.tunnel_id.clone(),
                summary,
            )
        }
        ChangeInput::RemoveRoute { hostname, path } => (
            Change::RemoveRoute {
                hostname: hostname.clone(),
                path: path.clone(),
            },
            None,
            format!(
                "Remove {hostname}{}",
                path.map(|p| format!(" {p}")).unwrap_or_default()
            ),
        ),
        ChangeInput::RequireLogin {
            hostname,
            path,
            allow,
        } => {
            if allow.is_empty() {
                return Err(ToolError::new(
                    "`allow` needs at least one email or @domain.",
                ));
            }
            let current = current_route(backend, account, &hostname, path.as_deref()).await?;
            let summary = format!(
                "Require a login for {hostname} ({})",
                people(&allow).unwrap_or_default()
            );
            (
                Change::UpdateRoute {
                    hostname: current.hostname.clone(),
                    path: current.path.clone(),
                    route: RouteInput {
                        hostname: current.hostname.clone(),
                        path: current.path.clone(),
                        origin: current.origin.clone(),
                        access: super::access_rule(&allow),
                        options: None,
                    },
                },
                current.tunnel_id.clone(),
                summary,
            )
        }
        ChangeInput::RemoveLogin { hostname, path } => {
            let current = current_route(backend, account, &hostname, path.as_deref()).await?;
            (
                Change::UpdateRoute {
                    hostname: current.hostname.clone(),
                    path: current.path.clone(),
                    route: RouteInput {
                        hostname: current.hostname.clone(),
                        path: current.path.clone(),
                        origin: current.origin.clone(),
                        access: None,
                        options: None,
                    },
                },
                current.tunnel_id.clone(),
                format!("Remove the login from {hostname} (it becomes public)"),
            )
        }
        ChangeInput::BalanceRoute { hostname } => (
            Change::BalanceRoute {
                hostname: hostname.clone(),
            },
            None,
            format!("Load balance {hostname}"),
        ),
        ChangeInput::UnbalanceRoute { hostname } => (
            Change::UnbalanceRoute {
                hostname: hostname.clone(),
            },
            None,
            format!("Stop load balancing {hostname}"),
        ),
        ChangeInput::AddNetwork { network } => (
            Change::AddNetwork {
                network: network.clone(),
            },
            None,
            format!("Route {network} to this machine for WARP clients"),
        ),
        ChangeInput::RemoveNetwork { network } => (
            Change::RemoveNetwork {
                network: network.clone(),
            },
            None,
            format!("Stop routing {network} through this machine"),
        ),
        ChangeInput::CreateTunnel { name } => (
            Change::CreateTunnel { name: name.clone() },
            None,
            format!("Create a tunnel called {name}"),
        ),
        ChangeInput::DeleteTunnel { tunnel } => {
            let id = tunnel_id(backend, account, Some(&tunnel)).await?;
            (
                Change::RemoveTunnel,
                id,
                format!("Delete the tunnel {tunnel} with its routes"),
            )
        }
        ChangeInput::ImportRoutes { routes } => {
            let summary = format!("Add {} routes", routes.len());
            (
                Change::ImportRoutes {
                    routes: routes
                        .into_iter()
                        .map(|r| RouteInput {
                            hostname: r.hostname,
                            path: r.path,
                            origin: r.origin,
                            access: None,
                            options: None,
                        })
                        .collect(),
                },
                None,
                summary,
            )
        }
        ChangeInput::RestoreConfig {} => (
            Change::RestoreConfig,
            None,
            "Restore this machine's routes (undo an outside edit)".into(),
        ),
        ChangeInput::DeleteDnsRecord {
            zone_id,
            hostname,
            record_id,
        } => (
            Change::DeleteRecord {
                zone_id,
                hostname: hostname.clone(),
                record_id,
            },
            None,
            format!("Delete the DNS record of {hostname}"),
        ),
    })
}

/// Plans `change` in `target`, stores it, and answers with the plan.
pub(crate) async fn plan_and_store(
    backend: &SharedBackend,
    plans: &Plans,
    target: Target,
    change: Change,
    summary: String,
    ctx: &ToolContext,
) -> Result<PlanOut, ToolError> {
    let view = backend.preview(&target, &change).await?;
    let plan = plans.insert(target, change, view, summary);
    Ok(plan_out(&plan, ctx))
}

fn next_step(plan: &PlanOut, ctx: &ToolContext) -> String {
    if plan.empty {
        return "Nothing to change: it's already like that.".into();
    }
    match ctx.mode() {
        Mode::ReadOnly => "This server is read-only, so it can't be applied here. Show the person the plan; they can apply it in the Teitunnel app.".into(),
        _ => format!(
            "Show the person the steps{}, then call apply_plan with planId \"{}\" and this fingerprint.",
            if plan.warnings.is_empty() { "" } else { " and warnings" },
            plan.plan_id
        ),
    }
}

pub(super) async fn plan_change(
    backend: &SharedBackend,
    plans: &Plans,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: PlanArgs = arguments(args)?;
    let account = match hostname_of(&args.change) {
        Some(host) => account_for_hostname(backend, args.account.as_deref(), host).await?,
        None => account(backend, args.account.as_deref()).await?,
    };
    let named = tunnel_id(backend, &account.id, args.tunnel.as_deref()).await?;
    let (change, carrying, summary) = to_change(backend, &account.id, args.change).await?;
    let target = Target {
        account: account.id.clone(),
        tunnel: named.or(carrying),
    };
    let plan = plan_and_store(backend, plans, target, change, summary, ctx).await?;
    let next = next_step(&plan, ctx);
    let steps = plan.steps.len();
    Ok(ToolOutput::new(&PlanResult {
        next,
        account: (&account).into(),
        plan,
    })
    .with_summary(format!(
        "A plan of {steps} step(s); nothing has changed yet."
    )))
}

/// Which plan to apply.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ApplyArgs {
    /// From plan_change (or undo_last, fix_issue).
    pub(crate) plan_id: String,
    /// The plan's fingerprint, as returned with it.
    fingerprint: String,
    /// The person reviewed this plan and agreed (needed when this server can't ask them
    /// itself, and for plans with `requiresConfirmation`).
    #[serde(default)]
    confirmed: bool,
    /// Check the affected routes end to end afterwards (default true).
    #[serde(default = "yes")]
    verify: bool,
}

fn yes() -> bool {
    true
}

/// A step's final state.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StepStateOut {
    /// What it did.
    description: String,
    /// `done`, `failed: …`, `undone`, `couldn't undo: …` or `skipped`.
    state: String,
}

/// A route check.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VerifyResult {
    /// Hostname.
    hostname: String,
    /// It works end to end.
    ok: bool,
    /// The service's HTTP status, when it answered.
    status: Option<u16>,
    /// Where it breaks, e.g. `noRecord`, `recordElsewhere`, `edgeUnreachable`,
    /// `notOnCloudflare`, `noConnector`, `dnsMismatch`, `originUnreachable`, `tls`.
    failure: Option<String>,
    /// Why, and what to do.
    message: Option<String>,
    /// Behind a login: the check reached Cloudflare's login page, not the service.
    protected: bool,
}

impl From<teitunnel_core::engine::Verification> for VerifyResult {
    fn from(v: teitunnel_core::engine::Verification) -> Self {
        Self {
            ok: v.ok(),
            failure: v.failure.as_ref().and_then(|f| {
                serde_json::to_value(f).ok().and_then(|v| {
                    v.get("type")
                        .and_then(|t| t.as_str())
                        .map(ToOwned::to_owned)
                })
            }),
            message: v.message.as_ref().map(teitunnel_core::text::Text::english),
            hostname: v.hostname,
            status: v.status,
            protected: v.protected,
        }
    }
}

/// The result of apply_plan.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ApplyResult {
    /// `applied`, `rolledBack` (failed, everything undone), `partiallyApplied` (failed,
    /// some left over), `stale` (review the new plan), `needsApproval`, `declined` or
    /// `nothingToDo`.
    outcome: String,
    /// What happened.
    pub(crate) message: String,
    /// Each step's final state.
    steps: Vec<StepStateOut>,
    /// Checks of the affected routes.
    verification: Vec<VerifyResult>,
    /// What couldn't be undone after a failure.
    leftovers: Vec<String>,
    /// With `stale`: the new plan to review.
    new_plan: Option<PlanOut>,
    /// How to reverse it.
    undo: Option<String>,
}

impl ApplyResult {
    fn only(outcome: &str, message: String) -> Self {
        Self {
            outcome: outcome.into(),
            message,
            steps: Vec::new(),
            verification: Vec::new(),
            leftovers: Vec::new(),
            new_plan: None,
            undo: None,
        }
    }
}

fn state_text(state: &StepState) -> String {
    match state {
        StepState::Running => "running".into(),
        StepState::Done => "done".into(),
        StepState::Skipped => "skipped".into(),
        StepState::Failed { message } => format!("failed: {}", message.english()),
        StepState::Undoing => "undoing".into(),
        StepState::Undone => "undone".into(),
        StepState::UndoFailed { message } => format!("couldn't undo: {}", message.english()),
    }
}

/// Applies a stored plan with the person's approval (shared by apply_plan and
/// fix_issue).
pub(crate) async fn apply_stored(
    backend: &SharedBackend,
    plans: &Plans,
    plan: PendingPlan,
    confirmed: bool,
    verify: bool,
    ctx: &ToolContext,
) -> Result<ApplyResult, ToolError> {
    if plan.view.steps.is_empty() {
        plans.remove(&plan.id);
        return Ok(ApplyResult::only(
            "nothingToDo",
            "Nothing to change: it's already like that.".into(),
        ));
    }
    let approval = ctx
        .approve(&ApprovalRequest {
            title: plan.summary.clone(),
            details: plan_text(&plan.view),
            confirmed,
        })
        .await;
    let by_person = match approval {
        Approval::Granted { how } => how == "person",
        Approval::NeedsConfirmation => {
            return Ok(ApplyResult::only(
                "needsApproval",
                format!(
                    "Nothing was applied. This server can't ask the person directly: show them the plan below, and call apply_plan again with \"confirmed\": true only if they agree.\n{}",
                    plan_text(&plan.view)
                ),
            ));
        }
        Approval::Declined(why) => {
            plans.remove(&plan.id);
            return Ok(ApplyResult::only(
                "declined",
                format!("{why} Nothing was applied."),
            ));
        }
    };
    let foreign_ok = by_person || confirmed;
    if plan.view.requires_confirmation && !foreign_ok {
        return Ok(ApplyResult::only(
            "needsApproval",
            format!(
                "Nothing was applied. This plan replaces or deletes DNS records Teitunnel didn't create; show the person the warnings and call again with \"confirmed\": true only if they agree.\n{}",
                plan_text(&plan.view)
            ),
        ));
    }

    let descriptions: Vec<String> = plan
        .view
        .steps
        .iter()
        .map(|s| s.description.english())
        .collect();
    let total = descriptions.len() as f64;
    let sender = ctx.progress_sender();
    let progress_names = descriptions.clone();
    let progress: crate::backend::ProgressSink = Box::new(move |p| {
        let Some(tx) = &sender else { return };
        let index = usize::try_from(p.step).unwrap_or(usize::MAX);
        let name = progress_names.get(index).cloned().unwrap_or_default();
        let _ = tx.send((
            f64::from(p.step) + 1.0,
            Some(total),
            format!("{}: {name}", state_text(&p.state)),
        ));
    });
    let outcome = backend
        .apply(
            &plan.target,
            &plan.change,
            ApplyApproval {
                fingerprint: plan.view.fingerprint.clone(),
                confirmed: plan.view.requires_confirmation && foreign_ok,
            },
            Some(ctx.actor().clone()),
            progress,
        )
        .await;
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(BackendError::Stale(view)) => {
            plans.replace_view(&plan.id, (*view).clone());
            let fresh = plans.get(&plan.id).ok_or_else(|| {
                ToolError::new("The plan expired; make it again with plan_change.")
            })?;
            let mut result = ApplyResult::only(
                "stale",
                "Nothing was applied: something changed in Cloudflare since this plan was made. Review the new plan with the person and apply it (same planId, new fingerprint).".into(),
            );
            result.new_plan = Some(plan_out(&fresh, ctx));
            return Ok(result);
        }
        Err(err) => return Err(err.into()),
    };
    plans.remove(&plan.id);
    let steps = |failed: Option<u32>| -> Vec<StepStateOut> {
        descriptions
            .iter()
            .enumerate()
            .map(|(i, d)| StepStateOut {
                description: d.clone(),
                state: match failed {
                    None => "done".into(),
                    Some(f) if i == usize::try_from(f).unwrap_or(usize::MAX) => "failed".into(),
                    Some(f) if i < usize::try_from(f).unwrap_or(usize::MAX) => {
                        "undone or left over".into()
                    }
                    Some(_) => "skipped".into(),
                },
            })
            .collect()
    };
    Ok(match outcome {
        Outcome::Applied {
            verify: hostnames,
            connector_error,
            ..
        } => {
            let mut verification = Vec::new();
            if verify {
                for (i, hostname) in hostnames.iter().enumerate() {
                    ctx.progress(
                        total + i as f64,
                        Some(total + hostnames.len() as f64),
                        format!("Checking https://{hostname}…"),
                    )
                    .await;
                    if let Ok(v) = backend
                        .verify(&plan.target.account, hostname, VERIFY_AFTER_APPLY)
                        .await
                    {
                        verification.push(VerifyResult::from(v));
                    }
                }
            }
            let failing = verification.iter().filter(|v| !v.ok).count();
            let mut message = format!("Applied: {}.", plan.summary);
            if let Some(error) = connector_error {
                message.push_str(&format!(
                    " Note: {} The change is in Cloudflare, but this machine's connector isn't running, so the routes don't answer until it does (the Teitunnel app, Always-on, or `teitunnel up`).",
                    error.english()
                ));
            }
            if failing > 0 {
                message.push_str(&format!(
                    " {failing} route(s) don't work yet; see `verification` (DNS can take a minute; verify_route checks again)."
                ));
            }
            ApplyResult {
                outcome: "applied".into(),
                message,
                steps: steps(None),
                verification,
                leftovers: Vec::new(),
                new_plan: None,
                undo: Some(
                    "Reverse it with undo_last (it plans the inverse change for review).".into(),
                ),
            }
        }
        Outcome::RolledBack { failed_step, error } => ApplyResult {
            outcome: "rolledBack".into(),
            message: format!(
                "Failed: {}. Everything done before was undone; nothing changed.",
                error.english()
            ),
            steps: steps(Some(failed_step)),
            verification: Vec::new(),
            leftovers: Vec::new(),
            new_plan: None,
            undo: None,
        },
        Outcome::PartiallyApplied {
            failed_step,
            error,
            leftovers,
        } => ApplyResult {
            outcome: "partiallyApplied".into(),
            message: format!(
                "Failed: {}. Some earlier changes couldn't be undone (see leftovers); the doctor lists them with fixes.",
                error.english()
            ),
            steps: steps(Some(failed_step)),
            verification: Vec::new(),
            leftovers: leftovers
                .iter()
                .map(teitunnel_core::text::Text::english)
                .collect(),
            new_plan: None,
            undo: None,
        },
    })
}

pub(super) async fn apply_plan(
    backend: &SharedBackend,
    plans: &Plans,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: ApplyArgs = arguments(args)?;
    let plan = plans.get(&args.plan_id).ok_or_else(|| {
        ToolError::new(format!(
            "No plan \"{}\" (plans expire after 30 minutes, and each applies once). Make it again with plan_change.",
            args.plan_id
        ))
    })?;
    if plan.view.fingerprint != args.fingerprint.trim() {
        return Err(ToolError::new(
            "That fingerprint isn't this plan's: the plan was made again since. Review the plan's current steps (plan_change or the `newPlan` you got) and pass its fingerprint.",
        ));
    }
    let result = apply_stored(backend, plans, plan, args.confirmed, args.verify, ctx).await?;
    let summary = result.message.clone();
    Ok(ToolOutput::new(&result).with_summary(summary))
}

/// What to check.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct VerifyArgs {
    /// The hostname, e.g. `app.example.com`.
    hostname: String,
    /// Account name or id; found from the hostname's domain when omitted.
    #[serde(default)]
    account: Option<String>,
    /// Keep retrying transient failures this long (0 to 60 s, default 10).
    #[serde(default)]
    #[schemars(range(max = 60))]
    patience_seconds: Option<u64>,
}

pub(super) async fn verify_route(
    backend: &SharedBackend,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: VerifyArgs = arguments(args)?;
    let account = account_for_hostname(backend, args.account.as_deref(), &args.hostname).await?;
    let patience = Duration::from_secs(args.patience_seconds.unwrap_or(10).min(60));
    ctx.progress(
        0.0,
        None,
        format!("Checking https://{}…", args.hostname.trim()),
    )
    .await;
    let result = VerifyResult::from(
        backend
            .verify(&account.id, args.hostname.trim(), patience)
            .await?,
    );
    let summary = if result.ok {
        format!("https://{} works.", result.hostname)
    } else {
        format!(
            "https://{} doesn't work: {}",
            result.hostname,
            result.message.clone().unwrap_or_default()
        )
    };
    Ok(ToolOutput::new(&result).with_summary(summary))
}

/// Which change to undo.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct UndoArgs {
    /// Account name or id; needed only when several accounts are connected.
    #[serde(default)]
    account: Option<String>,
    /// A specific Activity entry (from recent_activity). Default: the most recent one.
    #[serde(default)]
    entry_id: Option<i64>,
    /// Consider changes made by people too, not only by agents (default false).
    #[serde(default)]
    include_people: bool,
}

/// The change being undone.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UndoingOut {
    /// Activity entry id.
    entry_id: i64,
    /// What it did.
    summary: String,
    /// When (milliseconds since the epoch).
    at: i64,
    /// Who: an agent's name, or `a person`.
    by: String,
}

/// The undo plan.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UndoResult {
    /// The change being undone.
    undoing: UndoingOut,
    /// The plan that reverses it (apply with apply_plan).
    plan: PlanOut,
    /// What won't come back, if anything.
    caveat: Option<String>,
    /// What to do next.
    next: String,
}

/// The inverse of an applied change, with a caveat, or why there's none.
fn inverse(
    entry: &ActivityEntry,
    tunnel: Option<String>,
) -> Result<(Change, Option<String>, Option<String>), String> {
    let record = entry
        .record
        .as_ref()
        .ok_or("This entry is from an older Teitunnel and doesn't record enough to undo it.")?;
    let routes: Vec<_> = record
        .changes
        .iter()
        .filter(|d| d.area == DeltaArea::Route)
        .collect();
    let networks: Vec<_> = record
        .changes
        .iter()
        .filter(|d| d.area == DeltaArea::Network)
        .collect();
    let first_host = || {
        record
            .hostnames
            .first()
            .cloned()
            .ok_or("It named no hostname.")
    };
    Ok(match record.kind {
        ActivityKind::AddRoute => {
            let delta = routes.first().ok_or("It changed no route.")?;
            (
                Change::RemoveRoute {
                    hostname: delta.hostname.clone(),
                    path: delta.path.clone(),
                },
                tunnel,
                None,
            )
        }
        ActivityKind::RemoveRoute => {
            let delta = routes.first().ok_or("It changed no route.")?;
            let before = delta
                .before
                .as_ref()
                .map(teitunnel_core::text::Text::english)
                .ok_or("It doesn't record the removed route's service.")?;
            let (origin, had_options) = match before.split_once(" · ") {
                Some((origin, _)) => (origin.to_owned(), true),
                None => (before, false),
            };
            let had_login = record.changes.iter().any(|d| d.area == DeltaArea::Access);
            let mut lost = Vec::new();
            if had_options {
                lost.push("its origin settings");
            }
            if had_login {
                lost.push("its login");
            }
            (
                Change::AddRoute {
                    route: RouteInput {
                        hostname: delta.hostname.clone(),
                        path: delta.path.clone(),
                        origin,
                        access: None,
                        options: None,
                    },
                },
                tunnel,
                (!lost.is_empty()).then(|| {
                    format!(
                        "The route comes back without {} (they weren't recorded); set them again with plan_change updateRoute or requireLogin.",
                        lost.join(" and ")
                    )
                }),
            )
        }
        ActivityKind::AddNetwork => {
            let delta = networks.first().ok_or("It changed no network.")?;
            (
                Change::RemoveNetwork {
                    network: delta.hostname.clone(),
                },
                tunnel,
                None,
            )
        }
        ActivityKind::RemoveNetwork => {
            let delta = networks.first().ok_or("It changed no network.")?;
            (
                Change::AddNetwork {
                    network: delta.hostname.clone(),
                },
                tunnel,
                None,
            )
        }
        ActivityKind::BalanceRoute => (
            Change::UnbalanceRoute {
                hostname: first_host()?,
            },
            tunnel,
            None,
        ),
        ActivityKind::UnbalanceRoute => (
            Change::BalanceRoute {
                hostname: first_host()?,
            },
            tunnel,
            None,
        ),
        ActivityKind::CreateTunnel => (Change::RemoveTunnel, tunnel, None),
        ActivityKind::UpdateRoute => {
            return Err("A route edit isn't undone automatically: plan_change updateRoute with the previous values (the entry's `changes` show them).".into());
        }
        _ => {
            return Err(format!(
                "\"{}\" can't be undone automatically.",
                entry.summary
            ));
        }
    })
}

pub(super) async fn undo_last(
    backend: &SharedBackend,
    plans: &Plans,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: UndoArgs = arguments(args)?;
    let account = account(backend, args.account.as_deref()).await?;
    let entries = backend.activity(&account.id, 50).await?;
    let entry = entries
        .into_iter()
        .filter(|e| e.outcome == "applied")
        .find(|e| match args.entry_id {
            Some(id) => e.id == id,
            None => {
                args.include_people || e.record.as_ref().is_some_and(|r| r.actor.is_some())
            }
        })
        .ok_or_else(|| {
            ToolError::new(match args.entry_id {
                Some(id) => format!("No applied change with entry id {id} among the 50 most recent. See recent_activity."),
                None if args.include_people => "No applied change to undo in this account.".to_owned(),
                None => "No change made by an agent to undo. Pass includePeople: true to undo a person's change, or an entryId from recent_activity.".to_owned(),
            })
        })?;
    // A new tunnel is undone by deleting that tunnel.
    let tunnel = match entry.record.as_ref() {
        Some(record) if record.kind == ActivityKind::CreateTunnel => Some(
            tunnel_id(backend, &account.id, Some(&record.tunnel))
                .await?
                .ok_or_else(|| {
                    ToolError::new("That tunnel isn't one of this machine's any more.")
                })?,
        ),
        _ => None,
    };
    let (change, tunnel, caveat) = inverse(&entry, tunnel).map_err(ToolError::new)?;
    // Removing a route happens on the tunnel carrying it.
    let tunnel = match (tunnel, &change) {
        (Some(t), _) => Some(t),
        (None, Change::RemoveRoute { hostname, .. }) => {
            let overview = backend.overview(&account.id).await?;
            overview
                .routes
                .iter()
                .find(|r| &r.hostname == hostname)
                .and_then(|r| r.tunnel_id.clone())
        }
        (None, _) => None,
    };
    let summary = format!("Undo: {}", entry.summary);
    let plan = plan_and_store(
        backend,
        plans,
        Target {
            account: account.id.clone(),
            tunnel,
        },
        change,
        summary,
        ctx,
    )
    .await?;
    let next = next_step(&plan, ctx);
    Ok(ToolOutput::new(&UndoResult {
        undoing: UndoingOut {
            entry_id: entry.id,
            summary: entry.summary.clone(),
            at: entry.at,
            by: entry
                .record
                .as_ref()
                .and_then(|r| r.actor.as_ref())
                .map_or_else(|| "a person".to_owned(), |a| a.client.clone()),
        },
        plan,
        caveat,
        next,
    }))
}
