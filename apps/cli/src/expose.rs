//! Presets for putting AI services online, enforced by the inspector:
//! `teitunnel share <port> --mcp --on <hostname>` (a local MCP server for remote AI
//! clients), `teitunnel share <port> --ai` (a local AI server behind a bearer token),
//! and `teitunnel token <hostname>` (the token a protected hostname expects).

use std::{process::ExitCode, time::Duration};

use teitunnel_core::{
    Secret,
    domain::OriginUrl,
    inspect::expose::{self, AiServer, McpTransport},
    quick_share::HostHeaderChoice,
};

use crate::{
    context::App,
    share::{self, ShareOptions, status},
};

/// The token for `hostname`: saved in the keychain, or made now. Says which.
async fn token(app: &App, hostname: &str, new: bool) -> Result<Secret<String>, String> {
    let (token, is_new) = expose::token_for(Some(app.secrets()), hostname, new)
        .await
        .map_err(|e| e.to_string())?;
    if is_new {
        status(&format!(
            "New bearer token (kept in the keychain; `teitunnel token {hostname}` shows it again):"
        ));
        out!("{}", token.expose())?;
    } else {
        status(&format!(
            "Using the bearer token saved for {hostname} (`teitunnel token {hostname}` shows it; --new-token makes another)."
        ));
    }
    Ok(token)
}

/// `teitunnel share <origin> --mcp --on <hostname>`.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn mcp(
    app: &App,
    origin: &str,
    path: Option<&str>,
    hostname: &str,
    account: Option<&str>,
    new_token: bool,
    stop_after: Option<Duration>,
    options: ShareOptions,
) -> Result<ExitCode, String> {
    let origin_url = OriginUrl::parse(origin).map_err(|e| e.to_string())?;
    let probe = expose::probe_mcp(origin_url.as_str(), path)
        .await
        .ok_or_else(|| {
            format!(
                "No MCP server answered at {origin_url} ({}). Start it, or give its endpoint with --mcp-path.",
                path.unwrap_or("/mcp, /sse or /")
            )
        })?;
    let transport = match probe.transport {
        McpTransport::StreamableHttp => "Streamable HTTP",
        McpTransport::Sse => "HTTP+SSE",
    };
    status(&format!(
        "Found an MCP server ({transport}{}) at {origin_url}{}.",
        probe
            .server_name
            .as_deref()
            .map(|n| format!(", \"{n}\""))
            .unwrap_or_default(),
        probe.path
    ));
    if probe.requires_auth {
        status("It asks for credentials itself; the token below is checked first, then its own.");
    }
    let hostname = hostname.trim().to_ascii_lowercase();
    let token = token(app, &hostname, new_token).await?;
    let url = if probe.path == "/" {
        format!("https://{hostname}/")
    } else {
        format!("https://{hostname}{}", probe.path)
    };
    let name = probe
        .server_name
        .clone()
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
    let secret = token.expose().clone();
    let options = ShareOptions {
        bearer: Some(token),
        ..options
    };
    share::run_on_domain(
        app,
        &hostname,
        origin,
        share::DomainShareOptions {
            account: account.map(str::to_owned),
            allow: None,
            stop_after,
            json: false,
            strict: false,
        },
        &HostHeaderChoice::Off,
        &options,
        |host| {
            let configs = expose::mcp_client_configs(&name, &url, &secret);
            status(&format!(
                "MCP endpoint: {url} (streams kept alive; clients send the token)."
            ));
            status("Claude Code:");
            status(&format!("  {}", configs.claude_code));
            status("Cursor (~/.cursor/mcp.json):");
            status(&configs.cursor);
            status("VS Code (.vscode/mcp.json):");
            status(&configs.vscode);
            for note in teitunnel_mcp::expose::connector_notes(host) {
                status(&note);
            }
            Ok(())
        },
    )
    .await
}

/// `teitunnel share <origin> --ai [--on <hostname>]`: a local AI server behind a
/// bearer token.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn ai(
    origin: &str,
    on: Option<&str>,
    account: Option<&str>,
    new_token: bool,
    stop_after: Option<Duration>,
    qr: bool,
    options: ShareOptions,
) -> Result<ExitCode, String> {
    let origin_url = OriginUrl::parse(origin).map_err(|e| e.to_string())?;
    let server = expose::probe_ai(origin_url.as_str())
        .await
        .ok_or_else(|| {
            format!(
                "{origin_url} doesn't answer like an AI server (GET /v1/models or Ollama's /api/version). Start Ollama (11434), LM Studio's server (1234) or vLLM (8000)."
            )
        })?;
    status(&format!("Found {} at {origin_url}.", server.name()));
    match on {
        Some(hostname) => {
            let app = App::open().await?;
            let hostname = hostname.trim().to_ascii_lowercase();
            let token = token(&app, &hostname, new_token).await?;
            let secret = token.expose().clone();
            let options = ShareOptions {
                bearer: Some(token),
                ..options
            };
            share::run_on_domain(
                &app,
                &hostname,
                origin,
                share::DomainShareOptions {
                    account: account.map(str::to_owned),
                    allow: None,
                    stop_after,
                    json: false,
                    strict: false,
                },
                &HostHeaderChoice::Off,
                &options,
                |host| {
                    print_ai_usage(server, &format!("https://{host}"), &secret);
                    Ok(())
                },
            )
            .await
        }
        None => {
            // A Quick Share's address changes every time: a fresh token each time too.
            let token =
                teitunnel_core::inspect::secrets::generate_token().map_err(|e| e.to_string())?;
            status("Bearer token for this share (shown once):");
            out!("{}", token.expose())?;
            let options = ShareOptions {
                bearer: Some(token),
                ..options
            };
            share::run(
                origin,
                stop_after,
                qr,
                false,
                &HostHeaderChoice::Off,
                &options,
                false,
            )
            .await
        }
    }
}

fn print_ai_usage(server: AiServer, base: &str, token: &str) {
    let api = format!("{base}{}", server.api_path());
    status(&format!("OpenAI-compatible base URL: {api}"));
    status(&format!("  export OPENAI_BASE_URL={api}"));
    status(&format!("  export OPENAI_API_KEY={token}"));
    status(&format!(
        "  curl {api}/models -H \"Authorization: Bearer {token}\""
    ));
}

/// `teitunnel token <hostname> [--new]`: the bearer token a protected hostname expects.
pub(crate) async fn show_token(app: &App, hostname: &str, new: bool) -> Result<ExitCode, String> {
    let hostname = hostname.trim().to_ascii_lowercase();
    let saved = teitunnel_core::inspect::secrets::bearer_token(app.secrets(), &hostname)
        .await
        .map_err(|e| e.to_string())?;
    match (saved, new) {
        (Some(token), false) => out!("{}", token.expose())?,
        (None, false) => {
            return Err(format!(
                "No token is saved for {hostname}. `teitunnel share <port> --mcp --on {hostname}` makes one (or add --new)."
            ));
        }
        (_, true) => {
            let (token, _) = expose::token_for(Some(app.secrets()), &hostname, true)
                .await
                .map_err(|e| e.to_string())?;
            status(&format!(
                "New token for {hostname}; clients with the old one stop working the next time it's shared."
            ));
            out!("{}", token.expose())?;
        }
    }
    Ok(ExitCode::SUCCESS)
}
