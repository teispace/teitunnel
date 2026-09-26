//! "Test connection": starts the MCP server exactly as an AI client's configuration
//! says (program, arguments, environment; no shell), does the MCP handshake over
//! stdio (`initialize`, then `tools/list`) and stops it. What `claude mcp list` and
//! VS Code's "List Servers" do to show a server works.
//!
//! The check introduces itself as [`CHECK_CLIENT`], which the app doesn't count as an
//! agent. It never calls a tool.

use std::{
    collections::BTreeMap,
    path::Path,
    process::Stdio,
    time::{Duration, Instant},
};

use serde::Serialize;
use serde_json::{Value, json};
use teitunnel_core::agents_seen::CHECK_CLIENT;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdout, Command},
};

/// The protocol version the check asks for (servers answer with one they support).
const PROTOCOL: &str = "2025-11-25";

/// How long a check may take by default.
pub const TIMEOUT: Duration = Duration::from_secs(15);

/// Longest line read from the server (a tool list is well under this).
const MAX_LINE: usize = 4 * 1024 * 1024;

/// What a working server said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checked {
    /// The server's name, e.g. `teitunnel`.
    pub server: String,
    /// Its version.
    pub version: String,
    /// The protocol version it chose.
    pub protocol: String,
    /// How many tools it offers.
    pub tools: u32,
    /// How long the handshake took, in milliseconds.
    pub millis: u32,
}

/// Why the check failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CheckError {
    /// The program couldn't be started.
    #[error("The program couldn't be started: {0}")]
    Start(String),
    /// It ended before answering.
    #[error("The server stopped before answering.{}", stderr_note(.0))]
    Exited(String),
    /// It didn't answer in time.
    #[error("The server didn't answer within {0} seconds.")]
    Timeout(u64),
    /// It answered something that isn't MCP.
    #[error("The server's answer isn't MCP: {0}")]
    Protocol(String),
}

fn stderr_note(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(" It said: {stderr}")
    }
}

/// Starts `program args` with `env` added and checks it answers as an MCP server.
///
/// # Errors
/// See [`CheckError`].
pub async fn check(
    program: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    timeout: Duration,
) -> Result<Checked, CheckError> {
    let started = Instant::now();
    let mut command = Command::new(program);
    command
        .args(args)
        .envs(env)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    cloudflared::process::no_console(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| CheckError::Start(e.to_string()))?;
    let outcome = tokio::time::timeout(timeout, handshake(&mut child)).await;
    let _ = child.start_kill();
    let result = match outcome {
        Ok(Ok((server, version, protocol, tools))) => Ok(Checked {
            server,
            version,
            protocol,
            tools,
            millis: u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX),
        }),
        Ok(Err(HandshakeError::Closed)) => Err(CheckError::Exited(stderr_tail(&mut child).await)),
        Ok(Err(HandshakeError::Protocol(reason))) => Err(CheckError::Protocol(reason)),
        Err(_) => Err(CheckError::Timeout(timeout.as_secs())),
    };
    let _ = child.wait().await;
    result
}

enum HandshakeError {
    Closed,
    Protocol(String),
}

async fn handshake(child: &mut Child) -> Result<(String, String, String, u32), HandshakeError> {
    let mut stdin = child.stdin.take().ok_or(HandshakeError::Closed)?;
    let mut stdout = BufReader::new(child.stdout.take().ok_or(HandshakeError::Closed)?);
    let send = |message: Value| {
        let mut line = message.to_string();
        line.push('\n');
        line
    };
    let initialize = send(json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": PROTOCOL,
            "capabilities": {},
            "clientInfo": { "name": CHECK_CLIENT, "version": env!("CARGO_PKG_VERSION") }
        }
    }));
    stdin
        .write_all(initialize.as_bytes())
        .await
        .map_err(|_| HandshakeError::Closed)?;
    let result = answer(&mut stdout, 1).await?;
    let info = &result["serverInfo"];
    let server = info["name"].as_str().unwrap_or_default().to_owned();
    let version = info["version"].as_str().unwrap_or_default().to_owned();
    let protocol = result["protocolVersion"]
        .as_str()
        .ok_or_else(|| HandshakeError::Protocol("no protocol version".into()))?
        .to_owned();
    let rest = [
        send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })),
        send(json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {} })),
    ]
    .concat();
    stdin
        .write_all(rest.as_bytes())
        .await
        .map_err(|_| HandshakeError::Closed)?;
    let tools = answer(&mut stdout, 2).await?["tools"]
        .as_array()
        .map_or(0, Vec::len);
    Ok((
        server,
        version,
        protocol,
        u32::try_from(tools).unwrap_or(u32::MAX),
    ))
}

/// The result of request `id`, skipping notifications and requests from the server.
async fn answer(stdout: &mut BufReader<ChildStdout>, id: u64) -> Result<Value, HandshakeError> {
    let mut line = String::new();
    loop {
        line.clear();
        let read = (&mut *stdout)
            .take(MAX_LINE as u64)
            .read_line(&mut line)
            .await
            .map_err(|_| HandshakeError::Closed)?;
        if read == 0 {
            return Err(HandshakeError::Closed);
        }
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = serde_json::from_str(line.trim())
            .map_err(|_| HandshakeError::Protocol(first_line(&line)))?;
        if message["id"].as_u64() != Some(id) || message.get("method").is_some() {
            continue;
        }
        if let Some(error) = message.get("error") {
            return Err(HandshakeError::Protocol(
                error["message"].as_str().unwrap_or("an error").to_owned(),
            ));
        }
        return Ok(message["result"].clone());
    }
}

fn first_line(text: &str) -> String {
    let line = text.trim().lines().next().unwrap_or_default();
    line.chars().take(200).collect()
}

/// The last lines the server wrote to standard error, for the message.
async fn stderr_tail(child: &mut Child) -> String {
    let Some(mut stderr) = child.stderr.take() else {
        return String::new();
    };
    let mut text = String::new();
    let _ = tokio::time::timeout(
        Duration::from_secs(1),
        (&mut stderr).take(64 * 1024).read_to_string(&mut text),
    )
    .await;
    let lines: Vec<&str> = text.trim().lines().collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" ");
    tail.chars().take(400).collect()
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A tiny MCP server in Python (skipped where Python isn't installed).
    const FAKE: &str = r#"
import json, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "method": "notifications/message", "params": {}}), flush=True)
        print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": {"protocolVersion": "2025-11-25", "capabilities": {}, "serverInfo": {"name": "fake", "version": "1.2"}}}), flush=True)
    elif m.get("method") == "tools/list":
        print(json.dumps({"jsonrpc": "2.0", "id": m["id"], "result": {"tools": [{"name": "a"}, {"name": "b"}]}}), flush=True)
"#;

    fn python() -> Option<std::path::PathBuf> {
        [
            "/usr/bin/python3",
            "/opt/homebrew/bin/python3",
            "/usr/local/bin/python3",
        ]
        .into_iter()
        .map(std::path::PathBuf::from)
        .find(|p| p.is_file())
    }

    #[tokio::test]
    async fn checks_a_working_server() {
        let Some(python) = python() else { return };
        let checked = check(
            &python,
            &["-c".into(), FAKE.into()],
            &BTreeMap::new(),
            TIMEOUT,
        )
        .await
        .unwrap();
        assert_eq!(checked.server, "fake");
        assert_eq!(checked.version, "1.2");
        assert_eq!(checked.protocol, "2025-11-25");
        assert_eq!(checked.tools, 2);
    }

    #[tokio::test]
    async fn says_why_a_server_doesnt_work() {
        let missing = check(
            Path::new("/nonexistent/teitunnel-cli"),
            &[],
            &BTreeMap::new(),
            TIMEOUT,
        )
        .await;
        assert!(matches!(missing, Err(CheckError::Start(_))), "{missing:?}");

        let Some(python) = python() else { return };
        let exits = check(
            &python,
            &[
                "-c".into(),
                "import sys; sys.stderr.write('no such mode\\n')".into(),
            ],
            &BTreeMap::new(),
            TIMEOUT,
        )
        .await;
        assert_eq!(exits, Err(CheckError::Exited("no such mode".into())));

        let silent = check(
            &python,
            &["-c".into(), "import time; time.sleep(30)".into()],
            &BTreeMap::new(),
            Duration::from_millis(300),
        )
        .await;
        assert_eq!(silent, Err(CheckError::Timeout(0)));

        let not_mcp = check(
            &python,
            &["-c".into(), "print('hello')".into()],
            &BTreeMap::new(),
            TIMEOUT,
        )
        .await;
        assert_eq!(not_mcp, Err(CheckError::Protocol("hello".into())));
    }
}
