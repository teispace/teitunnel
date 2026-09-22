//! cloudflared processes on this Mac that Teitunnel didn't start (a Homebrew service, a
//! terminal, another tool). Shown so the user knows what's running; Teitunnel can stop
//! one on request, and only after re-checking it's still that cloudflared.

use std::{collections::HashMap, time::Duration};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

/// What a foreign cloudflared is doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ForeignMode {
    /// `tunnel --url …`: a Quick Tunnel.
    QuickTunnel {
        /// The local origin.
        origin: String,
    },
    /// `tunnel run [name]`, with a token or a config.
    Named {
        /// The tunnel name or id, when given on the command line.
        tunnel: Option<String>,
        /// The config file, when given.
        config: Option<String>,
    },
    /// Something else (`access`, `proxy-dns`, …).
    Other,
}

/// A cloudflared process Teitunnel doesn't manage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ForeignConnector {
    /// Process id.
    pub pid: u32,
    /// The command line, with tokens replaced by `[redacted]`.
    pub command: String,
    /// What it's doing.
    pub mode: ForeignMode,
    /// Started by launchd (a service) rather than a terminal.
    pub service: bool,
    /// Its metrics address, when it has one.
    pub metrics: Option<String>,
    /// Edge connections reported by `/ready` (None: no metrics server answered).
    pub connections: Option<u32>,
}

fn is_cloudflared(name: &str) -> bool {
    name.eq_ignore_ascii_case("cloudflared")
}

/// Replaces anything that looks like a secret: values of `--token`/`--cred-file`-like
/// flags and long base64 blobs.
pub(crate) fn redact(args: &[String]) -> String {
    let mut out = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for arg in args {
        if hide_next {
            out.push("[redacted]".to_owned());
            hide_next = false;
            continue;
        }
        if let Some((flag, _)) = arg.split_once('=')
            && flag.contains("token")
        {
            out.push(format!("{flag}=[redacted]"));
            continue;
        }
        if arg == "--token" || arg == "-token" {
            hide_next = true;
            out.push(arg.clone());
            continue;
        }
        if arg.len() > 60 && arg.starts_with("ey") {
            out.push("[redacted]".to_owned());
            continue;
        }
        out.push(arg.clone());
    }
    out.join(" ")
}

pub(crate) fn mode(args: &[String]) -> ForeignMode {
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
            .or_else(|| {
                args.iter()
                    .find_map(|a| a.strip_prefix(&format!("{flag}=")).map(str::to_owned))
            })
    };
    if let Some(origin) = value("--url") {
        return ForeignMode::QuickTunnel { origin };
    }
    if let Some(run) = args.iter().position(|a| a == "run") {
        // The first positional argument after `run`; `--flag value` pairs are skipped
        // (a token is never mistaken for a name).
        const SWITCHES: &[&str] = &["--no-autoupdate", "--no-tls-verify", "--post-quantum"];
        let mut rest = args[run + 1..].iter();
        let mut tunnel = None;
        while let Some(arg) = rest.next() {
            if arg.starts_with('-') {
                if !arg.contains('=') && !SWITCHES.contains(&arg.as_str()) {
                    rest.next();
                }
            } else {
                tunnel = Some(arg.clone());
                break;
            }
        }
        return ForeignMode::Named {
            tunnel,
            config: value("--config"),
        };
    }
    if args.iter().any(|a| a == "tunnel") && value("--config").is_some() {
        return ForeignMode::Named {
            tunnel: None,
            config: value("--config"),
        };
    }
    ForeignMode::Other
}

async fn ready_connections(addr: &str) -> Option<u32> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(600))
        .build()
        .ok()?;
    let body: serde_json::Value = client
        .get(format!("http://{addr}/ready"))
        .send()
        .await
        .ok()?
        .json()
        .await
        .ok()?;
    body.get("readyConnections")?
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
}

/// Running cloudflared processes that aren't Teitunnel's children.
pub async fn foreign() -> Vec<ForeignConnector> {
    let found = tokio::task::spawn_blocking(scan).await.unwrap_or_default();
    let mut out = Vec::with_capacity(found.len());
    for mut connector in found {
        if let Some(addr) = &connector.metrics {
            connector.connections = ready_connections(addr).await;
        }
        out.push(connector);
    }
    out
}

fn scan() -> Vec<ForeignConnector> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_cmd(UpdateKind::Always),
    );
    let own = Pid::from_u32(std::process::id());
    // Listening ports by pid, for processes without an explicit --metrics.
    let mut ports: HashMap<u32, Vec<u16>> = HashMap::new();
    if let Ok(listeners) = listeners::get_all() {
        for l in listeners {
            ports
                .entry(l.process.pid)
                .or_default()
                .push(l.socket.port());
        }
    }
    system
        .processes()
        .values()
        .filter(|p| is_cloudflared(&p.name().to_string_lossy()))
        .filter(|p| p.parent() != Some(own))
        .map(|p| {
            let args: Vec<String> = p
                .cmd()
                .iter()
                .map(|a| a.to_string_lossy().into_owned())
                .collect();
            let pid = p.pid().as_u32();
            let explicit = args
                .iter()
                .position(|a| a == "--metrics")
                .and_then(|i| args.get(i + 1))
                .cloned();
            let metrics = explicit.or_else(|| {
                ports
                    .get(&pid)
                    .and_then(|list| list.iter().min())
                    .map(|port| format!("127.0.0.1:{port}"))
            });
            ForeignConnector {
                pid,
                command: redact(&args),
                mode: mode(&args),
                service: p.parent().is_some_and(|parent| parent.as_u32() == 1),
                metrics,
                connections: None,
            }
        })
        .collect()
}

/// Stops a foreign cloudflared, after checking the pid still is a cloudflared that isn't
/// ours (pids get reused). Returns whether it was stopped.
pub async fn stop(pid: u32) -> bool {
    let still_foreign = tokio::task::spawn_blocking(move || scan().iter().any(|c| c.pid == pid))
        .await
        .unwrap_or(false);
    if !still_foreign {
        return false;
    }
    crate::runtime::stop_foreign(pid).await;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn classifies_command_lines() {
        assert_eq!(
            mode(&args("cloudflared tunnel --url http://localhost:3000")),
            ForeignMode::QuickTunnel {
                origin: "http://localhost:3000".into()
            }
        );
        assert_eq!(
            mode(&args(
                "cloudflared tunnel --config /etc/cloudflared/config.yml run home"
            )),
            ForeignMode::Named {
                tunnel: Some("home".into()),
                config: Some("/etc/cloudflared/config.yml".into())
            }
        );
        assert_eq!(
            mode(&args("cloudflared tunnel run --token eyJhIjoi")),
            ForeignMode::Named {
                tunnel: None,
                config: None
            }
        );
        assert_eq!(
            mode(&args("cloudflared access tcp --hostname x")),
            ForeignMode::Other
        );
    }

    #[test]
    fn never_shows_tokens() {
        let long = format!("ey{}", "A".repeat(80));
        let line = redact(&args(&format!(
            "cloudflared tunnel run --token {long} --token=abc {long}"
        )));
        assert!(!line.contains("AAAA"), "{line}");
        assert!(!line.contains("abc"), "{line}");
        assert_eq!(
            line,
            "cloudflared tunnel run --token [redacted] --token=[redacted] [redacted]"
        );
    }
}
