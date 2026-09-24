//! `expose_mcp_server`: puts a local MCP server online for remote AI clients, safely.
//!
//! The server is found with a Streamable HTTP `initialize` probe (or an SSE `endpoint`
//! event), shared on the person's own domain (Quick Tunnels don't carry event streams)
//! through the inspector, which keeps streams alive and requires a bearer token (what
//! MCP clients send). The token is made with the OS's random generator and kept in the
//! keychain for the hostname; agents never see it unless the server allows secrets:
//! the person reads it with `teitunnel token <hostname>`.

use std::time::Duration;

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use teitunnel_core::{
    domain::OriginUrl,
    inspect::{
        Inspector, TapScope, TapSpec,
        expose::{self, McpClientConfigs, McpProbe, McpTransport},
    },
};

use crate::{
    backend::{BoxFuture, DomainShareRequest, SharedBackend},
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolProvider,
        ToolResult, ToolSpec, arguments,
    },
    tools::{Hints, spec},
};

/// What stands for the token in configurations an agent sees.
pub const TOKEN_PLACEHOLDER: &str = "<TOKEN>";

/// The exposure tools, over the host's backend and inspector.
#[derive(Clone)]
pub struct ExposeTools {
    backend: SharedBackend,
    inspector: Inspector,
}

impl std::fmt::Debug for ExposeTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ExposeTools").finish_non_exhaustive()
    }
}

impl ExposeTools {
    /// Tools sharing through `backend`, protected by `inspector`'s taps.
    pub fn new(backend: SharedBackend, inspector: Inspector) -> Self {
        Self { backend, inspector }
    }
}

/// What to expose.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExposeArgs {
    /// The local MCP server: a port (`8000`), `host:port` or a URL.
    origin: String,
    /// Its endpoint path, e.g. `/mcp` (default: `/mcp`, then `/sse` and `/` are tried).
    #[serde(default)]
    path: Option<String>,
    /// A hostname on one of the person's domains, e.g. `mcp.example.com`.
    hostname: String,
    /// The account (id or name), when several are connected.
    #[serde(default)]
    account: Option<String>,
    /// The name clients show for the server (default: the server's own name).
    #[serde(default)]
    name: Option<String>,
    /// Make a new token even if one is saved for the hostname (clients using the old one
    /// stop working).
    #[serde(default)]
    new_token: bool,
    /// Stop sharing after this many minutes (default: when this MCP server ends).
    #[serde(default)]
    expires_in_minutes: Option<u32>,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// The exposed server.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExposeOut {
    /// `exposed`, `needsApproval` or `declined`.
    outcome: String,
    /// What happened, and what to tell the person.
    message: String,
    /// The public MCP endpoint, e.g. `https://mcp.example.com/mcp`.
    url: Option<String>,
    /// What the probe found.
    server: Option<ServerFound>,
    /// Whether the token was made now (clients need the new one).
    new_token: bool,
    /// The bearer token (only when the server allows secrets; otherwise the person runs
    /// `teitunnel token <hostname>`).
    token: Option<String>,
    /// Ready-to-use client configurations (the token is `<TOKEN>` unless allowed).
    configs: Option<ClientConfigs>,
    /// How claude.ai and ChatGPT connectors fit in.
    notes: Vec<String>,
}

/// The MCP server the probe found.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerFound {
    /// `streamableHttp` or `sse`.
    transport: String,
    /// Its endpoint path.
    path: String,
    /// Its name, if it said.
    name: Option<String>,
    /// Its version.
    version: Option<String>,
    /// The protocol version it chose.
    protocol_version: Option<String>,
    /// It has its own authentication (answered 401).
    requires_auth: bool,
}

impl From<McpProbe> for ServerFound {
    fn from(probe: McpProbe) -> Self {
        Self {
            transport: match probe.transport {
                McpTransport::StreamableHttp => "streamableHttp",
                McpTransport::Sse => "sse",
            }
            .into(),
            path: probe.path,
            name: probe.server_name,
            version: probe.server_version,
            protocol_version: probe.protocol_version,
            requires_auth: probe.requires_auth,
        }
    }
}

/// Client configurations.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ClientConfigs {
    /// A `claude mcp add` command for Claude Code.
    claude_code: String,
    /// Cursor's `mcp.json`.
    cursor: String,
    /// VS Code's `.vscode/mcp.json`.
    vscode: String,
}

impl From<McpClientConfigs> for ClientConfigs {
    fn from(configs: McpClientConfigs) -> Self {
        Self {
            claude_code: configs.claude_code,
            cursor: configs.cursor,
            vscode: configs.vscode,
        }
    }
}

/// Notes on web connectors (they can't send a static bearer header).
pub fn connector_notes(hostname: &str) -> Vec<String> {
    vec![
        "Claude Code, Cursor and VS Code send the Authorization header from their configuration.".to_owned(),
        "Claude.ai and ChatGPT custom connectors can't send a fixed bearer token: they connect with OAuth or without authentication. OAuth in front of a local MCP server comes later; until then, use them only with a server that has its own authentication.".to_owned(),
        format!("Anyone with the token can use the server: show it with `teitunnel token {hostname}`, and make a new one with `teitunnel token {hostname} --new`."),
    ]
}

fn declined(outcome: &str, message: String) -> ToolOutput {
    ToolOutput::new(&ExposeOut {
        outcome: outcome.into(),
        message,
        url: None,
        server: None,
        new_token: false,
        token: None,
        configs: None,
        notes: Vec::new(),
    })
}

impl ExposeTools {
    async fn expose(&self, args: JsonObject, ctx: &ToolContext) -> ToolResult {
        let args: ExposeArgs = arguments(args)?;
        let origin = OriginUrl::parse(&args.origin)
            .map_err(|e| ToolError::new(format!("{}: {e}", args.origin)))?;
        let hostname = args.hostname.trim().to_ascii_lowercase();
        let probe = expose::probe_mcp(origin.as_str(), args.path.as_deref())
            .await
            .ok_or_else(|| {
                ToolError::new(format!(
                    "No MCP server answered at {origin} ({}). Start it, or pass its endpoint path.",
                    args.path.as_deref().unwrap_or("/mcp, /sse or /")
                ))
            })?;
        let account = crate::tools::account(&self.backend, args.account.as_deref()).await?;
        let url = format!("https://{hostname}{}", probe.path.trim_end_matches('/'));
        let url = if probe.path == "/" {
            format!("https://{hostname}/")
        } else {
            url
        };
        let details = format!(
            "Share the MCP server at {origin}{} publicly at {url}, on your domain through this machine's tunnel. Clients must send a bearer token (kept in the keychain); streams are kept alive. It ends when this MCP server stops{}.",
            probe.path,
            args.expires_in_minutes
                .map(|m| format!(" or after {m} minutes"))
                .unwrap_or_default()
        );
        match ctx
            .approve(&ApprovalRequest {
                title: format!("Share the MCP server at {url}"),
                details: details.clone(),
                confirmed: args.confirmed,
            })
            .await
        {
            Approval::Granted { .. } => {}
            Approval::NeedsConfirmation => {
                return Ok(declined(
                    "needsApproval",
                    format!(
                        "Nothing was shared. Show the person this and call again with \"confirmed\": true if they agree:\n{details}"
                    ),
                ));
            }
            Approval::Declined(why) => {
                return Ok(declined("declined", format!("{why} Nothing was shared.")));
            }
        }
        let (token, new_token) =
            expose::token_for(self.inspector.secrets(), &hostname, args.new_token)
                .await
                .map_err(|e| ToolError::new(e.to_string()))?;
        let mut tap = TapSpec::new(
            TapScope::route(&account.id, &hostname, None),
            &hostname,
            origin.as_str(),
        );
        tap.public_url = Some(format!("https://{hostname}"));
        tap.bearer = vec![token.clone()];
        let tap = self
            .inspector
            .start(tap)
            .await
            .map_err(|e| ToolError::new(e.to_string()))?;
        let shared = self
            .backend
            .start_domain_share(
                DomainShareRequest {
                    account: account.id.clone(),
                    hostname: hostname.clone(),
                    origin: tap.address.clone(),
                    access: None,
                    expires_in: args
                        .expires_in_minutes
                        .map(|m| Duration::from_secs(u64::from(m) * 60)),
                },
                Some(ctx.actor().clone()),
            )
            .await;
        if let Err(err) = shared {
            self.inspector.stop(&tap.id).await;
            return Err(err.into());
        }
        let name = args
            .name
            .or_else(|| probe.server_name.clone())
            .unwrap_or_else(|| "local-mcp".to_owned())
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' {
                    c
                } else {
                    '-'
                }
            })
            .collect::<String>();
        let shown = if ctx.allow_secrets() {
            token.expose().clone()
        } else {
            TOKEN_PLACEHOLDER.to_owned()
        };
        let transport = match probe.transport {
            McpTransport::StreamableHttp => "Streamable HTTP",
            McpTransport::Sse => "HTTP+SSE",
        };
        Ok(ToolOutput::new(&ExposeOut {
            outcome: "exposed".into(),
            message: format!(
                "The MCP server ({transport}) is online at {url}, protected by a bearer token. {}",
                if new_token {
                    format!(
                        "A new token was made: the person sees it with `teitunnel token {hostname}`."
                    )
                } else {
                    format!(
                        "It uses the token saved for {hostname} (`teitunnel token {hostname}` shows it)."
                    )
                }
            ),
            url: Some(url.clone()),
            configs: Some(expose::mcp_client_configs(&name, &url, &shown).into()),
            server: Some(probe.into()),
            new_token,
            token: ctx.allow_secrets().then(|| token.expose().clone()),
            notes: connector_notes(&hostname),
        }))
    }
}

impl ToolProvider for ExposeTools {
    fn tools(&self) -> Vec<ToolSpec> {
        vec![spec::<ExposeArgs, ExposeOut>(
            "expose_mcp_server",
            "Put a local MCP server online",
            "Share an MCP server running on this machine at a hostname on the person's domain, so remote AI clients can use it: Teitunnel checks it answers MCP (Streamable HTTP `initialize`, or SSE), shares it through this machine's tunnel (not a Quick Share: those can't carry event streams), keeps streams alive past Cloudflare's 100-second idle limit, and requires a bearer token. Returns the URL and ready configurations for Claude Code, Cursor and VS Code. The token itself stays with the person (`teitunnel token <hostname>`) unless this server allows secrets.\n\
             \n\
             Example: {\"origin\": \"8000\", \"hostname\": \"mcp.example.com\"}",
            ToolClass::Change,
            Hints {
                read_only: false,
                destructive: false,
                idempotent: false,
                open_world: true,
            },
            Duration::from_secs(120),
        )]
    }

    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: JsonObject,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, ToolResult> {
        Box::pin(async move {
            match name {
                "expose_mcp_server" => self.expose(arguments, ctx).await,
                other => Err(ToolError::new(format!("Unknown tool {other}."))),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use teitunnel_core::secrets::{MemoryStore, Secrets};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;
    use crate::{
        config::Mode,
        registry::ToolContext,
        tools::tests::{FakeBackend, actor, settings},
    };

    /// A Streamable HTTP MCP server that answers `initialize`, and 200 to everything.
    async fn mcp_server() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let _ = socket.read(&mut buf).await;
                    let body = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-11-25","capabilities":{},"serverInfo":{"name":"notes","version":"1.0"}}}"#;
                    let reply = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(reply.as_bytes()).await;
                });
            }
        });
        port
    }

    fn args(value: serde_json::Value) -> JsonObject {
        value.as_object().cloned().unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn exposes_an_mcp_server_behind_a_token_the_agent_never_sees() {
        let port = mcp_server().await;
        let backend = FakeBackend::new();
        let secrets: Secrets = Arc::new(MemoryStore::default());
        let inspector = Inspector::new(None, Some(secrets.clone()), "app");
        let tools = ExposeTools::new(backend.clone(), inspector.clone());
        let ctx = ToolContext::detached(settings(Mode::Full), actor());
        let out = tools
            .call(
                "expose_mcp_server",
                args(serde_json::json!({ "origin": port.to_string(), "hostname": "MCP.xyz.com" })),
                &ctx,
            )
            .await
            .unwrap()
            .structured;
        assert_eq!(out["outcome"], "exposed");
        assert_eq!(out["url"], "https://mcp.xyz.com/mcp");
        assert_eq!(out["server"]["name"], "notes");
        assert!(out["token"].is_null(), "no secrets for agents");
        let claude = out["configs"]["claudeCode"].as_str().unwrap();
        assert!(
            claude.contains(TOKEN_PLACEHOLDER) && claude.contains("notes"),
            "{claude}"
        );
        let token = teitunnel_core::inspect::secrets::bearer_token(&secrets, "mcp.xyz.com")
            .await
            .unwrap()
            .unwrap();
        assert!(!out.to_string().contains(token.expose().as_str()));

        // The share goes to the inspector's tap, which wants the token.
        let tap = inspector.taps().pop().unwrap();
        let applied = backend.lock().applied.clone();
        assert!(matches!(
            &applied[0].0,
            teitunnel_core::engine::Change::AddRoute { route } if route.origin == tap.address
        ));
        assert_eq!(tap.protection.bearer_tokens, 1);
        assert_eq!(tap.sse_keepalive_secs, Some(25));
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        let refused = client
            .post(format!("{}/mcp", tap.address))
            .send()
            .await
            .unwrap();
        assert_eq!(refused.status(), 401);
        let allowed = client
            .post(format!("{}/mcp", tap.address))
            .bearer_auth(token.expose())
            .send()
            .await
            .unwrap();
        assert_eq!(allowed.status(), 200);
        inspector.shutdown().await;
    }

    #[tokio::test]
    async fn says_when_nothing_answers_mcp() {
        let backend = FakeBackend::new();
        let tools = ExposeTools::new(backend, Inspector::new(None, None, "app"));
        let ctx = ToolContext::detached(settings(Mode::Full), actor());
        let err = tools
            .call(
                "expose_mcp_server",
                args(serde_json::json!({ "origin": "1", "hostname": "mcp.xyz.com" })),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(err.message.contains("No MCP server answered"), "{err}");
    }
}
