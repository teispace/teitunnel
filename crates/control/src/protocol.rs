//! The wire protocol: newline-delimited JSON-RPC 2.0 over the control endpoint.
//!
//! Every message is one line of UTF-8 JSON, at most [`MAX_MESSAGE`] bytes. A client's
//! first request must be [`method::HELLO`] with the protocol version it speaks and the
//! install's token; nothing else is answered before it. Requests carry an `id`; the
//! server answers each with a response carrying the same `id`. Subscriptions arrive as
//! notifications (`{"jsonrpc":"2.0","method":"event","params":{"type":…}}`).
//!
//! These types are the contract with the CLI and editor extensions: fields are only
//! ever added (clients ignore unknown fields and event types); a breaking change means a
//! new [`PROTOCOL_VERSION`].

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The protocol version this build speaks.
pub const PROTOCOL_VERSION: u32 = 1;

/// The longest message (one line, without its newline) either side accepts.
pub const MAX_MESSAGE: usize = 1024 * 1024;

/// Method names.
pub mod method {
    /// Authenticate and agree on the protocol (must come first).
    pub const HELLO: &str = "hello";
    /// The app, accounts, this machine's tunnels and every share (no network).
    pub const STATUS: &str = "status";
    /// Every share: the app's, terminals' and domain shares.
    pub const SHARES_LIST: &str = "shares.list";
    /// Share a local service (needs the person's approval).
    pub const SHARES_START: &str = "shares.start";
    /// Stop a share (needs the person's approval).
    pub const SHARES_STOP: &str = "shares.stop";
    /// Pause a share on your domain or a route: the address stays and visitors get a
    /// "paused" page (needs the person's approval).
    pub const SHARES_PAUSE: &str = "shares.pause";
    /// Serve a paused share or route again at the same address (needs the person's
    /// approval).
    pub const SHARES_RESUME: &str = "shares.resume";
    /// This machine's routes in an account, with their status.
    pub const ROUTES_LIST: &str = "routes.list";
    /// Plan a change for review; nothing changes.
    pub const ROUTES_PREVIEW: &str = "routes.preview";
    /// Apply a reviewed plan by its fingerprint (needs the person's approval).
    pub const ROUTES_APPLY: &str = "routes.apply";
    /// Bring the app's window to a view.
    pub const OPEN: &str = "open";
    /// Run the Doctor's checks.
    pub const DOCTOR_RUN: &str = "doctor.run";
    /// Receive events as notifications.
    pub const EVENTS_SUBSCRIBE: &str = "events.subscribe";
    /// Local HTTPS domains on this computer and whether the app serves them.
    pub const LOCAL_DOMAINS_LIST: &str = "localDomains.list";
    /// Serve what's in the database now (after the CLI or a project changed it).
    pub const LOCAL_DOMAINS_RELOAD: &str = "localDomains.reload";
    /// An MCP server (`teitunnel mcp`) says which AI agent it serves; the app lists it
    /// in Settings ▸ AI Tools while the connection lasts.
    pub const AGENT_REGISTER: &str = "agent.register";
    /// An MCP server asks the person, in the app, to approve an agent's change.
    pub const AGENT_APPROVE: &str = "agent.approve";

    /// Every method after `hello`.
    pub const ALL: &[&str] = &[
        STATUS,
        SHARES_LIST,
        SHARES_START,
        SHARES_STOP,
        SHARES_PAUSE,
        SHARES_RESUME,
        ROUTES_LIST,
        ROUTES_PREVIEW,
        ROUTES_APPLY,
        OPEN,
        DOCTOR_RUN,
        EVENTS_SUBSCRIBE,
        LOCAL_DOMAINS_LIST,
        LOCAL_DOMAINS_RELOAD,
        AGENT_REGISTER,
        AGENT_APPROVE,
    ];

    /// Methods that change something, so the person approves them (or the client).
    pub fn is_mutation(name: &str) -> bool {
        matches!(
            name,
            SHARES_START
                | SHARES_STOP
                | SHARES_PAUSE
                | SHARES_RESUME
                | ROUTES_APPLY
                | AGENT_APPROVE
        )
    }
}

/// The notification method events arrive with.
pub const EVENT_NOTIFICATION: &str = "event";

/// JSON-RPC and Teitunnel error codes.
pub mod code {
    /// The line isn't JSON.
    pub const PARSE_ERROR: i64 = -32700;
    /// Not a JSON-RPC 2.0 request.
    pub const INVALID_REQUEST: i64 = -32600;
    /// No such method.
    pub const METHOD_NOT_FOUND: i64 = -32601;
    /// The parameters don't fit the method.
    pub const INVALID_PARAMS: i64 = -32602;
    /// Something went wrong in the app.
    pub const INTERNAL: i64 = -32603;
    /// `hello` missing, or its token is wrong.
    pub const UNAUTHORIZED: i64 = -32001;
    /// Too many requests; slow down.
    pub const RATE_LIMITED: i64 = -32002;
    /// The person said no.
    pub const DECLINED: i64 = -32003;
    /// The plan touches DNS records Teitunnel didn't create; pass `confirmed: true`.
    pub const NEEDS_CONFIRMATION: i64 = -32004;
    /// Cloudflare changed since the preview; `data` is the new plan.
    pub const STALE: i64 = -32005;
    /// The control connection is turned off in Settings.
    pub const DISABLED: i64 = -32006;
    /// A message was longer than [`super::MAX_MESSAGE`].
    pub const TOO_LARGE: i64 = -32007;
    /// What was asked for doesn't exist.
    pub const NOT_FOUND: i64 = -32008;
    /// The request took too long.
    pub const TIMEOUT: i64 = -32009;
    /// The client speaks a protocol version this app doesn't; `data.supported` lists
    /// the ones it does.
    pub const UNSUPPORTED_PROTOCOL: i64 = -32010;
}

/// A request id (a number or a string).
pub type Id = Value;

/// A request or notification from the client.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Absent for notifications (which the server ignores).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<Id>,
    /// The method.
    pub method: String,
    /// Its parameters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// An error in a response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{message} ({code})")]
pub struct RpcError {
    /// One of [`code`].
    pub code: i64,
    /// An English sentence.
    pub message: String,
    /// Details, depending on the code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl RpcError {
    /// An error with a code and message.
    pub fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }

    /// Adds details.
    #[must_use]
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

/// A response or a notification from the server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// The request's id (`null` when it couldn't be read).
    pub id: Id,
    /// The result, on success.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// The error, on failure.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    /// A successful response.
    pub fn ok(id: Id, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// A failed response.
    pub fn err(id: Id, error: RpcError) -> Self {
        Self {
            jsonrpc: "2.0".into(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

/// A notification from the server (an event).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notification {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// [`EVENT_NOTIFICATION`].
    pub method: String,
    /// The event.
    pub params: Value,
}

/// Who is connecting (shown to the person when approving, and in Settings).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientInfo {
    /// A short, stable name, e.g. `teitunnel-cli`, `vscode`, `raycast`.
    pub name: String,
    /// Its version.
    pub version: String,
}

impl ClientInfo {
    /// Whether the name is something Teitunnel will show and remember: 1–64 printable
    /// characters.
    pub fn is_valid(&self) -> bool {
        let valid = |s: &str, max: usize| {
            !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
        };
        valid(&self.name, 64) && self.version.chars().count() <= 64
    }
}

/// `hello` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloParams {
    /// The protocol version the client speaks.
    pub protocol: u32,
    /// The token from the install's token file.
    pub token: String,
    /// Who is connecting.
    pub client: ClientInfo,
}

/// The app answering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    /// `Teitunnel`.
    pub name: String,
    /// The app's version.
    pub version: String,
}

/// `hello` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HelloResult {
    /// The protocol version used from here on.
    pub protocol: u32,
    /// The app.
    pub app: AppInfo,
    /// The person allowed this client to make changes without asking each time.
    pub approved: bool,
    /// Methods available.
    pub methods: Vec<String>,
    /// Event types that can be subscribed to.
    pub events: Vec<String>,
}

/// A connected account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    /// Account id.
    pub id: String,
    /// Its name.
    pub name: String,
}

/// One of this machine's tunnels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TunnelInfo {
    /// Account id.
    pub account_id: String,
    /// Tunnel id.
    pub id: String,
    /// Its name.
    pub name: String,
    /// The machine's default tunnel.
    pub is_default: bool,
    /// `running`, `connecting`, `restarting`, `stopped`, … (see the app's connector
    /// states).
    pub state: String,
}

/// The kinds of share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShareKind {
    /// A random `trycloudflare.com` address run by the app.
    Quick,
    /// A random address run by a `teitunnel share` in a terminal.
    Terminal,
    /// A temporary route on one of the account's domains.
    Domain,
}

/// A share, whoever runs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareInfo {
    /// Pass this to `shares.stop`: a Quick Share's id, a terminal's owner id, or a
    /// domain share's hostname.
    pub id: String,
    /// What kind it is.
    pub kind: ShareKind,
    /// The public URL, once known.
    pub url: Option<String>,
    /// The local service shared.
    pub origin: String,
    /// `starting`, `live`, `reconnecting`, `failed` or `unknown`.
    pub status: String,
    /// Why it failed, in English.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// When it started (milliseconds since the epoch).
    pub started_at: u64,
    /// When it ends by itself (milliseconds since the epoch).
    pub expires_at: Option<u64>,
    /// Requests served so far, when the app runs it.
    pub requests: Option<u64>,
    /// The account, for domain shares.
    pub account_id: Option<String>,
    /// Visitors get a "paused" page (shares on your domain).
    #[serde(default)]
    pub paused: bool,
}

/// Everything at a glance (read from the app, no network).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The app.
    pub app: AppInfo,
    /// Connected accounts.
    pub accounts: Vec<AccountInfo>,
    /// This machine's tunnels.
    pub tunnels: Vec<TunnelInfo>,
    /// Every share.
    pub shares: Vec<ShareInfo>,
}

/// Which Host header a share sends to the service.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum HostHeader {
    /// The app's default: dev servers that only answer their own address get it.
    #[default]
    Auto,
    /// The visitor's Host header, unchanged.
    Off,
    /// This value.
    Set {
        /// E.g. `localhost:5173`.
        value: String,
    },
}

/// `shares.start` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StartShare {
    /// A port (`3000`), `host:port` or URL.
    pub origin: String,
    /// Stops by itself after this many seconds.
    #[serde(default)]
    pub stop_after_seconds: Option<u64>,
    /// The Host header (default: automatic, as in the app).
    #[serde(default)]
    pub host_header: HostHeader,
}

/// `shares.stop` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StopShare {
    /// A share's `id` (or its URL or hostname).
    pub id: String,
}

/// `shares.pause` and `shares.resume` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseShare {
    /// A share on your domain or a route (its hostname or URL), or one of the app's
    /// Quick Shares (its address or id).
    pub id: String,
    /// Account id or name, for a route (needed when several are connected).
    #[serde(default)]
    pub account: Option<String>,
}

/// An AI agent an MCP server serves (`agent.register`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    /// The agent's name as its MCP client reports it, e.g. `claude-code`.
    pub name: String,
    /// Its version.
    #[serde(default)]
    pub version: Option<String>,
    /// The MCP server's mode: `read-only`, `ask` or `full`.
    pub mode: String,
}

impl AgentInfo {
    /// Whether the fields are something Teitunnel will show: a 1–64 character name, and
    /// short, printable version and mode.
    pub fn is_valid(&self) -> bool {
        let valid = |s: &str, max: usize| {
            !s.trim().is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
        };
        valid(&self.name, 64)
            && self.version.as_deref().is_none_or(|v| valid(v, 64))
            && valid(&self.mode, 16)
    }
}

/// `agent.approve` parameters: what the agent wants to do, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentApproval {
    /// The agent (as registered, or named here).
    pub agent: String,
    /// One line, e.g. "Add app.example.com → http://localhost:3000".
    pub title: String,
    /// The plan or details.
    pub details: String,
}

/// `agent.approve` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentDecision {
    /// The person approved it.
    pub approved: bool,
}

/// `routes.list` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutesParams {
    /// Account id or name (needed when several are connected).
    #[serde(default)]
    pub account: Option<String>,
}

/// A route of this machine's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteInfo {
    /// Public hostname.
    pub hostname: String,
    /// Path rule.
    pub path: Option<String>,
    /// Where traffic goes.
    pub origin: String,
    /// `live`, `noDns`, `dnsElsewhere`, `connecting`, `restarting`, `connectionLost`,
    /// `keepsStopping` or `stopped`.
    pub status: String,
    /// The status in a few English words.
    pub status_text: String,
    /// Who may reach it, when a login is required (English).
    pub login: Option<String>,
    /// What visitors run to reach a TCP, SSH or RDP route.
    pub connect: Option<String>,
    /// The tunnel carrying it.
    pub tunnel_id: Option<String>,
    /// Its tunnel's name.
    pub tunnel_name: Option<String>,
    /// A share on your domain (removed when the share stops).
    pub temporary: bool,
}

/// `routes.list` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RoutesList {
    /// The account.
    pub account: AccountInfo,
    /// This machine's tunnels in it.
    pub tunnels: Vec<TunnelInfo>,
    /// Its routes, by domain then hostname.
    pub routes: Vec<RouteInfo>,
}

/// `routes.preview` parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewParams {
    /// Account id or name (needed when several are connected).
    #[serde(default)]
    pub account: Option<String>,
    /// One of this machine's tunnels (id or name); default: the one carrying the route,
    /// or the default tunnel.
    #[serde(default)]
    pub tunnel: Option<String>,
    /// The change, as the app's `Change` (e.g.
    /// `{"type":"addRoute","route":{"hostname":"app.example.com","origin":"3000"}}`).
    pub change: Value,
}

/// A step of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepInfo {
    /// What kind of step.
    pub kind: String,
    /// One English line.
    pub description: String,
    /// The equivalent command, when there is one.
    pub command: Option<String>,
}

/// A plan to review.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanInfo {
    /// The account it's in.
    pub account_id: String,
    /// Steps in order (empty: nothing to change).
    pub steps: Vec<StepInfo>,
    /// Things to review, as structured values (`type` plus fields).
    pub warnings: Vec<Value>,
    /// It replaces or deletes DNS records Teitunnel didn't create: `routes.apply` needs
    /// `confirmed: true`.
    pub requires_confirmation: bool,
    /// Pass it back to `routes.apply`.
    pub fingerprint: String,
}

/// `routes.apply` parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyParams {
    /// Account id or name.
    #[serde(default)]
    pub account: Option<String>,
    /// Tunnel id or name.
    #[serde(default)]
    pub tunnel: Option<String>,
    /// The same change as previewed.
    pub change: Value,
    /// The reviewed plan's fingerprint.
    pub fingerprint: String,
    /// Allow replacing or deleting DNS records Teitunnel didn't create.
    #[serde(default)]
    pub confirmed: bool,
}

/// How an apply ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ApplyOutcome {
    /// Every step was applied.
    Applied,
    /// A step failed and everything was undone.
    RolledBack,
    /// A step failed and some steps couldn't be undone.
    PartiallyApplied,
}

/// `routes.apply` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyResult {
    /// How it ended.
    pub outcome: ApplyOutcome,
    /// What failed (English).
    pub error: Option<String>,
    /// What couldn't be undone (English).
    pub leftovers: Vec<String>,
    /// Hostnames worth checking now.
    pub verify: Vec<String>,
    /// Applied, but the connector couldn't start (English).
    pub connector_error: Option<String>,
}

/// A view of the app's window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "view",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum View {
    /// The Overview.
    Overview,
    /// A route's sheet.
    Route {
        /// Its hostname.
        hostname: String,
    },
    /// Quick Share, optionally one share.
    Share {
        /// The share's id.
        #[serde(default)]
        id: Option<String>,
    },
    /// A share's request inspector.
    Inspector {
        /// The share's id.
        share: String,
    },
    /// The Doctor.
    Doctor,
}

/// A problem the Doctor found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorIssue {
    /// Stable id.
    pub id: String,
    /// Which check found it.
    pub check: String,
    /// `error`, `warning` or `info`.
    pub severity: String,
    /// The account, if any.
    pub account_id: Option<String>,
    /// What it's about (English).
    pub subject: String,
    /// One English line.
    pub title: String,
    /// What it means and what to do (English).
    pub detail: String,
}

/// A local HTTPS domain (`https://shop.test`) on this computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDomainInfo {
    /// E.g. `shop.test`.
    pub name: String,
    /// Where to open it (with the port when it isn't 443/80).
    pub url: String,
    /// The local service, e.g. `http://localhost:3000`.
    pub origin: Option<String>,
    /// Subdomains go to the same service.
    pub wildcard: bool,
    /// Over HTTPS.
    pub https: bool,
    /// Requests are recorded in the inspector.
    pub inspect: bool,
    /// The app answers for it now.
    pub serving: bool,
}

/// `localDomains.list` and `localDomains.reload` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDomainsInfo {
    /// The listeners are up.
    pub running: bool,
    /// The HTTPS port in use.
    pub https_port: Option<u16>,
    /// The plain HTTP port in use.
    pub http_port: Option<u16>,
    /// Why they aren't served, in English.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The domains.
    pub domains: Vec<LocalDomainInfo>,
}

/// `events.subscribe` parameters.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscribeParams {
    /// Event types to receive (default: all).
    #[serde(default)]
    pub events: Option<Vec<String>>,
}

/// Something happened in the app. Clients ignore types they don't know.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
#[non_exhaustive]
pub enum Event {
    /// A share started, stopped or changed status.
    SharesChanged {
        /// The share, when one changed.
        #[serde(default)]
        id: Option<String>,
    },
    /// Routes, tunnels or connectors changed.
    RoutesChanged {
        /// The account, when known.
        #[serde(default)]
        account_id: Option<String>,
    },
    /// A request reached an inspected share (only while its inspector runs).
    RequestArrived {
        /// The share's id, an inspected route's hostname, or a local domain's name.
        share: String,
        /// HTTP method.
        method: String,
        /// Path and query.
        path: String,
        /// Response status, once answered.
        status: Option<u16>,
        /// How long it took.
        duration_ms: Option<u64>,
    },
}

impl Event {
    /// The event's `type`, for subscriptions.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::SharesChanged { .. } => event::SHARES_CHANGED,
            Self::RoutesChanged { .. } => event::ROUTES_CHANGED,
            Self::RequestArrived { .. } => event::REQUEST_ARRIVED,
        }
    }
}

/// Event type names.
pub mod event {
    /// [`super::Event::SharesChanged`].
    pub const SHARES_CHANGED: &str = "sharesChanged";
    /// [`super::Event::RoutesChanged`].
    pub const ROUTES_CHANGED: &str = "routesChanged";
    /// [`super::Event::RequestArrived`].
    pub const REQUEST_ARRIVED: &str = "requestArrived";
    /// Every event type.
    pub const ALL: &[&str] = &[SHARES_CHANGED, ROUTES_CHANGED, REQUEST_ARRIVED];
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn events_are_tagged_and_camel_case() {
        let event = Event::RoutesChanged {
            account_id: Some("a1".into()),
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            json!({"type": "routesChanged", "accountId": "a1"})
        );
        assert_eq!(event.kind(), "routesChanged");
        let parsed: Event = serde_json::from_value(json!({"type": "sharesChanged"})).unwrap();
        assert_eq!(parsed, Event::SharesChanged { id: None });
    }

    #[test]
    fn views_and_host_headers_read_their_tags() {
        let view: View =
            serde_json::from_value(json!({"view": "route", "hostname": "app.example.com"}))
                .unwrap();
        assert_eq!(
            view,
            View::Route {
                hostname: "app.example.com".into()
            }
        );
        let start: StartShare = serde_json::from_value(json!({"origin": "3000"})).unwrap();
        assert_eq!(start.host_header, HostHeader::Auto);
        let start: StartShare = serde_json::from_value(
            json!({"origin": "5173", "hostHeader": {"mode": "set", "value": "localhost:5173"}}),
        )
        .unwrap();
        assert_eq!(
            start.host_header,
            HostHeader::Set {
                value: "localhost:5173".into()
            }
        );
    }

    #[test]
    fn client_names_are_checked() {
        let client = |name: &str| ClientInfo {
            name: name.into(),
            version: "1".into(),
        };
        assert!(client("vscode").is_valid());
        assert!(!client("").is_valid());
        assert!(!client("a\nb").is_valid());
        assert!(!client(&"x".repeat(65)).is_valid());
    }

    #[test]
    fn only_changes_are_mutations() {
        assert!(method::is_mutation(method::SHARES_START));
        assert!(method::is_mutation(method::ROUTES_APPLY));
        assert!(method::is_mutation(method::SHARES_PAUSE));
        assert!(method::is_mutation(method::AGENT_APPROVE));
        assert!(!method::is_mutation(method::AGENT_REGISTER));
        assert!(!method::is_mutation(method::ROUTES_PREVIEW));
        assert!(!method::is_mutation(method::OPEN));
    }
}
