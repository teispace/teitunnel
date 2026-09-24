//! Presets for putting AI services online safely, enforced by the inspector:
//!
//! - **An MCP server** (M12-03): found with a Streamable HTTP `initialize` probe (or an
//!   SSE `endpoint` event), shared on the person's own domain (Quick Tunnels don't carry
//!   event streams), with stream keep-alive and a bearer token checked by Lens (what MCP
//!   clients send), and client configurations ready to paste.
//! - **A local AI server** (Ollama, LM Studio, vLLM): OpenAI-compatible clients send
//!   `Authorization: Bearer`, which Lens checks.
//!
//! Tokens come from the OS's random generator, are kept in the keychain per hostname
//! (so a client's configuration keeps working next time), and are shown once.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{
    Secret,
    discovery::LocalService,
    domain_shares::{self, ShareRequest},
    engine::{CloudApi, Connectors, Context, Engine, Outcome},
    secrets::Secrets,
};

use super::{InspectError, Inspector, TapScope, TapSpec, secrets};

/// How long a probe waits.
const PROBE_TIMEOUT: Duration = Duration::from_secs(3);
/// Bytes of a probe's answer read at most.
const PROBE_BYTES: usize = 64 * 1024;
/// The protocol version the probe offers (servers answer with one they support).
const PROTOCOL: &str = "2025-11-25";

/// How an MCP server is reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum McpTransport {
    /// Streamable HTTP (one endpoint, POST and optional SSE).
    StreamableHttp,
    /// The older HTTP+SSE transport (a GET event stream announcing a POST endpoint).
    Sse,
}

/// A local MCP server found by a probe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct McpProbe {
    /// Transport.
    pub transport: McpTransport,
    /// The endpoint's path, e.g. `/mcp`.
    pub path: String,
    /// The server's name, when it said.
    pub server_name: Option<String>,
    /// Its version.
    pub server_version: Option<String>,
    /// The protocol version it chose.
    pub protocol_version: Option<String>,
    /// It asked for credentials already (401): it has its own authentication.
    pub requires_auth: bool,
}

fn client() -> Option<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(PROBE_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
        .build()
        .ok()
}

fn join(origin: &str, path: &str) -> String {
    format!(
        "{}/{}",
        origin.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Reads at most [`PROBE_BYTES`] of a response body.
async fn bounded(mut response: reqwest::Response) -> String {
    let mut body = Vec::new();
    while body.len() < PROBE_BYTES {
        match tokio::time::timeout(PROBE_TIMEOUT, response.chunk()).await {
            Ok(Ok(Some(chunk))) => body.extend_from_slice(&chunk),
            _ => break,
        }
        // An event stream never ends; the first event is enough.
        if body.windows(2).any(|w| w == b"\n\n") && !body.starts_with(b"{") {
            break;
        }
    }
    body.truncate(PROBE_BYTES);
    String::from_utf8_lossy(&body).into_owned()
}

/// The JSON-RPC result in a JSON or SSE answer.
fn rpc_result(body: &str) -> Option<serde_json::Value> {
    let parse = |text: &str| serde_json::from_str::<serde_json::Value>(text.trim()).ok();
    let value = parse(body).or_else(|| {
        body.lines()
            .filter_map(|line| line.strip_prefix("data:"))
            .find_map(parse)
    })?;
    value.get("result").cloned()
}

async fn probe_streamable(client: &reqwest::Client, origin: &str, path: &str) -> Option<McpProbe> {
    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": { "name": "teitunnel-probe", "version": env!("CARGO_PKG_VERSION") }
        }
    });
    let response = client
        .post(join(origin, path))
        .header("accept", "application/json, text/event-stream")
        .json(&request)
        .send()
        .await
        .ok()?;
    let status = response.status();
    if status == reqwest::StatusCode::UNAUTHORIZED {
        let mcp = response.headers().contains_key("www-authenticate");
        return mcp.then(|| McpProbe {
            transport: McpTransport::StreamableHttp,
            path: path.to_owned(),
            server_name: None,
            server_version: None,
            protocol_version: None,
            requires_auth: true,
        });
    }
    if !status.is_success() {
        return None;
    }
    let session = response
        .headers()
        .get("mcp-session-id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let result = rpc_result(&bounded(response).await)?;
    // End the session the probe opened.
    if let Some(session) = session {
        let _ = client
            .delete(join(origin, path))
            .header("mcp-session-id", session)
            .send()
            .await;
    }
    let info = result.get("serverInfo");
    let text =
        |value: Option<&serde_json::Value>| value.and_then(|v| v.as_str()).map(str::to_owned);
    Some(McpProbe {
        transport: McpTransport::StreamableHttp,
        path: path.to_owned(),
        server_name: text(info.and_then(|i| i.get("name"))),
        server_version: text(info.and_then(|i| i.get("version"))),
        protocol_version: text(result.get("protocolVersion")),
        requires_auth: false,
    })
}

async fn probe_sse(client: &reqwest::Client, origin: &str, path: &str) -> Option<McpProbe> {
    let response = client
        .get(join(origin, path))
        .header("accept", "text/event-stream")
        .send()
        .await
        .ok()?;
    let is_stream = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.starts_with("text/event-stream"));
    if !response.status().is_success() || !is_stream {
        return None;
    }
    let body = bounded(response).await;
    body.lines()
        .any(|line| line.trim() == "event: endpoint" || line.trim() == "event:endpoint")
        .then(|| McpProbe {
            transport: McpTransport::Sse,
            path: path.to_owned(),
            server_name: None,
            server_version: None,
            protocol_version: None,
            requires_auth: false,
        })
}

/// Looks for an MCP server at `origin` (e.g. `http://localhost:8000`): at `path`, or at
/// `/mcp`, `/sse` and `/`. Reads at most a little of each answer and never follows
/// redirects.
pub async fn probe_mcp(origin: &str, path: Option<&str>) -> Option<McpProbe> {
    let client = client()?;
    let candidates: Vec<&str> = match path {
        Some(path) => vec![path],
        None => vec!["/mcp", "/sse", "/"],
    };
    for path in candidates {
        let path = if path.starts_with('/') {
            path.to_owned()
        } else {
            format!("/{path}")
        };
        if let Some(found) = probe_streamable(&client, origin, &path).await {
            return Some(found);
        }
        if let Some(found) = probe_sse(&client, origin, &path).await {
            return Some(found);
        }
    }
    None
}

/// Local AI servers with an OpenAI-compatible API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum AiServer {
    /// Ollama (port 11434).
    Ollama,
    /// LM Studio's server (port 1234).
    LmStudio,
    /// vLLM (port 8000).
    Vllm,
    /// Another server answering `GET /v1/models`.
    OpenAiCompatible,
}

impl AiServer {
    /// Its name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Ollama => "Ollama",
            Self::LmStudio => "LM Studio",
            Self::Vllm => "vLLM",
            Self::OpenAiCompatible => "OpenAI-compatible server",
        }
    }

    /// Where the OpenAI-compatible API lives.
    pub fn api_path(self) -> &'static str {
        "/v1"
    }

    /// What a service looks like, from its process and port.
    pub fn guess(service: &LocalService) -> Option<Self> {
        let process = service.process.to_ascii_lowercase();
        if process.contains("ollama") || service.port == 11434 {
            Some(Self::Ollama)
        } else if process.contains("lm studio")
            || process.contains("lmstudio")
            || process == "lms"
            || service.port == 1234
        {
            Some(Self::LmStudio)
        } else if process.contains("vllm") {
            Some(Self::Vllm)
        } else {
            None
        }
    }
}

/// Asks `origin` whether it's an OpenAI-compatible server (`GET /v1/models`, or
/// Ollama's `GET /api/version`).
pub async fn probe_ai(origin: &str) -> Option<AiServer> {
    let client = client()?;
    if let Ok(response) = client.get(join(origin, "/api/version")).send().await
        && response.status().is_success()
        && bounded(response).await.contains("\"version\"")
    {
        return Some(AiServer::Ollama);
    }
    let response = client.get(join(origin, "/v1/models")).send().await.ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: serde_json::Value = serde_json::from_str(&bounded(response).await).ok()?;
    let models = body.get("data")?.as_array()?;
    let owned_by = |who: &str| {
        models
            .iter()
            .any(|m| m.get("owned_by").and_then(|o| o.as_str()) == Some(who))
    };
    Some(if owned_by("vllm") {
        AiServer::Vllm
    } else if owned_by("organization_owner") {
        AiServer::LmStudio
    } else {
        AiServer::OpenAiCompatible
    })
}

/// Local AI servers among `services`, confirmed by a probe.
pub async fn ai_servers(services: &[LocalService]) -> Vec<(LocalService, AiServer)> {
    let mut found = Vec::new();
    for service in services {
        let guessed = AiServer::guess(service);
        let likely = guessed.is_some() || service.port == 8000;
        if !likely {
            continue;
        }
        if let Some(kind) = probe_ai(&service.origin).await {
            let kind = match (guessed, kind) {
                (Some(guess), AiServer::OpenAiCompatible) => guess,
                (_, kind) => kind,
            };
            found.push((service.clone(), kind));
        }
    }
    found
}

/// Ready-to-use configurations for an MCP server at `url` behind a bearer token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct McpClientConfigs {
    /// For a terminal: `claude mcp add …`.
    pub claude_code: String,
    /// Cursor's `mcp.json` entry.
    pub cursor: String,
    /// VS Code's `.vscode/mcp.json` entry.
    pub vscode: String,
}

/// Configurations for `name` at `url`, sending `token`.
pub fn mcp_client_configs(name: &str, url: &str, token: &str) -> McpClientConfigs {
    let header = format!("Bearer {token}");
    let pretty =
        |value: serde_json::Value| serde_json::to_string_pretty(&value).unwrap_or_default();
    McpClientConfigs {
        claude_code: format!(
            "claude mcp add --transport http {name} {url} --header \"Authorization: {header}\""
        ),
        cursor: pretty(serde_json::json!({
            "mcpServers": { name: { "url": url, "headers": { "Authorization": header } } }
        })),
        vscode: pretty(serde_json::json!({
            "servers": { name: { "type": "http", "url": url, "headers": { "Authorization": header } } }
        })),
    }
}

/// What to share and how.
#[derive(Debug, Clone)]
pub struct ExposeRequest<'a> {
    /// A hostname on one of the account's domains.
    pub hostname: &'a str,
    /// The local service, e.g. `http://localhost:8000`.
    pub origin: &'a str,
    /// Make a new token even if one is saved for the hostname.
    pub new_token: bool,
    /// When it ends by itself (milliseconds since the epoch).
    pub expires_at: Option<u64>,
}

/// An exposed service.
#[derive(Debug)]
pub struct Exposed {
    /// The public URL (`https://hostname`).
    pub url: String,
    /// The bearer token clients send.
    pub token: Secret<String>,
    /// Whether the token was made now (show it once) or read from the keychain.
    pub token_is_new: bool,
    /// The share's outcome.
    pub outcome: Outcome,
}

/// The token for `hostname`: the saved one, or a new one (saved).
///
/// # Errors
/// The keychain refused, or no randomness.
pub async fn token_for(
    secrets: Option<&Secrets>,
    hostname: &str,
    new: bool,
) -> Result<(Secret<String>, bool), InspectError> {
    if !new
        && let Some(secrets) = secrets
        && let Some(token) = secrets::bearer_token(secrets, hostname).await?
    {
        return Ok((token, false));
    }
    let token = secrets::generate_token()
        .map_err(|e| InspectError::Lens(lens::LensError::Random(e.to_string())))?;
    if let Some(secrets) = secrets {
        secrets::set_bearer_token(secrets, hostname, token.clone()).await?;
    }
    Ok((token, true))
}

/// Shares `origin` on `hostname` (a temporary route, as with any share on your domain)
/// through a tap that requires the bearer token and keeps event streams alive. Stop it
/// with [`domain_shares::stop`] and [`Inspector::stop`].
///
/// # Errors
/// A plan that would replace someone else's DNS record (`NeedsConfirmation`), engine
/// errors, or the keychain.
pub async fn expose<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    inspector: &Inspector,
    request: ExposeRequest<'_>,
) -> Result<Exposed, InspectError> {
    let hostname = request.hostname.trim().to_ascii_lowercase();
    let (token, token_is_new) =
        token_for(inspector.secrets(), &hostname, request.new_token).await?;
    let url = format!("https://{hostname}");
    let mut spec = TapSpec::new(
        TapScope::route(ctx.account, &hostname, None),
        &hostname,
        request.origin,
    );
    spec.public_url = Some(url.clone());
    spec.bearer = vec![token.clone()];
    let tap = inspector.start(spec).await?;
    let started = domain_shares::start(
        engine,
        api,
        connectors,
        ctx,
        ShareRequest {
            hostname: &hostname,
            origin: &tap.address,
            access: None,
            expires_at: request.expires_at,
            owner: inspector.owner(),
            host_header: None,
        },
    )
    .await;
    match started {
        Ok(outcome @ Outcome::Applied { .. }) => Ok(Exposed {
            url,
            token,
            token_is_new,
            outcome,
        }),
        Ok(outcome) => {
            inspector.stop(&tap.id).await;
            Ok(Exposed {
                url,
                token,
                token_is_new,
                outcome,
            })
        }
        Err(err) => {
            inspector.stop(&tap.id).await;
            Err(err.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::secrets::MemoryStore;

    /// A one-route HTTP server answering `path` with `status`, `content_type` and `body`
    /// (everything else 404).
    async fn server(
        routes: Vec<(&'static str, &'static str, u16, &'static str, &'static str)>,
    ) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let routes = routes.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let head = String::from_utf8_lossy(&buf[..n]).to_string();
                    let mut parts = head.split_whitespace();
                    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
                    let (status, content_type, body) = routes
                        .iter()
                        .find(|(m, p, ..)| *m == method && *p == path)
                        .map_or((404, "text/plain", "no"), |(_, _, s, t, b)| (*s, *t, *b));
                    let extra = if status == 401 {
                        "WWW-Authenticate: Bearer resource_metadata=\"x\"\r\n"
                    } else {
                        ""
                    };
                    let response = format!(
                        "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn finds_a_streamable_http_mcp_server() {
        let origin = server(vec![(
            "POST",
            "/mcp",
            200,
            "application/json",
            r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"notes","version":"1.2"}}}"#,
        )])
        .await;
        let found = probe_mcp(&origin, None).await.unwrap();
        assert_eq!(found.transport, McpTransport::StreamableHttp);
        assert_eq!(found.path, "/mcp");
        assert_eq!(found.server_name.as_deref(), Some("notes"));
        assert_eq!(found.protocol_version.as_deref(), Some("2025-11-25"));
    }

    #[tokio::test]
    async fn finds_sse_servers_and_ones_that_want_auth() {
        let origin = server(vec![
            (
                "GET",
                "/sse",
                200,
                "text/event-stream",
                "event: endpoint\ndata: /messages?s=1\n\n",
            ),
            ("POST", "/secure", 401, "application/json", "{}"),
        ])
        .await;
        let sse = probe_mcp(&origin, None).await.unwrap();
        assert_eq!(
            (sse.transport, sse.path.as_str()),
            (McpTransport::Sse, "/sse")
        );
        let secure = probe_mcp(&origin, Some("secure")).await.unwrap();
        assert!(secure.requires_auth);
        assert!(probe_mcp(&origin, Some("/nothing")).await.is_none());
    }

    #[tokio::test]
    async fn recognises_local_ai_servers() {
        let ollama = server(vec![(
            "GET",
            "/api/version",
            200,
            "application/json",
            r#"{"version":"0.12.0"}"#,
        )])
        .await;
        assert_eq!(probe_ai(&ollama).await, Some(AiServer::Ollama));
        let vllm = server(vec![(
            "GET",
            "/v1/models",
            200,
            "application/json",
            r#"{"object":"list","data":[{"id":"m","owned_by":"vllm"}]}"#,
        )])
        .await;
        assert_eq!(probe_ai(&vllm).await, Some(AiServer::Vllm));
        let nothing = server(vec![]).await;
        assert_eq!(probe_ai(&nothing).await, None);
    }

    #[test]
    fn guesses_ai_servers_from_discovery() {
        let service = |process: &str, port: u16| LocalService {
            port,
            all_interfaces: false,
            pid: 1,
            process: process.into(),
            kind: crate::discovery::ServiceKind::Other,
            project: None,
            origin: format!("http://localhost:{port}"),
        };
        assert_eq!(
            AiServer::guess(&service("ollama", 11434)),
            Some(AiServer::Ollama)
        );
        assert_eq!(
            AiServer::guess(&service("LM Studio", 1234)),
            Some(AiServer::LmStudio)
        );
        assert_eq!(
            AiServer::guess(&service("python3 -m vllm", 8000)),
            Some(AiServer::Vllm)
        );
        assert_eq!(AiServer::guess(&service("node", 3000)), None);
        assert_eq!(AiServer::Ollama.api_path(), "/v1");
    }

    #[test]
    fn writes_client_configurations() {
        let configs = mcp_client_configs("notes", "https://mcp.xyz.com/mcp", "tt_abc");
        assert_eq!(
            configs.claude_code,
            "claude mcp add --transport http notes https://mcp.xyz.com/mcp --header \"Authorization: Bearer tt_abc\""
        );
        let cursor: serde_json::Value = serde_json::from_str(&configs.cursor).unwrap();
        assert_eq!(
            cursor["mcpServers"]["notes"]["headers"]["Authorization"],
            "Bearer tt_abc"
        );
        let vscode: serde_json::Value = serde_json::from_str(&configs.vscode).unwrap();
        assert_eq!(vscode["servers"]["notes"]["type"], "http");
    }

    #[tokio::test]
    async fn tokens_are_saved_per_hostname_and_reused() {
        let secrets: Secrets = Arc::new(MemoryStore::default());
        let (first, new) = token_for(Some(&secrets), "mcp.xyz.com", false)
            .await
            .unwrap();
        assert!(new);
        let (again, new) = token_for(Some(&secrets), "MCP.xyz.com", false)
            .await
            .unwrap();
        assert!(!new);
        assert_eq!(first, again);
        let (fresh, new) = token_for(Some(&secrets), "mcp.xyz.com", true)
            .await
            .unwrap();
        assert!(new);
        assert_ne!(fresh, first);
    }
}
