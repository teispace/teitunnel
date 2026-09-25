//! Edge protection for one hostname: bot and AI-crawler rules, a rate limit and header
//! rules, written as Cloudflare Ruleset Engine rules scoped to that hostname, never to a
//! whole domain (docs/research/cloudflare-edge-rules.md). Pure, except [`observe`].
//!
//! Teitunnel owns only the rules it creates: they carry a `teitunnel:` description
//! marker and are listed in the local ownership index (migration 15). Per-hostname
//! rules are `teitunnel:<route-id>:<kind>`; rate limits, which plans allow only a few
//! of, are shared by every hostname with the same limit in a zone:
//! `teitunnel:ratelimit:<requests>-<period>-<action>` over `http.host in {…}`.

use std::collections::{BTreeSet, HashSet};

use cf_api::{NewRule, RateLimit};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::cloud::CloudApi;
use crate::domain::Hostname;
use crate::text::{Text, UserText, english_display, msg};

/// The description prefix of every rule Teitunnel creates.
pub const RULE_MARKER: &str = "teitunnel:";
const RATE_LIMIT_MARKER: &str = "teitunnel:ratelimit:";

/// What to do with automated clients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum BotMode {
    /// Nothing.
    #[default]
    Off,
    /// A managed challenge (most people never see it; scripts can't pass it).
    Challenge,
    /// Refused.
    Block,
}

/// What happens to a visitor over the rate limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum LimitAction {
    /// Refused until the period ends.
    #[default]
    Block,
    /// A managed challenge until the period ends.
    Challenge,
}

impl LimitAction {
    fn cloudflare(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Challenge => "managed_challenge",
        }
    }

    fn from_cloudflare(action: &str) -> Option<Self> {
        match action {
            "block" => Some(Self::Block),
            "managed_challenge" => Some(Self::Challenge),
            _ => None,
        }
    }
}

/// Requests per period per visitor (IP address).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RateLimitSpec {
    /// Requests allowed per period.
    pub requests: u32,
    /// The period, in seconds (10, 60, 120, 300, 600 or 3600).
    pub period: u32,
    /// What happens above it.
    pub action: LimitAction,
}

/// Rate limit periods Cloudflare supports, in seconds.
pub const PERIODS: [u32; 6] = [10, 60, 120, 300, 600, 3600];

/// What a header rule does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum EdgeHeaderOp {
    /// Set the header to a value (replacing it).
    Set,
    /// Add a value (response headers only; keeps existing ones).
    Add,
    /// Remove the header.
    Remove,
}

impl EdgeHeaderOp {
    fn cloudflare(self) -> &'static str {
        match self {
            Self::Set => "set",
            Self::Add => "add",
            Self::Remove => "remove",
        }
    }
}

/// One header change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct HeaderRule {
    /// Header name, e.g. `X-Robots-Tag`.
    pub name: String,
    /// What to do.
    pub op: EdgeHeaderOp,
    /// The value, for `set` and `add`.
    #[serde(default)]
    pub value: Option<String>,
}

/// Everything Teitunnel can enforce at Cloudflare's edge for one hostname. The default
/// is nothing (no rules).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct EdgeProtection {
    /// Automated clients (scripts, headless browsers) that aren't verified bots.
    #[serde(default)]
    pub bots: BotMode,
    /// Block AI crawlers (Cloudflare's verified "AI Crawler" category).
    #[serde(default)]
    pub ai_crawlers: bool,
    /// Requests per period per visitor.
    #[serde(default)]
    pub rate_limit: Option<RateLimitSpec>,
    /// Headers changed on requests before they reach the origin.
    #[serde(default)]
    pub request_headers: Vec<HeaderRule>,
    /// Headers changed on responses before they reach visitors.
    #[serde(default)]
    pub response_headers: Vec<HeaderRule>,
}

/// Why protection settings were rejected. Shown next to the field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EdgeInputError {
    /// A header name that isn't a valid token.
    HeaderName(String),
    /// A header value with control characters, or too long.
    HeaderValue(String),
    /// `set`/`add` without a value.
    MissingValue(String),
    /// The same header twice in one list.
    DuplicateHeader(String),
    /// A header Cloudflare doesn't let rules change on requests.
    ProtectedHeader(String),
    /// Cookies can only be removed from requests.
    CookieRemoveOnly,
    /// `add` only exists for response headers.
    AddOnRequest(String),
    /// More header rules than fit.
    TooManyHeaders,
    /// Zero or absurdly many requests.
    Requests,
    /// A period Cloudflare doesn't support.
    Period(u32),
}

impl UserText for EdgeInputError {
    fn text(&self) -> Text {
        use msg::protection::input as m;
        match self {
            Self::HeaderName(name) => m::header_name(name),
            Self::HeaderValue(name) => m::header_value(name),
            Self::MissingValue(name) => m::missing_value(name),
            Self::DuplicateHeader(name) => m::duplicate_header(name),
            Self::ProtectedHeader(name) => m::protected_header(name),
            Self::CookieRemoveOnly => m::cookie_remove_only(),
            Self::AddOnRequest(name) => m::add_on_request(name),
            Self::TooManyHeaders => m::too_many_headers(MAX_HEADERS as u64),
            Self::Requests => m::requests(),
            Self::Period(period) => m::period(u64::from(*period)),
        }
    }
}

english_display!(EdgeInputError);

/// Header rules per list.
pub const MAX_HEADERS: usize = 20;

/// Request headers Cloudflare's rules can't change (docs, 2026-09-04), besides
/// `cf-*`/`x-cf-*`.
const PROTECTED_REQUEST: &[&str] = &[
    "x-forwarded-for",
    "true-client-ip",
    "x-real-ip",
    "x-forwarded-proto",
    "accept-encoding",
    "host",
    "content-length",
    "connection",
    "transfer-encoding",
    "upgrade",
    "te",
    "trailer",
    "keep-alive",
];

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn normalize_headers(
    list: &[HeaderRule],
    request: bool,
) -> Result<Vec<HeaderRule>, EdgeInputError> {
    if list.len() > MAX_HEADERS {
        return Err(EdgeInputError::TooManyHeaders);
    }
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(list.len());
    for rule in list {
        let name = rule.name.trim().to_owned();
        if !valid_name(&name) {
            return Err(EdgeInputError::HeaderName(name));
        }
        let lower = name.to_ascii_lowercase();
        if !seen.insert(lower.clone()) {
            return Err(EdgeInputError::DuplicateHeader(name));
        }
        if request {
            let cf = lower.starts_with("cf-") || lower.starts_with("x-cf-");
            let removable_cf = lower == "cf-connecting-ip" && rule.op == EdgeHeaderOp::Remove;
            if (cf && !removable_cf) || PROTECTED_REQUEST.contains(&lower.as_str()) {
                return Err(EdgeInputError::ProtectedHeader(name));
            }
            if lower == "cookie" && rule.op != EdgeHeaderOp::Remove {
                return Err(EdgeInputError::CookieRemoveOnly);
            }
            if rule.op == EdgeHeaderOp::Add {
                return Err(EdgeInputError::AddOnRequest(name));
            }
        }
        let value = match rule.op {
            EdgeHeaderOp::Remove => None,
            EdgeHeaderOp::Set | EdgeHeaderOp::Add => {
                let value = rule.value.as_deref().map(str::trim).unwrap_or_default();
                if value.is_empty() {
                    return Err(EdgeInputError::MissingValue(name));
                }
                if value.len() > 1024 || value.chars().any(char::is_control) {
                    return Err(EdgeInputError::HeaderValue(name));
                }
                Some(value.to_owned())
            }
        };
        out.push(HeaderRule {
            name,
            op: rule.op,
            value,
        });
    }
    out.sort_by_key(|r| r.name.to_ascii_lowercase());
    Ok(out)
}

impl EdgeProtection {
    /// Nothing enforced.
    pub fn is_off(&self) -> bool {
        *self == Self::default()
    }

    /// Checks and tidies the settings (header names trimmed, lists sorted).
    ///
    /// # Errors
    /// See [`EdgeInputError`].
    pub fn normalized(&self) -> Result<Self, EdgeInputError> {
        if let Some(limit) = &self.rate_limit {
            if limit.requests == 0 || limit.requests > 1_000_000 {
                return Err(EdgeInputError::Requests);
            }
            if !PERIODS.contains(&limit.period) {
                return Err(EdgeInputError::Period(limit.period));
            }
        }
        Ok(Self {
            bots: self.bots,
            ai_crawlers: self.ai_crawlers,
            rate_limit: self.rate_limit,
            request_headers: normalize_headers(&self.request_headers, true)?,
            response_headers: normalize_headers(&self.response_headers, false)?,
        })
    }
}

/// A zone's Cloudflare plan, for its rule quotas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ZonePlan {
    /// Free (also any plan Teitunnel doesn't know: the smallest limits).
    Free,
    /// Pro.
    Pro,
    /// Business.
    Business,
    /// Enterprise.
    Enterprise,
}

/// What a plan allows (docs/research/cloudflare-edge-rules.md, 2026-09-24).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanLimits {
    /// Custom rules.
    pub custom: u32,
    /// Rate limiting rules.
    pub rate_limit: u32,
    /// Transform Rules (every kind together).
    pub transform: u32,
    /// Whether rate limiting rules can match a hostname (`http.host`; Pro and up).
    pub host_rate_limit: bool,
    /// The longest rate limit period, in seconds.
    pub longest_period: u32,
}

impl ZonePlan {
    /// The plan from the zone's `plan.legacy_id`.
    pub fn from_legacy_id(id: Option<&str>) -> Self {
        match id {
            Some("pro") => Self::Pro,
            Some("business") => Self::Business,
            Some("enterprise") => Self::Enterprise,
            _ => Self::Free,
        }
    }

    /// Its limits.
    pub fn limits(self) -> PlanLimits {
        match self {
            Self::Free => PlanLimits {
                custom: 5,
                rate_limit: 1,
                transform: 10,
                host_rate_limit: false,
                longest_period: 10,
            },
            Self::Pro => PlanLimits {
                custom: 20,
                rate_limit: 2,
                transform: 25,
                host_rate_limit: true,
                longest_period: 60,
            },
            Self::Business => PlanLimits {
                custom: 100,
                rate_limit: 5,
                transform: 50,
                host_rate_limit: true,
                longest_period: 600,
            },
            Self::Enterprise => PlanLimits {
                custom: 1000,
                rate_limit: 100,
                transform: 300,
                host_rate_limit: true,
                longest_period: 3600,
            },
        }
    }
}

/// Which of Teitunnel's rules a rule is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum RuleKind {
    /// Blocks automated clients and/or AI crawlers.
    Block,
    /// Challenges automated clients.
    Challenge,
    /// A shared rate limit.
    RateLimit,
    /// Request header changes.
    RequestHeaders,
    /// Response header changes.
    ResponseHeaders,
}

impl RuleKind {
    fn slug(self) -> &'static str {
        match self {
            Self::Block => "block",
            Self::Challenge => "challenge",
            Self::RateLimit => "ratelimit",
            Self::RequestHeaders => "request-headers",
            Self::ResponseHeaders => "response-headers",
        }
    }

    /// The phase its rules live in.
    pub fn phase(self) -> &'static str {
        match self {
            Self::Block | Self::Challenge => cf_api::PHASE_CUSTOM,
            Self::RateLimit => cf_api::PHASE_RATE_LIMIT,
            Self::RequestHeaders => cf_api::PHASE_REQUEST_HEADERS,
            Self::ResponseHeaders => cf_api::PHASE_RESPONSE_HEADERS,
        }
    }

    /// The quota its rules count towards.
    pub fn quota(self) -> QuotaKind {
        match self {
            Self::Block | Self::Challenge => QuotaKind::Custom,
            Self::RateLimit => QuotaKind::RateLimit,
            Self::RequestHeaders | Self::ResponseHeaders => QuotaKind::Transform,
        }
    }
}

/// A plan quota.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum QuotaKind {
    /// Custom rules.
    Custom,
    /// Rate limiting rules.
    RateLimit,
    /// Transform Rules.
    Transform,
}

impl QuotaKind {
    /// The phases whose rules count towards it.
    pub fn phases(self) -> &'static [&'static str] {
        match self {
            Self::Custom => &[cf_api::PHASE_CUSTOM],
            Self::RateLimit => &[cf_api::PHASE_RATE_LIMIT],
            Self::Transform => &[
                cf_api::PHASE_REQUEST_HEADERS,
                cf_api::PHASE_RESPONSE_HEADERS,
                cf_api::PHASE_URL_REWRITE,
            ],
        }
    }

    /// Its limit on `plan`.
    pub fn limit(self, plan: ZonePlan) -> u32 {
        let limits = plan.limits();
        match self {
            Self::Custom => limits.custom,
            Self::RateLimit => limits.rate_limit,
            Self::Transform => limits.transform,
        }
    }
}

/// The marker of one of `hostname`'s rules.
pub fn marker(hostname: &Hostname, kind: RuleKind) -> String {
    format!(
        "{RULE_MARKER}{}:{}",
        super::views::route_id(hostname, None),
        kind.slug()
    )
}

/// The marker of the shared rate limit with `spec`.
pub fn rate_limit_marker(spec: &RateLimitSpec) -> String {
    format!(
        "{RATE_LIMIT_MARKER}{}-{}-{}",
        spec.requests,
        spec.period,
        spec.action.cloudflare()
    )
}

/// Automated clients that aren't verified bots: no user agent, or one of a script,
/// library or headless browser (no regex: Free and Pro don't have it).
pub const BOTS_EXPRESSION: &str = r#"(not cf.client.bot and (http.user_agent eq "" or lower(http.user_agent) contains "curl" or lower(http.user_agent) contains "wget" or lower(http.user_agent) contains "python-requests" or lower(http.user_agent) contains "python-urllib" or lower(http.user_agent) contains "aiohttp" or lower(http.user_agent) contains "httpx" or lower(http.user_agent) contains "go-http-client" or lower(http.user_agent) contains "okhttp" or lower(http.user_agent) contains "java/" or lower(http.user_agent) contains "libwww-perl" or lower(http.user_agent) contains "node-fetch" or lower(http.user_agent) contains "axios" or lower(http.user_agent) contains "scrapy" or lower(http.user_agent) contains "headlesschrome" or lower(http.user_agent) contains "phantomjs"))"#;

/// Cloudflare's verified AI crawlers.
pub const AI_EXPRESSION: &str = r#"(cf.verified_bot_category eq "AI Crawler")"#;

fn host_expression(hostname: &Hostname) -> String {
    format!(r#"(http.host eq "{hostname}")"#)
}

/// The rate limit expression for `hostnames` (sorted, deduplicated).
pub fn hosts_expression(hostnames: &BTreeSet<String>) -> String {
    let list: Vec<String> = hostnames.iter().map(|h| format!("\"{h}\"")).collect();
    format!("(http.host in {{{}}})", list.join(" "))
}

/// The hostnames of a rate limit expression Teitunnel wrote.
pub fn hosts_of(expression: &str) -> BTreeSet<String> {
    let Some(inner) = expression
        .split_once('{')
        .and_then(|(_, rest)| rest.split_once('}'))
        .map(|(inner, _)| inner)
    else {
        return BTreeSet::new();
    };
    inner
        .split_whitespace()
        .map(|h| h.trim_matches('"').to_ascii_lowercase())
        .filter(|h| !h.is_empty())
        .collect()
}

fn headers_parameters(list: &[HeaderRule]) -> Value {
    let headers: Map<String, Value> = list
        .iter()
        .map(|rule| {
            let mut op = json!({ "operation": rule.op.cloudflare() });
            if let (Some(value), Some(object)) = (&rule.value, op.as_object_mut()) {
                object.insert("value".into(), Value::String(value.clone()));
            }
            (rule.name.clone(), op)
        })
        .collect();
    json!({ "headers": headers })
}

fn headers_of(parameters: Option<&Value>) -> Vec<HeaderRule> {
    let Some(headers) = parameters
        .and_then(|p| p.get("headers"))
        .and_then(Value::as_object)
    else {
        return Vec::new();
    };
    let mut out: Vec<HeaderRule> = headers
        .iter()
        .filter_map(|(name, op)| {
            let op_kind = match op.get("operation")?.as_str()? {
                "set" => EdgeHeaderOp::Set,
                "add" => EdgeHeaderOp::Add,
                "remove" => EdgeHeaderOp::Remove,
                _ => return None,
            };
            Some(HeaderRule {
                name: name.clone(),
                op: op_kind,
                value: op.get("value").and_then(Value::as_str).map(str::to_owned),
            })
        })
        .collect();
    out.sort_by_key(|r| r.name.to_ascii_lowercase());
    out
}

/// The per-hostname rules `protection` needs (the rate limit is shared: see
/// [`rate_limit_rule`]).
pub fn hostname_rules(
    hostname: &Hostname,
    protection: &EdgeProtection,
) -> Vec<(RuleKind, NewRule)> {
    let host = host_expression(hostname);
    let rule =
        |kind: RuleKind, action: &str, condition: Option<String>, params: Option<Value>| NewRule {
            action: action.to_owned(),
            expression: condition.map_or_else(|| host.clone(), |c| format!("{host} and {c}")),
            description: marker(hostname, kind),
            enabled: true,
            action_parameters: params,
            ratelimit: None,
        };
    let mut rules = Vec::new();
    let mut blocked = Vec::new();
    if protection.bots == BotMode::Block {
        blocked.push(BOTS_EXPRESSION);
    }
    if protection.ai_crawlers {
        blocked.push(AI_EXPRESSION);
    }
    if !blocked.is_empty() {
        let condition = if blocked.len() == 1 {
            blocked[0].to_owned()
        } else {
            format!("({})", blocked.join(" or "))
        };
        rules.push((
            RuleKind::Block,
            rule(RuleKind::Block, "block", Some(condition), None),
        ));
    }
    if protection.bots == BotMode::Challenge {
        rules.push((
            RuleKind::Challenge,
            rule(
                RuleKind::Challenge,
                "managed_challenge",
                Some(BOTS_EXPRESSION.to_owned()),
                None,
            ),
        ));
    }
    for (kind, list) in [
        (RuleKind::RequestHeaders, &protection.request_headers),
        (RuleKind::ResponseHeaders, &protection.response_headers),
    ] {
        if !list.is_empty() {
            rules.push((
                kind,
                rule(kind, "rewrite", None, Some(headers_parameters(list))),
            ));
        }
    }
    rules
}

/// The shared rate limit rule for `hostnames` with `spec`.
pub fn rate_limit_rule(spec: &RateLimitSpec, hostnames: &BTreeSet<String>) -> NewRule {
    NewRule {
        action: spec.action.cloudflare().to_owned(),
        expression: hosts_expression(hostnames),
        description: rate_limit_marker(spec),
        enabled: true,
        action_parameters: None,
        ratelimit: Some(RateLimit {
            characteristics: vec!["cf.colo.id".into(), "ip.src".into()],
            period: spec.period,
            requests_per_period: spec.requests,
            mitigation_timeout: spec.period,
        }),
    }
}

/// The limit a rate limit rule enforces, if Teitunnel wrote it.
pub fn rate_limit_spec(rule: &NewRule) -> Option<RateLimitSpec> {
    if !rule.description.starts_with(RATE_LIMIT_MARKER) {
        return None;
    }
    let limit = rule.ratelimit.as_ref()?;
    Some(RateLimitSpec {
        requests: limit.requests_per_period,
        period: limit.period,
        action: LimitAction::from_cloudflare(&rule.action)?,
    })
}

/// What a step does to a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Adds it.
    Add,
    /// Changes it.
    Change,
    /// Removes it.
    Remove,
}

/// A plan step's line for one of Teitunnel's rules.
pub fn describe_rule(verb: Verb, kind: RuleKind, hostnames: &[String], rule: &NewRule) -> Text {
    use msg::protection::step as m;
    let host = hostnames.join(", ");
    let bots = rule.expression.contains(BOTS_EXPRESSION);
    let ai = rule.expression.contains(AI_EXPRESSION);
    let headers = rule
        .action_parameters
        .as_ref()
        .and_then(|p| p.get("headers"))
        .and_then(Value::as_object)
        .map_or(0, |h| h.len() as u64);
    let limit = rate_limit_spec(rule);
    let (requests, seconds) =
        limit.map_or((0, 0), |l| (u64::from(l.requests), u64::from(l.period)));
    let challenge = limit.is_some_and(|l| l.action == LimitAction::Challenge);
    match (verb, kind) {
        (Verb::Remove, RuleKind::Block) => m::remove::block(host),
        (Verb::Remove, RuleKind::Challenge) => m::remove::challenge(host),
        (Verb::Remove, RuleKind::RateLimit) => m::remove::rate_limit(host),
        (Verb::Remove, RuleKind::RequestHeaders) => m::remove::request_headers(host),
        (Verb::Remove, RuleKind::ResponseHeaders) => m::remove::response_headers(host),
        (Verb::Add, RuleKind::Block) if bots && ai => m::add::block_both(host),
        (Verb::Add, RuleKind::Block) if ai => m::add::block_ai(host),
        (Verb::Add, RuleKind::Block) => m::add::block(host),
        (Verb::Add, RuleKind::Challenge) => m::add::challenge(host),
        (Verb::Add, RuleKind::RateLimit) if challenge => {
            m::add::rate_limit_challenge(host, requests, seconds)
        }
        (Verb::Add, RuleKind::RateLimit) => m::add::rate_limit_block(host, requests, seconds),
        (Verb::Add, RuleKind::RequestHeaders) => m::add::request_headers(headers, host),
        (Verb::Add, RuleKind::ResponseHeaders) => m::add::response_headers(headers, host),
        (Verb::Change, RuleKind::Block) if bots && ai => m::change::block_both(host),
        (Verb::Change, RuleKind::Block) if ai => m::change::block_ai(host),
        (Verb::Change, RuleKind::Block) => m::change::block(host),
        (Verb::Change, RuleKind::Challenge) => m::change::challenge(host),
        (Verb::Change, RuleKind::RateLimit) if challenge => {
            m::change::rate_limit_challenge(host, requests, seconds)
        }
        (Verb::Change, RuleKind::RateLimit) => m::change::rate_limit_block(host, requests, seconds),
        (Verb::Change, RuleKind::RequestHeaders) => m::change::request_headers(headers, host),
        (Verb::Change, RuleKind::ResponseHeaders) => m::change::response_headers(headers, host),
    }
}

/// A rule in a phase, and whether Teitunnel owns it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservedRule {
    /// Rule id.
    pub id: String,
    /// Its definition.
    pub rule: NewRule,
    /// Created by Teitunnel (marker or ownership index).
    pub owned: bool,
}

/// A phase's entry point ruleset as observed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservedRuleset {
    /// The phase.
    pub phase: String,
    /// The entry point's id (`None`: the zone has none yet).
    pub id: Option<String>,
    /// Its rules, in order.
    pub rules: Vec<ObservedRule>,
}

/// A zone's edge rules as observed for a change.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EdgeState {
    /// Zone id.
    pub zone_id: String,
    /// Zone name.
    pub zone: String,
    /// Its plan.
    pub plan: ZonePlan,
    /// Every phase Teitunnel writes to or counts.
    pub rulesets: Vec<ObservedRuleset>,
}

impl EdgeState {
    /// A phase's ruleset.
    pub fn ruleset(&self, phase: &str) -> Option<&ObservedRuleset> {
        self.rulesets.iter().find(|r| r.phase == phase)
    }

    /// Rules counted towards `quota` now.
    pub fn used(&self, quota: QuotaKind) -> u32 {
        quota
            .phases()
            .iter()
            .filter_map(|p| self.ruleset(p))
            .map(|r| u32::try_from(r.rules.len()).unwrap_or(u32::MAX))
            .sum()
    }

    /// Teitunnel's rule with `description`, and its 1-based position.
    pub fn owned_rule(&self, phase: &str, description: &str) -> Option<(u32, &ObservedRule)> {
        self.ruleset(phase)?
            .rules
            .iter()
            .zip(1u32..)
            .find(|(r, _)| r.owned && r.rule.description == description)
            .map(|(r, i)| (i, r))
    }

    /// Teitunnel's shared rate limits: position, rule, limit and hostnames.
    pub fn rate_limits(&self) -> Vec<(u32, &ObservedRule, RateLimitSpec, BTreeSet<String>)> {
        self.ruleset(cf_api::PHASE_RATE_LIMIT)
            .into_iter()
            .flat_map(|r| r.rules.iter().zip(1u32..))
            .filter(|(r, _)| r.owned)
            .filter_map(|(r, i)| {
                let spec = rate_limit_spec(&r.rule)?;
                Some((i, r, spec, hosts_of(&r.rule.expression)))
            })
            .collect()
    }

    /// What Teitunnel enforces for `hostname` now, read back from its rules.
    pub fn protection_of(&self, hostname: &Hostname) -> EdgeProtection {
        let mut out = EdgeProtection::default();
        if let Some((_, rule)) =
            self.owned_rule(cf_api::PHASE_CUSTOM, &marker(hostname, RuleKind::Block))
        {
            if rule.rule.expression.contains(BOTS_EXPRESSION) {
                out.bots = BotMode::Block;
            }
            out.ai_crawlers = rule.rule.expression.contains(AI_EXPRESSION);
        }
        if out.bots == BotMode::Off
            && self
                .owned_rule(cf_api::PHASE_CUSTOM, &marker(hostname, RuleKind::Challenge))
                .is_some()
        {
            out.bots = BotMode::Challenge;
        }
        out.rate_limit = self
            .rate_limits()
            .into_iter()
            .find(|(_, _, _, hosts)| hosts.contains(hostname.as_str()))
            .map(|(_, _, spec, _)| spec);
        for (kind, list) in [
            (RuleKind::RequestHeaders, &mut out.request_headers),
            (RuleKind::ResponseHeaders, &mut out.response_headers),
        ] {
            if let Some((_, rule)) = self.owned_rule(kind.phase(), &marker(hostname, kind)) {
                *list = headers_of(rule.rule.action_parameters.as_ref());
            }
        }
        out
    }
}

/// What a change reads about edge rules.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EdgeNeed {
    /// The hostname whose zone's rules to read.
    pub hostname: Option<String>,
    /// The plan can't be made without them. Otherwise (removing a route) they're read
    /// only when Teitunnel owns rules in the zone, and not being allowed is no error.
    pub required: bool,
}

/// Why edge rules couldn't be read.
#[derive(Debug)]
pub(crate) enum EdgeReadError {
    /// The credential can't read them.
    Permission,
    /// Anything else.
    Api(cf_api::Error),
}

/// Every phase Teitunnel reads for a zone.
pub(crate) const PHASES: [&str; 5] = [
    cf_api::PHASE_CUSTOM,
    cf_api::PHASE_RATE_LIMIT,
    cf_api::PHASE_REQUEST_HEADERS,
    cf_api::PHASE_RESPONSE_HEADERS,
    cf_api::PHASE_URL_REWRITE,
];

/// Reads the plan and the rules of `zone`; `owned` holds the rule ids in the local
/// ownership index.
pub(crate) async fn observe<C: CloudApi>(
    api: &C,
    zone: &super::types::ZoneRef,
    owned: &HashSet<String>,
) -> Result<EdgeState, EdgeReadError> {
    let classify = |err: cf_api::Error| {
        if err.is_auth() {
            EdgeReadError::Permission
        } else {
            EdgeReadError::Api(err)
        }
    };
    let plan = api.zone_plan(&zone.id);
    let reads = futures_util::future::try_join_all(
        PHASES
            .iter()
            .map(|phase| async move { api.phase_entrypoint(&zone.id, phase).await }),
    );
    let (plan, rulesets) = tokio::try_join!(plan, reads).map_err(classify)?;
    let rulesets = PHASES
        .iter()
        .zip(rulesets)
        .map(|(phase, found)| ObservedRuleset {
            phase: (*phase).to_owned(),
            id: found.as_ref().map(|r| r.id.clone()),
            rules: found
                .map(|r| r.rules)
                .unwrap_or_default()
                .into_iter()
                .map(|rule| ObservedRule {
                    owned: owned.contains(&rule.id) || rule.description.starts_with(RULE_MARKER),
                    rule: rule.to_new(),
                    id: rule.id,
                })
                .collect(),
        })
        .collect();
    Ok(EdgeState {
        zone_id: zone.id.clone(),
        zone: zone.name.clone(),
        plan: ZonePlan::from_legacy_id(plan.as_deref()),
        rulesets,
    })
}

/// A service token in the account, without its secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedServiceToken {
    /// Token id.
    pub id: String,
    /// Name.
    pub name: String,
    /// The `CF-Access-Client-Id` value.
    pub client_id: String,
    /// When it stops working (RFC 3339).
    pub expires_at: Option<String>,
    /// Teitunnel created it (ownership index).
    pub owned: bool,
    /// The hostname Teitunnel made it for (its tokens go with the hostname's last route).
    pub made_for: Option<String>,
}

/// A service token's credentials right after it was created or rotated: the only time
/// the secret exists outside Cloudflare. Never stored, logged or serialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuedToken {
    /// Token id.
    pub token_id: String,
    /// Its name.
    pub name: String,
    /// The `CF-Access-Client-Id` value.
    pub client_id: String,
    /// The `CF-Access-Client-Secret` value.
    pub client_secret: crate::Secret<String>,
    /// When it stops working (RFC 3339).
    pub expires_at: Option<String>,
}

impl From<cf_api::IssuedServiceToken> for IssuedToken {
    fn from(issued: cf_api::IssuedServiceToken) -> Self {
        Self {
            token_id: issued.token.id,
            name: issued.token.name,
            client_id: issued.token.client_id,
            client_secret: crate::Secret::new(issued.client_secret),
            expires_at: issued.token.expires_at,
        }
    }
}

/// Service tokens per account (Cloudflare One limits, 2026-09-24).
pub const MAX_SERVICE_TOKENS: usize = 50;

/// How long a new service token lasts.
pub const SERVICE_TOKEN_DURATION: &str = "8760h";

/// The name Teitunnel gives a service token.
pub fn service_token_name(hostname: &Hostname, label: &str) -> String {
    format!("{}{hostname} · {}", cf_api::TEITUNNEL_PREFIX, label.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(name: &str) -> Hostname {
        Hostname::parse(name).unwrap()
    }

    #[test]
    fn rules_are_scoped_to_the_hostname() {
        let protection = EdgeProtection {
            bots: BotMode::Block,
            ai_crawlers: true,
            request_headers: vec![HeaderRule {
                name: "X-Env".into(),
                op: EdgeHeaderOp::Set,
                value: Some("preview".into()),
            }],
            ..EdgeProtection::default()
        };
        let rules = hostname_rules(&host("app.xyz.com"), &protection);
        assert_eq!(rules.len(), 2);
        for (_, rule) in &rules {
            assert!(
                rule.expression
                    .starts_with(r#"(http.host eq "app.xyz.com")"#),
                "{}",
                rule.expression
            );
            assert!(rule.description.starts_with("teitunnel:"));
        }
        assert!(rules[0].1.expression.contains(" or "));
        assert_eq!(
            rules[1].1.action_parameters,
            Some(json!({"headers": {"X-Env": {"operation": "set", "value": "preview"}}}))
        );
        assert!(
            hostname_rules(&host("app.xyz.com"), &EdgeProtection::default()).is_empty(),
            "off means no rules"
        );
    }

    #[test]
    fn rate_limits_round_trip_their_hostnames() {
        let hosts: BTreeSet<String> = ["b.xyz.com", "a.xyz.com"].map(String::from).into();
        let spec = RateLimitSpec {
            requests: 30,
            period: 60,
            action: LimitAction::Challenge,
        };
        let rule = rate_limit_rule(&spec, &hosts);
        assert_eq!(
            rule.expression,
            r#"(http.host in {"a.xyz.com" "b.xyz.com"})"#
        );
        assert_eq!(hosts_of(&rule.expression), hosts);
        assert_eq!(rate_limit_spec(&rule), Some(spec));
        assert_eq!(
            rule.ratelimit.unwrap().characteristics,
            ["cf.colo.id", "ip.src"]
        );
    }

    #[test]
    fn settings_are_checked() {
        let header = |name: &str, op: EdgeHeaderOp, value: Option<&str>| HeaderRule {
            name: name.into(),
            op,
            value: value.map(str::to_owned),
        };
        let request = |rules: Vec<HeaderRule>| EdgeProtection {
            request_headers: rules,
            ..EdgeProtection::default()
        };
        let err = |p: EdgeProtection| p.normalized().unwrap_err();
        assert_eq!(
            err(request(vec![header(
                "CF-Ray",
                EdgeHeaderOp::Set,
                Some("x")
            )])),
            EdgeInputError::ProtectedHeader("CF-Ray".into())
        );
        assert!(
            request(vec![header("cf-connecting-ip", EdgeHeaderOp::Remove, None)])
                .normalized()
                .is_ok()
        );
        assert_eq!(
            err(request(vec![header(
                "Cookie",
                EdgeHeaderOp::Set,
                Some("a=b")
            )])),
            EdgeInputError::CookieRemoveOnly
        );
        assert_eq!(
            err(request(vec![header("X-A", EdgeHeaderOp::Add, Some("1"))])),
            EdgeInputError::AddOnRequest("X-A".into())
        );
        assert_eq!(
            err(request(vec![header(
                "Bad Name",
                EdgeHeaderOp::Remove,
                None
            )])),
            EdgeInputError::HeaderName("Bad Name".into())
        );
        assert_eq!(
            err(request(vec![
                header("X-A", EdgeHeaderOp::Remove, None),
                header("x-a", EdgeHeaderOp::Remove, None)
            ])),
            EdgeInputError::DuplicateHeader("x-a".into())
        );
        assert_eq!(
            err(request(vec![header("X-A", EdgeHeaderOp::Set, Some(" "))])),
            EdgeInputError::MissingValue("X-A".into())
        );
        let limit = |requests, period| EdgeProtection {
            rate_limit: Some(RateLimitSpec {
                requests,
                period,
                action: LimitAction::Block,
            }),
            ..EdgeProtection::default()
        };
        assert_eq!(err(limit(0, 60)), EdgeInputError::Requests);
        assert_eq!(err(limit(10, 30)), EdgeInputError::Period(30));
        let sorted = EdgeProtection {
            response_headers: vec![
                header("X-Z", EdgeHeaderOp::Remove, None),
                header(" X-A ", EdgeHeaderOp::Add, Some("1")),
            ],
            ..EdgeProtection::default()
        }
        .normalized()
        .unwrap();
        assert_eq!(sorted.response_headers[0].name, "X-A");
    }

    #[test]
    fn unknown_plans_get_the_smallest_limits() {
        assert_eq!(ZonePlan::from_legacy_id(Some("pro")), ZonePlan::Pro);
        assert_eq!(ZonePlan::from_legacy_id(Some("mystery")), ZonePlan::Free);
        assert!(!ZonePlan::Free.limits().host_rate_limit);
        assert_eq!(QuotaKind::RateLimit.limit(ZonePlan::Free), 1);
    }
}
