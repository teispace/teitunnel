//! The Streamable HTTP transport, mounted by `teitunnel serve` at `/mcp`.
//!
//! Per the MCP spec's security guidance: every request needs an API key
//! (`Authorization: Bearer ttk_…`, the same keys as the server API); a request carrying
//! an `Origin` header is refused unless that origin is explicitly allowed (DNS
//! rebinding); the `Host` header must be a loopback name unless the server listens
//! remotely; no CORS headers are ever sent; repeated bad keys from one address are
//! refused for a while. Sessions follow the spec (an `Mcp-Session-Id` per client, for
//! protocol versions that have them).

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use axum::{
    Router,
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use tokio_util::sync::CancellationToken;

use crate::{McpServer, backend::BoxFuture, server::HttpIdentity};

/// Checks an API key; `Some(name)` when it's valid.
pub type KeyVerifier = Arc<dyn Fn(String) -> BoxFuture<'static, Option<String>> + Send + Sync>;

/// Failed keys allowed per address in [`FAILURE_WINDOW`].
const MAX_FAILURES: u32 = 10;
const FAILURE_WINDOW: Duration = Duration::from_secs(300);
/// Largest request body.
const MAX_BODY: usize = 1024 * 1024;

/// How the endpoint is exposed.
#[derive(Debug, Clone, Default)]
pub struct HttpOptions {
    /// Browser origins allowed to call it (`https://app.teispace.com`). Empty: requests
    /// with an `Origin` header are refused (agents don't send one).
    pub allowed_origins: Vec<String>,
    /// Accept any `Host` (the server listens on a public address behind a proxy).
    pub any_host: bool,
    /// Stops every session.
    pub cancel: CancellationToken,
}

#[derive(Clone)]
struct Guard {
    verify: KeyVerifier,
    origins: Arc<Vec<String>>,
    failures: Arc<Mutex<HashMap<IpAddr, (u32, Instant)>>>,
}

fn refuse(status: StatusCode, message: &str) -> Response {
    let mut response = (
        status,
        axum::Json(serde_json::json!({
            "jsonrpc": "2.0",
            "error": { "code": -32001, "message": message },
            "id": null
        })),
    )
        .into_response();
    if status == StatusCode::UNAUTHORIZED {
        response.headers_mut().insert(
            header::WWW_AUTHENTICATE,
            HeaderValue::from_static(r#"Bearer realm="teitunnel""#),
        );
    }
    response
}

fn origin_allowed(origin: &str, allowed: &[String]) -> bool {
    let origin = origin.trim().trim_end_matches('/');
    allowed
        .iter()
        .any(|a| a.trim().trim_end_matches('/').eq_ignore_ascii_case(origin))
}

async fn guard(State(guard): State<Guard>, mut request: Request, next: Next) -> Response {
    if let Some(origin) = request.headers().get(header::ORIGIN) {
        let origin = origin.to_str().unwrap_or_default();
        if !origin_allowed(origin, &guard.origins) {
            return refuse(
                StatusCode::FORBIDDEN,
                "This origin isn't allowed to use Teitunnel's MCP server.",
            );
        }
    }
    let ip = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0.ip());
    if let Some(ip) = ip {
        let mut failures = guard
            .failures
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        failures.retain(|_, (_, first)| first.elapsed() < FAILURE_WINDOW);
        if failures.get(&ip).is_some_and(|(n, _)| *n >= MAX_FAILURES) {
            return refuse(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many invalid API keys. Try again in a few minutes.",
            );
        }
    }
    let key = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|k| k.trim().to_owned());
    let Some(key) = key else {
        return refuse(
            StatusCode::UNAUTHORIZED,
            "An API key is required: `Authorization: Bearer ttk_…` (create one with `teitunnel api-key create NAME`).",
        );
    };
    match (guard.verify)(key).await {
        Some(key_name) => {
            request.extensions_mut().insert(HttpIdentity { key_name });
            next.run(request).await
        }
        None => {
            if let Some(ip) = ip {
                let mut failures = guard
                    .failures
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner);
                failures.entry(ip).or_insert((0, Instant::now())).0 += 1;
            }
            refuse(StatusCode::UNAUTHORIZED, "Invalid API key.")
        }
    }
}

/// The `/mcp` endpoint for `server`, authenticated with `verify`.
pub fn router(server: McpServer, verify: KeyVerifier, options: HttpOptions) -> Router {
    let mut config = StreamableHttpServerConfig::default()
        .with_cancellation_token(options.cancel.clone())
        .with_max_request_body_bytes(MAX_BODY)
        .with_allowed_origins(options.allowed_origins.clone());
    config = if options.allowed_origins.is_empty() {
        config.enforce_origin_validation()
    } else {
        config
    };
    if options.any_host {
        config = config.disable_allowed_hosts();
    }
    let service = StreamableHttpService::new(
        move || Ok(server.session()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let guard_state = Guard {
        verify,
        origins: Arc::new(options.allowed_origins),
        failures: Arc::default(),
    };
    Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(guard_state, guard))
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        backend::SharedBackend,
        config::Mode,
        tools::tests::{FakeBackend, settings},
    };

    struct Running {
        base: String,
        backend: Arc<FakeBackend>,
        http: reqwest::Client,
    }

    async fn start(mode: Mode) -> Running {
        let backend = FakeBackend::new();
        let shared: SharedBackend = backend.clone();
        let server = McpServer::builder(shared, settings(mode))
            .via("mcp over HTTP")
            .build();
        let verify: KeyVerifier = Arc::new(|key: String| {
            Box::pin(async move { (key == "ttk_good").then(|| "ci".to_owned()) })
        });
        let app = router(server, verify, HttpOptions::default());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let _ = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .await;
        });
        Running {
            base: format!("http://{addr}/mcp"),
            backend,
            http: reqwest::Client::new(),
        }
    }

    impl Running {
        fn post(&self, key: Option<&str>, body: &Value) -> reqwest::RequestBuilder {
            let mut request = self
                .http
                .post(&self.base)
                .header("accept", "application/json, text/event-stream")
                .header("content-type", "application/json")
                .body(body.to_string());
            if let Some(key) = key {
                request = request.bearer_auth(key);
            }
            request
        }
    }

    fn initialize() -> Value {
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "http-agent", "version": "0.1" }
            }
        })
    }

    #[tokio::test]
    async fn needs_an_api_key() {
        let server = start(Mode::Ask).await;
        let none = server.post(None, &initialize()).send().await.unwrap();
        assert_eq!(none.status(), 401);
        assert!(none.headers().contains_key("www-authenticate"));
        let bad = server
            .post(Some("ttk_bad"), &initialize())
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), 401);
    }

    #[tokio::test]
    async fn refuses_browser_origins() {
        let server = start(Mode::Ask).await;
        let response = server
            .post(Some("ttk_good"), &initialize())
            .header("origin", "https://evil.example")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403);
    }

    #[tokio::test]
    async fn runs_a_session_and_records_the_key() {
        let server = start(Mode::Full).await;
        let response = server
            .post(Some("ttk_good"), &initialize())
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let session = response
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(ToOwned::to_owned)
            .expect("a session id");
        let body = response.text().await.unwrap();
        assert!(body.contains("\"teitunnel\""), "{body}");

        let with_session = |body: Value| {
            server
                .post(Some("ttk_good"), &body)
                .header("mcp-session-id", &session)
                .header("mcp-protocol-version", "2025-11-25")
        };
        let initialized =
            with_session(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
                .send()
                .await
                .unwrap();
        assert!(initialized.status().is_success());
        let listed = with_session(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" }))
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(listed.contains("share_port"), "{listed}");
        let called = with_session(json!({
            "jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": { "name": "share_port", "arguments": { "target": "3000", "hostname": "demo.xyz.com" } }
        }))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
        assert!(called.contains("https://demo.xyz.com"), "{called}");
        let actor = server
            .backend
            .lock()
            .applied
            .last()
            .and_then(|(_, a)| a.clone())
            .unwrap();
        assert_eq!(actor.client, "http-agent");
        assert_eq!(actor.via, "mcp over HTTP (API key \"ci\")");
    }

    #[test]
    fn matches_origins_exactly() {
        let allowed = vec!["https://app.example.com".to_owned()];
        assert!(origin_allowed("https://app.example.com/", &allowed));
        assert!(!origin_allowed("https://evil.example.com", &allowed));
        assert!(!origin_allowed("http://app.example.com", &allowed));
        assert!(!origin_allowed("null", &[]));
    }
}
