//! `teitunnel serve`: this machine's tunnels plus a web dashboard and JSON API, for
//! servers without the app. It runs the connectors like `up`, and every
//! change goes through the same plan → apply engine (preview, then apply the reviewed
//! plan by its fingerprint).
//!
//! Security: it listens on loopback unless `--allow-remote`; signing in needs the
//! password (argon2id, `serve --set-password`) and is rate-limited per address; a
//! session is a random 256-bit cookie (`HttpOnly`, `SameSite=Strict`, `Secure` with
//! `--secure-cookies`) kept in memory; every change also needs the `X-Teitunnel` header
//! (a cross-site form can't send it, and no CORS headers are ever sent); automation uses
//! API keys (`Authorization: Bearer ttk_…`, stored as hashes). No secret is ever returned.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    process::ExitCode,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};
use teitunnel_core::{
    domain_shares::DomainShare,
    engine::{Approval, Change, Context, RoutesOverview},
    machine::MachineTunnels,
    web_auth,
};

use crate::context::App;

/// How long a session lasts without signing in again.
const SESSION_TTL: Duration = Duration::from_secs(12 * 3600);
/// Failed sign-ins allowed per address in [`FAILURE_WINDOW`].
const MAX_FAILURES: u32 = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(300);
/// The header every cookie-authenticated change must carry.
const CSRF_HEADER: &str = "x-teitunnel";
const COOKIE: &str = "teitunnel_session";

const INDEX: &str = include_str!("serve/index.html");
const SCRIPT: &str = include_str!("serve/app.js");
const STYLE: &str = include_str!("serve/app.css");

/// Options for `serve`.
#[derive(Debug, Clone)]
pub(crate) struct Options {
    pub(crate) listen: SocketAddr,
    pub(crate) allow_remote: bool,
    pub(crate) secure_cookies: bool,
    /// The MCP endpoint at `/mcp`, unless turned off.
    pub(crate) mcp: Option<McpOptions>,
}

/// Options for the MCP endpoint.
#[derive(Debug, Clone)]
pub(crate) struct McpOptions {
    /// Its mode (default: the settings file's, else ask).
    pub(crate) mode: Option<teitunnel_mcp::Mode>,
    /// Browser origins allowed to call it.
    pub(crate) allowed_origins: Vec<String>,
}

struct Server {
    app: App,
    machine: MachineTunnels,
    /// This process's inspector (shares started over `/mcp`, and the history).
    inspector: teitunnel_core::inspect::Inspector,
    analytics: teitunnel_core::analytics::Analytics,
    monitor: teitunnel_core::uptime::Monitor,
    secure_cookies: bool,
    sessions: Mutex<HashMap<String, Instant>>,
    failures: Limiter,
}

type Shared = Arc<Server>;

/// Failed sign-ins per address: after [`MAX_FAILURES`] within [`FAILURE_WINDOW`] of the
/// first, that address waits until the window ends.
#[derive(Default)]
struct Limiter(Mutex<HashMap<IpAddr, (u32, Instant)>>);

impl Limiter {
    fn blocked(&self, ip: IpAddr) -> bool {
        let mut failures = locked(&self.0);
        failures.retain(|_, (_, first)| first.elapsed() < FAILURE_WINDOW);
        failures
            .get(&ip)
            .is_some_and(|(count, _)| *count >= MAX_FAILURES)
    }

    fn failed(&self, ip: IpAddr) {
        let mut failures = locked(&self.0);
        failures.entry(ip).or_insert((0, Instant::now())).0 += 1;
    }
}

/// An error as JSON: `{ "error": "…" }`.
struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(serde_json::json!({ "error": self.1 }))).into_response()
    }
}

// By value so it can be passed to `map_err` directly.
#[allow(clippy::needless_pass_by_value)]
fn bad(message: impl ToString) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, message.to_string())
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            let (key, value) = pair.trim().split_once('=')?;
            (key == name).then(|| value.to_owned())
        })
}

impl Server {
    fn session_valid(&self, headers: &HeaderMap) -> bool {
        let Some(token) = cookie(headers, COOKIE) else {
            return false;
        };
        let mut sessions = locked(&self.sessions);
        sessions.retain(|_, started| started.elapsed() < SESSION_TTL);
        sessions.contains_key(&token)
    }

    /// Who's asking: a signed-in browser (changes need the CSRF header too) or an API
    /// key.
    async fn authorize(&self, headers: &HeaderMap, change: bool) -> Result<(), ApiError> {
        if let Some(key) = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
        {
            return match web_auth::verify_api_key(self.app.store(), key.trim()).await {
                Ok(Some(_)) => Ok(()),
                _ => Err(ApiError(
                    StatusCode::UNAUTHORIZED,
                    "Invalid API key.".into(),
                )),
            };
        }
        if !self.session_valid(headers) {
            return Err(ApiError(StatusCode::UNAUTHORIZED, "Sign in first.".into()));
        }
        if change && headers.get(CSRF_HEADER).is_none() {
            return Err(ApiError(
                StatusCode::FORBIDDEN,
                "Missing the X-Teitunnel header.".into(),
            ));
        }
        Ok(())
    }
}

/// The JSON of `value`, with every message (`{ "key": "core.…", "args": … }`) rendered
/// as English text: the dashboard has no catalogs, and the API is simpler with text.
fn english<T: Serialize>(value: &T) -> Json<serde_json::Value> {
    fn walk(value: serde_json::Value) -> serde_json::Value {
        use serde_json::Value;
        match value {
            Value::Object(map)
                if map.len() == 2
                    && map
                        .get("key")
                        .and_then(Value::as_str)
                        .is_some_and(|k| k.starts_with("core."))
                    && map.contains_key("args") =>
            {
                match serde_json::from_value::<teitunnel_core::text::Text>(Value::Object(
                    map.clone(),
                )) {
                    Ok(text) => Value::String(text.english()),
                    Err(_) => Value::Object(map),
                }
            }
            Value::Object(map) => {
                Value::Object(map.into_iter().map(|(k, v)| (k, walk(v))).collect())
            }
            Value::Array(items) => Value::Array(items.into_iter().map(walk).collect()),
            other => other,
        }
    }
    Json(walk(serde_json::to_value(value).unwrap_or_default()))
}

/// Security headers on every response.
fn hardened(mut response: Response) -> Response {
    let headers = response.headers_mut();
    for (name, value) in [
        (
            "content-security-policy",
            "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "no-referrer"),
        ("x-frame-options", "DENY"),
        ("cache-control", "no-store"),
    ] {
        headers.insert(name, HeaderValue::from_static(value));
    }
    response
}

fn asset(content_type: &'static str, body: &'static str) -> Response {
    hardened(([(header::CONTENT_TYPE, content_type)], body).into_response())
}

async fn index() -> Response {
    asset("text/html; charset=utf-8", INDEX)
}
async fn script() -> Response {
    asset("text/javascript; charset=utf-8", SCRIPT)
}
async fn style() -> Response {
    asset("text/css; charset=utf-8", STYLE)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionView {
    signed_in: bool,
}

async fn session(State(server): State<Shared>, headers: HeaderMap) -> Response {
    hardened(
        Json(SessionView {
            signed_in: server.session_valid(&headers),
        })
        .into_response(),
    )
}

#[derive(Deserialize)]
struct Login {
    password: String,
}

async fn login(
    State(server): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(body): Json<Login>,
) -> Result<Response, ApiError> {
    if headers.get(CSRF_HEADER).is_none() {
        return Err(ApiError(
            StatusCode::FORBIDDEN,
            "Missing the X-Teitunnel header.".into(),
        ));
    }
    if server.failures.blocked(peer.ip()) {
        return Err(ApiError(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts. Try again in a few minutes.".into(),
        ));
    }
    let ok = web_auth::verify_password(server.app.store(), &body.password)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    if !ok {
        server.failures.failed(peer.ip());
        return Err(ApiError(
            StatusCode::UNAUTHORIZED,
            "That password isn't right.".into(),
        ));
    }
    let token = web_auth::random_token()
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    locked(&server.sessions).insert(token.clone(), Instant::now());
    let secure = if server.secure_cookies {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{secure}",
        SESSION_TTL.as_secs()
    );
    let mut response = Json(SessionView { signed_in: true }).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&cookie).map_err(bad)?,
    );
    Ok(hardened(response))
}

async fn logout(State(server): State<Shared>, headers: HeaderMap) -> Response {
    if let Some(token) = cookie(&headers, COOKIE) {
        locked(&server.sessions).remove(&token);
    }
    let mut response = Json(SessionView { signed_in: false }).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "teitunnel_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        ),
    );
    hardened(response)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AccountOverview {
    id: String,
    name: String,
    overview: Option<RoutesOverview>,
    error: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct OverviewView {
    machine: String,
    accounts: Vec<AccountOverview>,
    shares: Vec<DomainShare>,
}

/// Every account's tunnels and routes on this machine, with their status.
async fn overview(State(server): State<Shared>, headers: HeaderMap) -> Result<Response, ApiError> {
    server.authorize(&headers, false).await?;
    let app = &server.app;
    let mut accounts = Vec::new();
    for account in app.accounts.list().await.map_err(bad)? {
        let result = async {
            let api = app
                .accounts
                .client(&account.id)
                .await
                .map_err(|e| e.to_string())?;
            app.engine
                .overview(&api, &server.machine, app.context(&account))
                .await
                .map_err(|e| e.to_string())
        }
        .await;
        let (overview, error) = match result {
            Ok(overview) => (Some(overview), None),
            Err(error) => (None, Some(error)),
        };
        accounts.push(AccountOverview {
            id: account.id,
            name: account.name,
            overview,
            error,
        });
    }
    let shares = app.engine.local().shares(None).await.map_err(bad)?;
    Ok(hardened(
        english(&OverviewView {
            machine: app.machine_name.clone(),
            accounts,
            shares,
        })
        .into_response(),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PreviewBody {
    account_id: String,
    tunnel_id: Option<String>,
    change: Change,
}

/// Plans a change for review; nothing changes.
async fn preview(
    State(server): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<PreviewBody>,
) -> Result<Response, ApiError> {
    server.authorize(&headers, true).await?;
    let app = &server.app;
    let account = app.account(Some(&body.account_id)).await.map_err(bad)?;
    let api = app.accounts.client(&account.id).await.map_err(bad)?;
    let ctx = Context {
        tunnel: body.tunnel_id.as_deref(),
        ..app.context(&account)
    };
    let intent = app
        .engine
        .intent_for(&api, ctx, &body.change)
        .await
        .map_err(bad)?;
    let plan = app.engine.preview(&api, ctx, &intent).await.map_err(bad)?;
    Ok(hardened(english(&plan.view(&account.id)).into_response()))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApplyBody {
    account_id: String,
    tunnel_id: Option<String>,
    change: Change,
    fingerprint: String,
    #[serde(default)]
    confirmed: bool,
}

/// Applies a reviewed plan (409 if anything changed since the preview: review again).
async fn apply(
    State(server): State<Shared>,
    headers: HeaderMap,
    Json(body): Json<ApplyBody>,
) -> Result<Response, ApiError> {
    server.authorize(&headers, true).await?;
    let app = &server.app;
    let account = app.account(Some(&body.account_id)).await.map_err(bad)?;
    let api = app.accounts.client(&account.id).await.map_err(bad)?;
    let ctx = Context {
        tunnel: body.tunnel_id.as_deref(),
        ..app.context(&account)
    };
    let intent = app
        .engine
        .intent_for(&api, ctx, &body.change)
        .await
        .map_err(bad)?;
    let approval = Approval {
        fingerprint: &body.fingerprint,
        confirmed: body.confirmed,
    };
    match app
        .engine
        .apply(&api, &server.machine, ctx, &intent, approval, |_| {})
        .await
    {
        Ok(outcome) => Ok(hardened(english(&outcome).into_response())),
        Err(teitunnel_core::engine::EngineError::Stale(_)) => Err(ApiError(
            StatusCode::CONFLICT,
            "Something changed in Cloudflare since this was reviewed. Review it again.".into(),
        )),
        Err(err) => Err(bad(err)),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnalyticsQuery {
    account_id: String,
    /// One route in detail; every route of this machine in the account without it.
    hostname: Option<String>,
    path: Option<String>,
    range: Option<String>,
}

fn range_of(value: Option<&str>) -> Result<teitunnel_core::analytics::AnalyticsRange, ApiError> {
    value.map_or(Ok(teitunnel_core::analytics::AnalyticsRange::Day), |r| {
        teitunnel_core::analytics::AnalyticsRange::parse(r)
            .ok_or_else(|| bad("range must be hour, day, week or month"))
    })
}

fn analytics_error(err: &teitunnel_core::analytics::AnalyticsError) -> ApiError {
    use teitunnel_core::analytics::AnalyticsError as E;
    let status = match err {
        E::Permission => StatusCode::FORBIDDEN,
        E::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        E::NoZone(_) => StatusCode::NOT_FOUND,
        _ => StatusCode::BAD_GATEWAY,
    };
    ApiError(status, err.to_string())
}

fn now_ms() -> i64 {
    i64::try_from(teitunnel_core::domain_shares::now_ms()).unwrap_or(i64::MAX)
}

/// Edge analytics: one route (`hostname`, optional `path`) or all of the account's
/// routes on this machine.
async fn analytics(
    State(server): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<AnalyticsQuery>,
) -> Result<Response, ApiError> {
    use teitunnel_core::analytics::{RouteRef, path_prefix};
    server.authorize(&headers, false).await?;
    let range = range_of(query.range.as_deref())?;
    let app = &server.app;
    let account = app.account(Some(&query.account_id)).await.map_err(bad)?;
    if let Some(hostname) = query.hostname {
        let hostname = teitunnel_core::domain::Hostname::parse(&hostname).map_err(bad)?;
        let route = RouteRef {
            hostname: hostname.as_str().to_owned(),
            path: query.path.as_deref().and_then(path_prefix),
        };
        let stats = server
            .analytics
            .route(&app.accounts, &account.id, &route, range)
            .await
            .map_err(|e| analytics_error(&e))?;
        return Ok(hardened(english(&stats).into_response()));
    }
    let mut hosts: Vec<String> = teitunnel_core::uptime::targets(&app.accounts, app.engine.local())
        .await
        .into_iter()
        .filter(|t| t.account_id == account.id)
        .map(|t| t.route.hostname)
        .collect();
    hosts.sort();
    hosts.dedup();
    let summary = server
        .analytics
        .summary(&app.accounts, &account.id, &hosts, range)
        .await
        .map_err(|e| analytics_error(&e))?;
    Ok(hardened(english(&summary).into_response()))
}

#[derive(Deserialize)]
struct UptimeQuery {
    hostname: Option<String>,
    path: Option<String>,
    range: Option<String>,
}

/// Uptime of every route on this machine, or one route in detail (`hostname`).
async fn uptime(
    State(server): State<Shared>,
    headers: HeaderMap,
    Query(query): Query<UptimeQuery>,
) -> Result<Response, ApiError> {
    use teitunnel_core::analytics::{RouteRef, path_prefix};
    server.authorize(&headers, false).await?;
    let Some(hostname) = query.hostname else {
        let list = server.monitor.summaries(now_ms()).await.map_err(bad)?;
        return Ok(hardened(english(&list).into_response()));
    };
    let range = range_of(query.range.as_deref())?;
    let route = RouteRef {
        hostname: hostname.trim().to_ascii_lowercase(),
        path: query
            .path
            .as_deref()
            .and_then(path_prefix)
            .filter(|p| p != "/"),
    };
    match server
        .monitor
        .detail(&route, range, now_ms())
        .await
        .map_err(bad)?
    {
        Some(detail) => Ok(hardened(english(&detail).into_response())),
        None => Err(ApiError(
            StatusCode::NOT_FOUND,
            format!("This machine doesn't serve {}.", route.key()),
        )),
    }
}

/// Filters of `GET /api/traffic`.
#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TrafficParams {
    tap: Option<String>,
    host: Option<String>,
    method: Option<String>,
    status: Option<String>,
    path: Option<String>,
    text: Option<String>,
    limit: Option<u32>,
    before: Option<String>,
}

impl TrafficParams {
    fn query(self) -> Result<teitunnel_core::inspect::ExchangeQuery, ApiError> {
        use teitunnel_core::inspect::lens::{ExchangeId, TapId};
        let mut query = teitunnel_core::inspect::ExchangeQuery {
            tap: self
                .tap
                .as_deref()
                .map(TapId::new)
                .transpose()
                .map_err(bad)?,
            methods: self.method.iter().map(|m| m.to_ascii_uppercase()).collect(),
            path: self.path,
            host: self.host,
            text: self.text,
            limit: Some(self.limit.unwrap_or(100).clamp(1, 1_000)),
            before: self
                .before
                .as_deref()
                .map(str::parse::<ExchangeId>)
                .transpose()
                .map_err(bad)?,
            ..teitunnel_core::inspect::ExchangeQuery::default()
        };
        if let Some(status) = self.status {
            let lower = status.trim().to_ascii_lowercase();
            match lower.strip_suffix("xx") {
                Some(class) => query
                    .status_classes
                    .push(class.parse().map_err(|_| bad("status: try 404 or 5xx"))?),
                None => query
                    .statuses
                    .push(lower.parse().map_err(|_| bad("status: try 404 or 5xx"))?),
            }
        }
        Ok(query)
    }
}

/// Requests captured by the inspector, newest first (credentials masked).
async fn traffic(
    State(server): State<Shared>,
    headers: HeaderMap,
    Query(params): Query<TrafficParams>,
) -> Result<Response, ApiError> {
    server.authorize(&headers, false).await?;
    let page = server.inspector.list(&params.query()?);
    Ok(hardened(Json(page).into_response()))
}

/// One captured request in full (credentials masked; the API never reveals them).
async fn traffic_exchange(
    State(server): State<Shared>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Response, ApiError> {
    server.authorize(&headers, false).await?;
    let id = id
        .parse::<teitunnel_core::inspect::lens::ExchangeId>()
        .map_err(bad)?;
    let detail = server
        .inspector
        .detail(id, false)
        .await
        .map_err(|e| ApiError(StatusCode::NOT_FOUND, e.to_string()))?;
    Ok(hardened(Json(detail).into_response()))
}

/// The inspector's taps: running here, and known from the history.
async fn traffic_taps(
    State(server): State<Shared>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    server.authorize(&headers, false).await?;
    Ok(hardened(
        Json(serde_json::json!({
            "running": server.inspector.taps(),
            "known": server.inspector.known_taps(),
        }))
        .into_response(),
    ))
}

/// The API described for automation (OpenAPI 3.1).
async fn openapi() -> Response {
    hardened(Json(openapi_document()).into_response())
}

fn openapi_document() -> serde_json::Value {
    let json = |schema: serde_json::Value| serde_json::json!({ "content": { "application/json": { "schema": schema } } });
    serde_json::json!({
        "openapi": "3.1.0",
        "info": {
            "title": "Teitunnel server API",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "Manage this machine's Cloudflare Tunnel routes. Authenticate with `Authorization: Bearer ttk_…` (create a key with `teitunnel api-key create NAME`). Changes are two steps: `POST /api/preview` returns a plan with a fingerprint; `POST /api/apply` applies exactly that plan, or answers 409 if Cloudflare changed meanwhile."
        },
        "components": {
            "securitySchemes": { "apiKey": { "type": "http", "scheme": "bearer" } },
            "schemas": {
                "Change": {
                    "description": "What to change, e.g. {\"type\":\"addRoute\",\"route\":{\"hostname\":\"app.teispace.com\",\"origin\":\"3000\",\"path\":null,\"access\":null}}, {\"type\":\"removeRoute\",\"hostname\":\"app.teispace.com\",\"path\":null}, {\"type\":\"createTunnel\",\"name\":\"staging\"}, {\"type\":\"removeTunnel\"}.",
                    "type": "object",
                    "required": ["type"]
                },
                "Error": { "type": "object", "properties": { "error": { "type": "string" } } }
            }
        },
        "security": [{ "apiKey": [] }],
        "paths": {
            "/api/overview": { "get": {
                "summary": "Accounts, this machine's tunnels and routes with their status, and shares on your domains",
                "responses": { "200": json(serde_json::json!({ "type": "object" })) }
            }},
            "/api/preview": { "post": {
                "summary": "Plan a change (nothing changes)",
                "requestBody": json(serde_json::json!({
                    "type": "object",
                    "required": ["accountId", "change"],
                    "properties": {
                        "accountId": { "type": "string" },
                        "tunnelId": { "type": ["string", "null"], "description": "One of this machine's tunnels; default: the default tunnel" },
                        "change": { "$ref": "#/components/schemas/Change" }
                    }
                })),
                "responses": {
                    "200": json(serde_json::json!({ "type": "object", "description": "The plan: steps, warnings, requiresConfirmation, fingerprint" })),
                    "400": json(serde_json::json!({ "$ref": "#/components/schemas/Error" }))
                }
            }},
            "/api/analytics": { "get": {
                "summary": "Traffic from Cloudflare's edge: one route in detail, or every route of this machine in the account (needs Zone Analytics Read on the token)",
                "parameters": [
                    { "name": "accountId", "in": "query", "required": true, "schema": { "type": "string" } },
                    { "name": "hostname", "in": "query", "schema": { "type": "string" }, "description": "One route; all of this machine's without it" },
                    { "name": "path", "in": "query", "schema": { "type": "string" }, "description": "The route's path rule, e.g. ^/api" },
                    { "name": "range", "in": "query", "schema": { "type": "string", "enum": ["hour", "day", "week", "month"], "default": "day" } }
                ],
                "responses": {
                    "200": json(serde_json::json!({ "type": "object", "description": "With hostname: requests, bytes, classes, statuses, paths, countries, browsers, bots, cache, originMs, series. Without: hosts with requests, errorRate, p95Ms, spark" })),
                    "403": json(serde_json::json!({ "$ref": "#/components/schemas/Error" })),
                    "429": json(serde_json::json!({ "$ref": "#/components/schemas/Error" }))
                }
            }},
            "/api/uptime": { "get": {
                "summary": "Uptime of every route on this machine (24 h, 7 d, 30 d, P95, open incident), or one route in detail with its status strip, response times and incidents",
                "parameters": [
                    { "name": "hostname", "in": "query", "schema": { "type": "string" } },
                    { "name": "path", "in": "query", "schema": { "type": "string" } },
                    { "name": "range", "in": "query", "schema": { "type": "string", "enum": ["hour", "day", "week", "month"], "default": "day" } }
                ],
                "responses": {
                    "200": json(serde_json::json!({ "type": ["array", "object"] })),
                    "404": json(serde_json::json!({ "$ref": "#/components/schemas/Error" }))
                }
            }},
            "/api/traffic": { "get": {
                "summary": "Requests captured by Teitunnel's inspector (shares and inspected routes run by this server, and the recent history of this machine), newest first. Credentials are always masked.",
                "parameters": [
                    { "name": "tap", "in": "query", "schema": { "type": "string" }, "description": "Only this tap (see /api/traffic/taps)" },
                    { "name": "host", "in": "query", "schema": { "type": "string" } },
                    { "name": "method", "in": "query", "schema": { "type": "string" } },
                    { "name": "status", "in": "query", "schema": { "type": "string" }, "description": "An exact status (404) or a class (5xx)" },
                    { "name": "path", "in": "query", "schema": { "type": "string" }, "description": "Text in the path" },
                    { "name": "text", "in": "query", "schema": { "type": "string" }, "description": "Text anywhere (secrets can't be searched)" },
                    { "name": "limit", "in": "query", "schema": { "type": "integer", "minimum": 1, "maximum": 1000, "default": 100 } },
                    { "name": "before", "in": "query", "schema": { "type": "string" }, "description": "The previous page's `next`" }
                ],
                "responses": {
                    "200": json(serde_json::json!({ "type": "object", "description": "items (id, tap, method, host, path, status, durationMs, sizes, kind, state, webhook) and next" })),
                    "400": json(serde_json::json!({ "$ref": "#/components/schemas/Error" }))
                }
            }},
            "/api/traffic/taps": { "get": {
                "summary": "The inspector's taps: running here, and known from the history",
                "responses": { "200": json(serde_json::json!({ "type": "object" })) }
            }},
            "/api/traffic/{id}": { "get": {
                "summary": "One captured request and its response in full (headers, bodies, timings, webhook signature check), credentials masked",
                "parameters": [{ "name": "id", "in": "path", "required": true, "schema": { "type": "string" } }],
                "responses": {
                    "200": json(serde_json::json!({ "type": "object" })),
                    "404": json(serde_json::json!({ "$ref": "#/components/schemas/Error" }))
                }
            }},
            "/api/apply": { "post": {
                "summary": "Apply a reviewed plan",
                "requestBody": json(serde_json::json!({
                    "type": "object",
                    "required": ["accountId", "change", "fingerprint"],
                    "properties": {
                        "accountId": { "type": "string" },
                        "tunnelId": { "type": ["string", "null"] },
                        "change": { "$ref": "#/components/schemas/Change" },
                        "fingerprint": { "type": "string" },
                        "confirmed": { "type": "boolean", "description": "Allow replacing records Teitunnel didn't create, when the plan asks" }
                    }
                })),
                "responses": {
                    "200": json(serde_json::json!({ "type": "object", "description": "applied, rolledBack or partiallyApplied" })),
                    "409": json(serde_json::json!({ "$ref": "#/components/schemas/Error" }))
                }
            }}
        }
    })
}

fn router(server: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/app.js", get(script))
        .route("/app.css", get(style))
        .route("/api/session", get(session))
        .route("/api/login", post(login))
        .route("/api/logout", post(logout))
        .route("/api/overview", get(overview))
        .route("/api/preview", post(preview))
        .route("/api/apply", post(apply))
        .route("/api/analytics", get(analytics))
        .route("/api/uptime", get(uptime))
        .route("/api/traffic", get(traffic))
        .route("/api/traffic/taps", get(traffic_taps))
        .route("/api/traffic/{id}", get(traffic_exchange))
        .route("/api/openapi.json", get(openapi))
        .with_state(server)
}

/// Runs the connectors and the dashboard until interrupted.
pub(crate) async fn run(app: App, options: Options) -> Result<ExitCode, String> {
    if !options.listen.ip().is_loopback() && !options.allow_remote {
        return Err(format!(
            "{} isn't a loopback address. Pass --allow-remote to listen there, and put TLS in front (or publish it through Teitunnel with a login).",
            options.listen
        ));
    }
    if let Ok(password) = std::env::var("TEITUNNEL_WEB_PASSWORD") {
        web_auth::set_password(app.store(), &password)
            .await
            .map_err(|e| e.to_string())?;
    }
    if !web_auth::has_password(app.store())
        .await
        .map_err(|e| e.to_string())?
    {
        return Err(
            "Set a password first: `teitunnel serve --set-password` (or TEITUNNEL_WEB_PASSWORD)."
                .into(),
        );
    }
    let (machine, supervisor) = app.machine(false).await;
    for account in app.accounts.list().await.map_err(|e| e.to_string())? {
        if let Ok(api) = app.accounts.client(&account.id).await
            && let Err(message) = machine.resume(&api, &account.id).await
        {
            crate::share::status(&format!("{}: {}", account.name, message.english()));
        }
    }
    let sweeper = crate::inspect::sweep_left_behind(&app, machine.clone());
    let listener = tokio::net::TcpListener::bind(options.listen)
        .await
        .map_err(|e| format!("Couldn't listen on {}: {e}", options.listen))?;
    crate::share::status(&format!(
        "Teitunnel dashboard on http://{}. Press Ctrl-C to stop.",
        listener.local_addr().map_err(|e| e.to_string())?
    ));
    let analytics = teitunnel_core::analytics::Analytics::default();
    let monitor = crate::analytics::spawn_monitor(&app, analytics.clone());
    let stop_mcp = tokio_util::sync::CancellationToken::new();
    let inspector = crate::mcp::inspector(&app);
    if let Err(err) = inspector.load().await {
        crate::share::status(&format!("(Couldn't read the inspector's history: {err})"));
    }
    let route_host = crate::sharing::RouteHost::spawn(&app, machine.clone(), inspector.clone());
    let mut mcp_backend = None;
    let mcp = match &options.mcp {
        Some(mcp) => {
            let settings = crate::mcp::settings(app.dir(), mcp.mode, false)?;
            let backend = crate::mcp::backend(
                &app,
                machine.clone(),
                machine.clone(),
                supervisor.clone(),
                &inspector,
            );
            mcp_backend = Some(Arc::clone(&backend));
            let reservations = Arc::new(teitunnel_mcp::reservations::ReservationTools::new(
                Arc::clone(&backend),
            ));
            let server = teitunnel_mcp::McpServer::builder(Arc::clone(&backend), settings.clone())
                .via("mcp over HTTP")
                .traffic(Arc::new(teitunnel_mcp::InspectorTraffic::new(
                    inspector.clone(),
                    false,
                )))
                .provider(reservations)
                .provider(Arc::new(teitunnel_mcp::CommentsTools::new(Arc::clone(
                    &backend,
                ))))
                .provider(Arc::new(teitunnel_mcp::ExposeTools::new(
                    backend,
                    inspector.clone(),
                )))
                .provider(Arc::new(teitunnel_mcp::InspectionTools::new(
                    inspector.clone(),
                )))
                .build();
            let store = app.store().clone();
            let verify: teitunnel_mcp::http::KeyVerifier = Arc::new(move |key: String| {
                let store = store.clone();
                Box::pin(async move { web_auth::verify_api_key(&store, &key).await.ok().flatten() })
            });
            crate::share::status(&format!(
                "MCP endpoint for AI agents at /mcp ({} mode; authenticate with an API key).",
                settings.mode
            ));
            Some(teitunnel_mcp::http::router(
                server,
                verify,
                teitunnel_mcp::http::HttpOptions {
                    allowed_origins: mcp.allowed_origins.clone(),
                    any_host: options.allow_remote,
                    cancel: stop_mcp.clone(),
                },
            ))
        }
        None => None,
    };
    let server = Arc::new(Server {
        app,
        machine: machine.clone(),
        inspector: inspector.clone(),
        analytics,
        monitor: monitor.clone(),
        secure_cookies: options.secure_cookies,
        sessions: Mutex::default(),
        failures: Limiter::default(),
    });
    let kept = Arc::clone(&server);
    let routes = match mcp {
        Some(mcp) => router(server).merge(mcp),
        None => router(server),
    };
    let stopping = stop_mcp.clone();
    axum::serve(
        listener,
        routes.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        crate::share::interrupted().await;
        // End MCP sessions (their streams would keep the server open).
        stopping.cancel();
    })
    .await
    .map_err(|e| e.to_string())?;
    sweeper.abort();
    monitor.release().await;
    if let Some(backend) = mcp_backend {
        backend.stop_own_shares().await;
    }
    route_host.stop(&kept.app, &machine, &inspector).await;
    supervisor.stop_all().await;
    inspector.shutdown().await;
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_messages_as_english() {
        let value = serde_json::json!({
            "steps": [{ "description": { "key": "core.raw", "args": { "text": "Create tunnel" } } }],
            "other": { "key": "not-a-message", "args": {} }
        });
        let Json(out) = english(&value);
        assert_eq!(out["steps"][0]["description"], "Create tunnel");
        assert_eq!(out["other"]["key"], "not-a-message");
    }

    #[test]
    fn limits_failed_sign_ins_per_address() {
        let limiter = Limiter::default();
        let (a, b): (IpAddr, IpAddr) = ("192.0.2.1".parse().unwrap(), "192.0.2.2".parse().unwrap());
        for _ in 0..MAX_FAILURES {
            assert!(!limiter.blocked(a));
            limiter.failed(a);
        }
        assert!(limiter.blocked(a));
        assert!(!limiter.blocked(b), "other addresses aren't affected");
    }

    #[test]
    fn reads_the_session_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("a=1; teitunnel_session=abc; b=2"),
        );
        assert_eq!(cookie(&headers, COOKIE).as_deref(), Some("abc"));
        assert_eq!(cookie(&headers, "missing"), None);
    }

    #[test]
    fn documents_the_api() {
        let doc = openapi_document();
        assert_eq!(doc["openapi"], "3.1.0");
        for path in [
            "/api/overview",
            "/api/preview",
            "/api/apply",
            "/api/analytics",
            "/api/uptime",
            "/api/traffic",
            "/api/traffic/taps",
            "/api/traffic/{id}",
        ] {
            assert!(doc["paths"][path].is_object(), "{path}");
        }
    }

    #[test]
    fn traffic_filters_from_the_query_string() {
        let query = TrafficParams {
            method: Some("post".into()),
            status: Some("5xx".into()),
            limit: Some(5_000),
            ..TrafficParams::default()
        }
        .query()
        .ok()
        .unwrap();
        assert_eq!(query.methods, ["POST"]);
        assert_eq!(query.status_classes, [5]);
        assert_eq!(query.limit, Some(1_000));
        assert!(
            TrafficParams {
                status: Some("x".into()),
                ..TrafficParams::default()
            }
            .query()
            .is_err()
        );
    }

    #[test]
    fn the_page_loads_nothing_inline_or_remote() {
        // The CSP forbids inline and remote scripts and styles; the page mustn't need them.
        assert!(!INDEX.contains("<script>") && !INDEX.contains("style="));
        assert!(!INDEX.contains("http://") && !INDEX.contains("https://"));
    }
}
