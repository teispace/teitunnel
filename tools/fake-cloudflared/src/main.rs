//! Test double for `cloudflared`.
//!
//! Integration tests and E2E runs spawn this binary in place of the real one
//! (`TEITUNNEL_CLOUDFLARED=…`). It accepts the command lines Teitunnel builds, serves
//! the local metrics endpoints (`/ready`, `/quicktunnel`, `/metrics`, `/healthcheck`)
//! and writes JSON log lines to stderr, like cloudflared 2026.9.
//!
//! `FAKE_CFD_SCENARIO` scripts its behaviour:
//! - `healthy` (default): connects after 150 ms.
//! - `slow_start`: connects after 3 s.
//! - `crash_after:<ms>`: exits with status 1 after `<ms>` milliseconds.
//! - `exit_immediately`: exits with status 1 at once (crash-loop tests).
//! - `degraded`: never registers a connection.
//! - `no_url`: connects, but never gets a Quick Share hostname.
//! - `ignore_sigterm`: ignores SIGTERM, so only SIGKILL stops it.
//!
//! As a safety net it exits with status 3 if a tunnel token appears in argv.
#![allow(clippy::print_stderr, clippy::print_stdout)]

use std::{
    process::ExitCode,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use serde_json::json;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

const VERSION: &str = "2026.9.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Healthy,
    SlowStart,
    CrashAfter(u64),
    ExitImmediately,
    Degraded,
    NoUrl,
    IgnoreSigterm,
}

impl Scenario {
    fn from_env() -> Self {
        let raw = std::env::var("FAKE_CFD_SCENARIO").unwrap_or_default();
        match raw.split_once(':') {
            Some(("crash_after", ms)) => Self::CrashAfter(ms.parse().unwrap_or(1000)),
            _ => match raw.as_str() {
                "slow_start" => Self::SlowStart,
                "exit_immediately" => Self::ExitImmediately,
                "degraded" => Self::Degraded,
                "no_url" => Self::NoUrl,
                "ignore_sigterm" => Self::IgnoreSigterm,
                _ => Self::Healthy,
            },
        }
    }
}

fn log(level: &str, message: &str, extra: serde_json::Value) {
    let mut line = json!({ "level": level, "message": message, "time": "2026-09-22T12:00:00Z" });
    if let (Some(line), serde_json::Value::Object(extra)) = (line.as_object_mut(), extra) {
        line.extend(extra);
    }
    eprintln!("{line}");
}

struct State {
    connected: AtomicBool,
    url: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "version") {
        println!("cloudflared version {VERSION} (built 2026-09-10T20:52:23Z) [fake]");
        return ExitCode::SUCCESS;
    }
    if args.iter().any(|a| a.starts_with("eyJ")) {
        log("fatal", "token found in argv", json!({}));
        return ExitCode::from(3);
    }

    let scenario = Scenario::from_env();
    if scenario == Scenario::ExitImmediately {
        log("fatal", "failed to start: simulated", json!({}));
        return ExitCode::from(1);
    }
    let value_after = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
    };
    let Some(metrics) = value_after("--metrics").cloned() else {
        log("fatal", "--metrics is required by the fake", json!({}));
        return ExitCode::from(2);
    };
    let is_run = args.iter().any(|a| a == "run");
    if is_run && std::env::var_os("TUNNEL_TOKEN").is_none() && value_after("--token-file").is_none()
    {
        log("fatal", "no token provided", json!({}));
        return ExitCode::from(2);
    }

    let state = Arc::new(State {
        connected: AtomicBool::new(false),
        url: (value_after("--url").is_some() && scenario != Scenario::NoUrl)
            .then(|| format!("fake-{}.trycloudflare.com", std::process::id())),
    });

    let listener = match TcpListener::bind(&metrics).await {
        Ok(listener) => listener,
        Err(err) => {
            log(
                "fatal",
                &format!("failed to bind metrics server: {err}"),
                json!({}),
            );
            return ExitCode::from(1);
        }
    };
    log("info", &format!("Version {VERSION} (fake)"), json!({}));
    log(
        "info",
        &format!("Starting metrics server on {metrics}/metrics"),
        json!({}),
    );
    tokio::spawn(serve(listener, Arc::clone(&state)));

    let connect_after = match scenario {
        Scenario::SlowStart => Some(Duration::from_secs(3)),
        Scenario::Degraded => None,
        _ => Some(Duration::from_millis(150)),
    };
    if let Some(delay) = connect_after {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            if let Some(url) = &state.url {
                log("info", &format!("|  https://{url}  |"), json!({}));
            }
            state.connected.store(true, Ordering::SeqCst);
            log(
                "info",
                "Registered tunnel connection",
                json!({ "connIndex": 0, "location": "ams01", "protocol": "quic", "event": 0 }),
            );
        });
    }

    let crash = async {
        match scenario {
            Scenario::CrashAfter(ms) => tokio::time::sleep(Duration::from_millis(ms)).await,
            _ => std::future::pending().await,
        }
    };
    tokio::select! {
        () = crash => {
            log("error", "simulated crash", json!({ "error": "boom" }));
            ExitCode::from(1)
        }
        () = terminated(scenario == Scenario::IgnoreSigterm) => {
            log("info", "Initiating graceful shutdown due to signal terminated ...", json!({}));
            tokio::time::sleep(Duration::from_millis(50)).await;
            log("info", "Tunnel server stopped", json!({}));
            ExitCode::SUCCESS
        }
    }
}

/// Resolves on SIGTERM/SIGINT (never, when told to ignore SIGTERM).
async fn terminated(ignore_sigterm: bool) {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let (Ok(mut term), Ok(mut int)) = (
            signal(SignalKind::terminate()),
            signal(SignalKind::interrupt()),
        ) else {
            return std::future::pending().await;
        };
        loop {
            tokio::select! {
                _ = term.recv() => {
                    if ignore_sigterm {
                        log("warn", "ignoring SIGTERM (scenario)", json!({}));
                    } else {
                        return;
                    }
                }
                _ = int.recv() => return,
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = ignore_sigterm;
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn serve(listener: TcpListener, state: Arc<State>) {
    loop {
        let Ok((mut socket, _)) = listener.accept().await else {
            continue;
        };
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            let mut buf = [0u8; 1024];
            let Ok(read) = socket.read(&mut buf).await else {
                return;
            };
            let request = String::from_utf8_lossy(&buf[..read]);
            let path = request.split_whitespace().nth(1).unwrap_or("/");
            let connected = state.connected.load(Ordering::SeqCst);
            let (status, body) = match path {
                "/ready" => {
                    let status = if connected { 200 } else { 503 };
                    (status, json!({ "status": status, "readyConnections": u8::from(connected), "connectorId": "00000000-0000-4000-8000-000000000000" }).to_string())
                }
                "/quicktunnel" => {
                    let host = if connected {
                        state.url.clone().unwrap_or_default()
                    } else {
                        String::new()
                    };
                    (200, json!({ "hostname": host }).to_string())
                }
                "/healthcheck" => (200, "OK\n".to_owned()),
                "/metrics" => (
                    200,
                    format!(
                        "# TYPE cloudflared_tunnel_ha_connections gauge\ncloudflared_tunnel_ha_connections {}\ncloudflared_tunnel_total_requests 7\ncloudflared_tunnel_request_errors 1\ncloudflared_tunnel_server_locations{{connection_id=\"0\",edge_location=\"ams01\"}} {}\nbuild_info{{version=\"{VERSION}\"}} 1\n",
                        u8::from(connected),
                        u8::from(connected)
                    ),
                ),
                _ => (404, "not found\n".to_owned()),
            };
            let reason = match status {
                200 => "OK",
                503 => "Service Unavailable",
                _ => "Not Found",
            };
            let response = format!(
                "HTTP/1.1 {status} {reason}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
    }
}
