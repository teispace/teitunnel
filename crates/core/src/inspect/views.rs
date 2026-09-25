//! What the inspector shows and takes: taps, exchange rows and details, queries,
//! replay edits and tap settings. Types here cross IPC (the lens types they embed are
//! `specta`-derived in `crates/lens`).

use lens::{
    AgentPreset, ExchangeId, ExchangeKind, ExchangeState, ExchangeView, FaultRule, HeaderRules,
    Latency, NetworkConfig, PausedPage, StubRule, TapId, webhook,
};
use serde::{Deserialize, Serialize};

/// Webhook senders the inspector recognises (Lens's [`webhook::Provider`], named for
/// the IPC types).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum WebhookSender {
    /// Stripe.
    Stripe,
    /// GitHub.
    GitHub,
    /// Slack.
    Slack,
    /// Shopify.
    Shopify,
    /// Standard Webhooks / Svix (Clerk, Resend…).
    StandardWebhooks,
    /// Twilio.
    Twilio,
    /// Linear.
    Linear,
    /// Discord.
    Discord,
}

impl From<webhook::Provider> for WebhookSender {
    fn from(provider: webhook::Provider) -> Self {
        use webhook::Provider as P;
        match provider {
            P::Stripe => Self::Stripe,
            P::GitHub => Self::GitHub,
            P::Slack => Self::Slack,
            P::Shopify => Self::Shopify,
            P::StandardWebhooks => Self::StandardWebhooks,
            P::Twilio => Self::Twilio,
            P::Linear => Self::Linear,
            P::Discord => Self::Discord,
        }
    }
}

impl From<WebhookSender> for webhook::Provider {
    fn from(sender: WebhookSender) -> Self {
        use WebhookSender as S;
        match sender {
            S::Stripe => Self::Stripe,
            S::GitHub => Self::GitHub,
            S::Slack => Self::Slack,
            S::Shopify => Self::Shopify,
            S::StandardWebhooks => Self::StandardWebhooks,
            S::Twilio => Self::Twilio,
            S::Linear => Self::Linear,
            S::Discord => Self::Discord,
        }
    }
}

/// A webhook signature check's result (Lens's [`webhook::Verification`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "result",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum WebhookVerdict {
    /// The signature matches and the timestamp (if any) is recent.
    Valid,
    /// The signature doesn't match, or headers are missing.
    Invalid {
        /// Why (English, technical).
        reason: String,
    },
    /// The signature matches but the timestamp is too old (or in the future).
    Expired {
        /// The signed time (Unix seconds).
        #[cfg_attr(feature = "specta", specta(type = f64))]
        timestamp: u64,
        /// Its age in seconds.
        #[cfg_attr(feature = "specta", specta(type = f64))]
        age_secs: i64,
    },
    /// Not a known signature.
    UnknownProvider,
    /// The body wasn't captured in full.
    NotEnoughData {
        /// Why.
        reason: String,
    },
}

impl From<webhook::Verification> for WebhookVerdict {
    fn from(verification: webhook::Verification) -> Self {
        use webhook::Verification as V;
        match verification {
            V::Valid => Self::Valid,
            V::Invalid { reason } => Self::Invalid { reason },
            V::Expired {
                timestamp,
                age_secs,
            } => Self::Expired {
                timestamp,
                age_secs,
            },
            V::UnknownProvider => Self::UnknownProvider,
            V::NotEnoughData { reason } => Self::NotEnoughData { reason },
        }
    }
}

/// Export formats for captured requests (Lens's `export::ExportFormat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum TrafficFormat {
    /// A `curl` command.
    Curl,
    /// An HTTPie command.
    Httpie,
    /// A JavaScript `fetch` call.
    Fetch,
    /// Raw HTTP/1.1.
    Raw,
    /// HAR 1.2.
    Har,
    /// JSON.
    Json,
    /// Markdown (issues, agents).
    Markdown,
}

impl From<TrafficFormat> for lens::export::ExportFormat {
    fn from(format: TrafficFormat) -> Self {
        use TrafficFormat as F;
        match format {
            F::Curl => Self::Curl,
            F::Httpie => Self::Httpie,
            F::Fetch => Self::Fetch,
            F::Raw => Self::Raw,
            F::Har => Self::Har,
            F::Json => Self::Json,
            F::Markdown => Self::Markdown,
        }
    }
}

/// What a tap inspects.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TapScope {
    /// A Quick Share.
    QuickShare {
        /// The share's id.
        share_id: String,
    },
    /// A route (or a share on your domain, which is a temporary route).
    Route {
        /// Account id.
        account_id: String,
        /// Public hostname.
        hostname: String,
        /// Path rule.
        path: Option<String>,
    },
    /// A local HTTPS domain on this computer (`https://shop.test`).
    LocalDomain {
        /// The name, e.g. `shop.test`.
        name: String,
    },
}

impl TapScope {
    /// A route's scope with the hostname normalised.
    pub fn route(account_id: &str, hostname: &str, path: Option<&str>) -> Self {
        Self::Route {
            account_id: account_id.to_owned(),
            hostname: hostname.trim().to_ascii_lowercase(),
            path: path
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_owned),
        }
    }

    /// Where webhook secrets for this scope are kept: a route's hostname, or a Quick
    /// Share's local service (its public address changes every time).
    pub fn secret_scope(&self, origin: &str) -> String {
        match self {
            Self::QuickShare { .. } => format!("origin:{}", origin.trim().to_ascii_lowercase()),
            Self::Route { hostname, .. } => super::secrets::host_scope(hostname),
            Self::LocalDomain { name } => format!("local:{name}"),
        }
    }
}

/// Protection on a tap, without its secrets.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TapProtectionView {
    /// A password page.
    pub password: bool,
    /// A secret link.
    pub secret_link: bool,
    /// HTTP basic authentication (the user name).
    pub basic_user: Option<String>,
    /// Bearer tokens accepted.
    pub bearer_tokens: u32,
    /// Networks allowed (empty: everyone).
    pub ip_allow: Vec<String>,
    /// Networks refused.
    pub ip_deny: Vec<String>,
    /// Blocked user-agent lists.
    pub agent_presets: Vec<AgentPreset>,
    /// Extra blocked user-agent text.
    pub agent_patterns: Vec<String>,
    /// Paths that skip sign-in (e.g. `/webhooks/*`).
    pub bypass: Vec<String>,
}

/// A tap: one inspected share or route.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TapView {
    /// Id (captures refer to it).
    pub id: TapId,
    /// What it inspects.
    pub scope: TapScope,
    /// Display name (the public hostname or the share's URL, else the service).
    pub name: String,
    /// The local service behind it, e.g. `http://localhost:3000`.
    pub origin: String,
    /// The public URL, once known.
    pub public_url: Option<String>,
    /// Where cloudflared sends requests (the inspector's local address).
    pub address: String,
    /// When it started (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub started_at: u64,
    /// Requests are recorded.
    pub capturing: bool,
    /// The paused page is served instead of the service.
    pub paused: Option<PausedPage>,
    /// Protection.
    pub protection: TapProtectionView,
    /// The Host header the service gets (`None`: the visitor's).
    pub host_header: Option<String>,
    /// Seconds of silence before an event stream gets a keep-alive comment (`None`:
    /// off).
    pub sse_keepalive_secs: Option<u32>,
    /// Canned responses.
    pub stubs: Vec<StubRule>,
    /// Header rewrites.
    pub header_rules: HeaderRules,
    /// Simulated network.
    pub network: NetworkConfig,
    /// Injected faults.
    pub faults: Vec<FaultRule>,
    /// Paths that notify when requested.
    pub watched_paths: Vec<String>,
    /// Minutes without a request before the share stops (`None`: never).
    pub idle_stop_minutes: Option<u32>,
    /// Requests seen since it started.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub requests: u64,
    /// Reviewers can pin comments to its pages (the overlay is added to HTML pages).
    pub comments: bool,
}

/// A captured exchange in a list.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ExchangeRow {
    /// Id.
    pub id: ExchangeId,
    /// Tap.
    pub tap: TapId,
    /// Number within the tap.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub seq: u64,
    /// When the request arrived (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub started_at: u64,
    /// Method.
    pub method: String,
    /// Host the visitor asked for.
    pub host: String,
    /// Path and query, masked.
    pub path: String,
    /// Status, once answered.
    pub status: Option<u16>,
    /// Duration in milliseconds, when known.
    pub duration_ms: Option<f64>,
    /// Request body bytes.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub request_bytes: u64,
    /// Response body bytes.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub response_bytes: u64,
    /// HTTP, WebSocket, SSE or another upgrade.
    pub kind: ExchangeKind,
    /// Where it is in its life.
    pub state: ExchangeState,
    /// `Content-Type` of the response.
    pub content_type: Option<String>,
    /// A recognised webhook sender.
    pub webhook: Option<WebhookSender>,
    /// Replays this exchange.
    pub replay_of: Option<ExchangeId>,
    /// Answered by the inspector (a stub, a gate, the paused page, a fault) rather than
    /// the service.
    pub answered_locally: bool,
    /// An error reaching the service, in English (technical).
    pub error: Option<String>,
}

impl ExchangeRow {
    /// The row for `exchange`, masked.
    pub fn of(exchange: &lens::Exchange) -> Self {
        let redaction = lens::Redaction::masked();
        let request = &exchange.request;
        let path = match request.query() {
            Some(query) => format!(
                "{}?{}",
                lens::mask_text(request.path(), &redaction),
                lens::mask_query(query, &redaction)
            ),
            None => lens::mask_text(request.path(), &redaction).into_owned(),
        };
        Self {
            id: exchange.id,
            tap: exchange.tap.clone(),
            seq: exchange.seq,
            started_at: exchange.started_at_ms,
            method: request.method.to_string(),
            host: request.host.clone(),
            path,
            status: exchange.status().map(|s| s.as_u16()),
            duration_ms: exchange.duration().map(|d| d.as_secs_f64() * 1_000.0),
            request_bytes: request.body.size,
            response_bytes: exchange.response.as_ref().map_or(0, |r| r.body.size),
            kind: exchange.kind,
            state: exchange.state,
            content_type: exchange.response.as_ref().and_then(|r| {
                r.headers
                    .get(http::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .map(str::to_owned)
            }),
            webhook: webhook::detect(&request.headers).map(WebhookSender::from),
            replay_of: exchange.replay_of,
            answered_locally: !matches!(
                exchange.responder,
                lens::Responder::Upstream | lens::Responder::Folder
            ),
            error: exchange.error.as_ref().map(|e| e.message.clone()),
        }
    }
}

/// A page of exchanges.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ExchangePage {
    /// Newest first.
    pub items: Vec<ExchangeRow>,
    /// Pass as `before` for the next (older) page.
    pub next: Option<ExchangeId>,
}

/// Which exchanges to list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ExchangeQuery {
    /// Only this tap.
    #[serde(default)]
    pub tap: Option<TapId>,
    /// Any of these methods.
    #[serde(default)]
    pub methods: Vec<String>,
    /// Status classes (`2` for 2xx…).
    #[serde(default)]
    pub status_classes: Vec<u8>,
    /// Exact statuses.
    #[serde(default)]
    pub statuses: Vec<u16>,
    /// Text in the path.
    #[serde(default)]
    pub path: Option<String>,
    /// Text in the host.
    #[serde(default)]
    pub host: Option<String>,
    /// Text anywhere (URL, headers, bodies; secrets can't be searched).
    #[serde(default)]
    pub text: Option<String>,
    /// At least this slow (milliseconds).
    #[serde(default)]
    pub min_duration_ms: Option<u32>,
    /// Started at or after (milliseconds since the epoch).
    #[serde(default)]
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub since_ms: Option<u64>,
    /// Kinds.
    #[serde(default)]
    pub kinds: Vec<ExchangeKind>,
    /// Only failed ones.
    #[serde(default)]
    pub errors_only: bool,
    /// Page size (default 100, at most 1,000).
    #[serde(default)]
    pub limit: Option<u32>,
    /// Older than this exchange (the previous page's `next`).
    #[serde(default)]
    pub before: Option<ExchangeId>,
}

impl ExchangeQuery {
    /// As Lens's filter.
    pub fn filter(&self) -> lens::Filter {
        lens::Filter {
            tap: self.tap.clone(),
            methods: self.methods.clone(),
            status_classes: self.status_classes.clone(),
            statuses: self.statuses.clone(),
            path: self.path.clone().filter(|p| !p.is_empty()),
            host: self.host.clone().filter(|h| !h.is_empty()),
            text: self.text.clone().filter(|t| !t.is_empty()),
            min_duration_ms: self.min_duration_ms.map(u64::from),
            since_ms: self.since_ms,
            until_ms: None,
            kinds: self.kinds.clone(),
            errors_only: self.errors_only,
            finished_only: false,
        }
    }

    /// As Lens's query.
    pub fn query(&self) -> lens::Query {
        lens::Query {
            filter: self.filter(),
            limit: self
                .limit
                .map(|l| usize::try_from(l).unwrap_or(lens::MAX_PAGE)),
            before: self.before,
        }
    }
}

/// A webhook signature check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct WebhookCheck {
    /// Who sent it.
    pub provider: WebhookSender,
    /// Whether a signing secret is saved for this share or route.
    pub has_secret: bool,
    /// The result, when a secret is saved.
    pub verification: Option<WebhookVerdict>,
}

/// One exchange in full.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ExchangeDetail {
    /// Everything captured (masked unless revealed).
    pub view: ExchangeView,
    /// The webhook sender and its signature check.
    pub webhook: Option<WebhookCheck>,
    /// Read back from the history kept on disk (credentials were masked when stored).
    pub restored: bool,
}

/// Changes to a request before replaying it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ReplayInput {
    /// Another method.
    #[serde(default)]
    pub method: Option<String>,
    /// Another path and query (starting with `/`).
    #[serde(default)]
    pub path: Option<String>,
    /// Headers to set.
    #[serde(default)]
    pub set_headers: Vec<(String, String)>,
    /// Headers to remove.
    #[serde(default)]
    pub remove_headers: Vec<String>,
    /// Another body (text).
    #[serde(default)]
    pub body: Option<String>,
    /// How many times, one after another (1–100).
    #[serde(default)]
    pub times: Option<u32>,
    /// Recompute the webhook signature with the saved secret (fresh timestamp).
    #[serde(default)]
    pub resign: bool,
}

/// Network presets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum NetworkPreset {
    /// No simulation.
    Off,
    /// 3G latency and bandwidth.
    ThreeG,
    /// 4G latency and bandwidth.
    FourG,
    /// Satellite latency.
    Satellite,
}

impl NetworkPreset {
    /// The simulated network.
    pub fn config(self) -> NetworkConfig {
        match self {
            Self::Off => NetworkConfig::default(),
            Self::ThreeG => NetworkConfig {
                latency: Some(Latency::THREE_G),
                up_bytes_per_sec: Some(96 * 1024),
                down_bytes_per_sec: Some(200 * 1024),
            },
            Self::FourG => NetworkConfig {
                latency: Some(Latency::FOUR_G),
                up_bytes_per_sec: Some(1_500 * 1024),
                down_bytes_per_sec: Some(4_000 * 1024),
            },
            Self::Satellite => NetworkConfig {
                latency: Some(Latency::SATELLITE),
                up_bytes_per_sec: Some(256 * 1024),
                down_bytes_per_sec: Some(2_000 * 1024),
            },
        }
    }
}

/// Changes to a tap; fields left out stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TapPatch {
    /// Record requests.
    #[serde(default)]
    pub capturing: Option<bool>,
    /// Serve the paused page (`true` uses the default page unless `pausedPage` is set).
    #[serde(default)]
    pub paused: Option<bool>,
    /// The paused page's text.
    #[serde(default)]
    pub paused_page: Option<PausedPage>,
    /// Canned responses (replacing the list).
    #[serde(default)]
    pub stubs: Option<Vec<StubRule>>,
    /// Header rewrites.
    #[serde(default)]
    pub header_rules: Option<HeaderRules>,
    /// A network preset.
    #[serde(default)]
    pub network_preset: Option<NetworkPreset>,
    /// A custom simulated network (wins over the preset).
    #[serde(default)]
    pub network: Option<NetworkConfig>,
    /// Injected faults (replacing the list).
    #[serde(default)]
    pub faults: Option<Vec<FaultRule>>,
    /// Keep-alive for event streams after this many seconds of silence; 0 turns it
    /// off.
    #[serde(default)]
    pub sse_keepalive_secs: Option<u32>,
    /// Host header for the service; empty sends the visitor's.
    #[serde(default)]
    pub host_header: Option<String>,
    /// Paths that notify when requested (replacing the list).
    #[serde(default)]
    pub watched_paths: Option<Vec<String>>,
    /// Minutes without a request before the share stops; 0 turns it off.
    #[serde(default)]
    pub idle_stop_minutes: Option<u32>,
}

/// Protection to set on a tap. Secrets go in, never out: the answer only says what's on
/// (and shows a generated secret link or token once).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProtectionInput {
    /// A password page with this password; empty removes it; left out keeps it.
    #[serde(default)]
    pub password: Option<String>,
    /// Create a new secret link (`true`) or remove it (`false`).
    #[serde(default)]
    pub secret_link: Option<bool>,
    /// HTTP basic authentication `[user, password]`; an empty user removes it.
    #[serde(default)]
    pub basic: Option<(String, String)>,
    /// Create a new bearer token (`true`, replacing the others) or remove them all
    /// (`false`).
    #[serde(default)]
    pub bearer: Option<bool>,
    /// Networks allowed (replacing the list).
    #[serde(default)]
    pub ip_allow: Option<Vec<String>>,
    /// Networks refused (replacing the list).
    #[serde(default)]
    pub ip_deny: Option<Vec<String>>,
    /// Blocked user-agent lists.
    #[serde(default)]
    pub agent_presets: Option<Vec<AgentPreset>>,
    /// Extra blocked user-agent text.
    #[serde(default)]
    pub agent_patterns: Option<Vec<String>>,
    /// Paths that skip sign-in.
    #[serde(default)]
    pub bypass: Option<Vec<String>>,
}

/// Protection after a change, and secrets generated by it (shown once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProtectionResult {
    /// What's on now.
    pub protection: TapProtectionView,
    /// A new secret link's key (`?key=…`), shown once.
    pub secret_link_key: Option<String>,
    /// A new bearer token, shown once.
    pub bearer_token: Option<String>,
}
