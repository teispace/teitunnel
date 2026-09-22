//! Discovery of local services (listening ports, processes) that can be shared or
//! routed, and later of Docker containers and existing cloudflared setups (M4).
//!
//! Discovery is a snapshot, taken when a picker opens (and every few seconds while it
//! stays open). It never runs as a background loop.

mod classify;

use std::{
    collections::BTreeMap,
    net::IpAddr,
    path::{Path, PathBuf},
};

use serde::Serialize;
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

pub use classify::ServiceKind;

/// A TCP port something on this machine is listening on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LocalService {
    /// The port.
    pub port: u16,
    /// Whether it listens on all interfaces (vs. loopback only).
    pub all_interfaces: bool,
    /// Owning process id.
    pub pid: u32,
    /// Process name, e.g. `node`.
    pub process: String,
    /// What it looks like, e.g. a Vite dev server.
    pub kind: ServiceKind,
    /// Project the process runs in (from its working directory), e.g. `my-app`.
    pub project: Option<String>,
    /// Suggested origin URL, e.g. `http://localhost:5173`.
    pub origin: String,
}

/// Ports used by cloudflared metrics servers (ours and cloudflared's defaults).
fn is_cloudflared_metrics(port: u16) -> bool {
    (20241..=20245).contains(&port) || (20300..20500).contains(&port)
}

/// Lists listening TCP services, one entry per port, likely dev servers first.
/// Blocking: call it from `spawn_blocking`.
pub fn list_services() -> Vec<LocalService> {
    let listeners = match listeners::get_all() {
        Ok(listeners) => listeners,
        Err(err) => {
            tracing::warn!(error = %err, "couldn't list listening sockets");
            return Vec::new();
        }
    };
    let own_pid = std::process::id();
    let mut by_port: BTreeMap<u16, (listeners::Listener, bool)> = BTreeMap::new();
    for listener in listeners {
        if listener.protocol != listeners::Protocol::TCP
            || listener.state != listeners::SocketState::Listen
            || listener.process.pid == own_pid
            || is_cloudflared_metrics(listener.socket.port())
        {
            continue;
        }
        let all_interfaces = listener.socket.ip().is_unspecified();
        by_port
            .entry(listener.socket.port())
            .and_modify(|(_, any)| *any |= all_interfaces)
            .or_insert((listener, all_interfaces));
    }

    let pids: Vec<Pid> = by_port
        .values()
        .map(|(l, _)| Pid::from_u32(l.process.pid))
        .collect();
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&pids),
        true,
        ProcessRefreshKind::nothing()
            .with_cmd(UpdateKind::Always)
            .with_cwd(UpdateKind::Always),
    );

    let mut services: Vec<LocalService> = by_port
        .into_iter()
        .map(|(port, (listener, all_interfaces))| {
            let process = system.process(Pid::from_u32(listener.process.pid));
            let cmd: Vec<String> = process
                .map(|p| {
                    p.cmd()
                        .iter()
                        .map(|a| a.to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            let kind = classify::classify(&listener.process.name, &cmd, port);
            let project = process.and_then(|p| p.cwd()).and_then(project_name);
            LocalService {
                port,
                all_interfaces,
                pid: listener.process.pid,
                process: listener.process.name,
                kind,
                project,
                origin: kind.origin(port),
            }
        })
        .collect();
    services.sort_by_key(|service| (service.kind.rank(), service.port));
    services
}

/// Names the project a process runs in: `package.json` or `Cargo.toml` name, else the
/// directory name. Home and root directories aren't projects.
fn project_name(cwd: &Path) -> Option<String> {
    if cwd.parent().is_none() || Some(cwd.to_path_buf()) == home_dir() {
        return None;
    }
    manifest_name(cwd).or_else(|| cwd.file_name().map(|n| n.to_string_lossy().into_owned()))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn manifest_name(dir: &Path) -> Option<String> {
    if let Ok(text) = std::fs::read_to_string(dir.join("package.json"))
        && let Ok(json) = serde_json::from_str::<serde_json::Value>(&text)
        && let Some(name) = json.get("name").and_then(|n| n.as_str())
    {
        return Some(name.to_owned());
    }
    let text = std::fs::read_to_string(dir.join("Cargo.toml")).ok()?;
    let mut in_package = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            in_package = line == "[package]";
        } else if in_package && let Some(value) = line.strip_prefix("name") {
            return Some(
                value
                    .trim_start_matches([' ', '='])
                    .trim()
                    .trim_matches('"')
                    .to_owned(),
            );
        }
    }
    None
}

/// `true` if `address` is a loopback or unspecified address, i.e. reachable as
/// `localhost` from this machine.
pub fn is_local(address: IpAddr) -> bool {
    address.is_loopback() || address.is_unspecified()
}

#[cfg(test)]
mod tests {
    use std::net::TcpListener;

    use super::*;

    #[test]
    fn finds_a_listening_socket() {
        // Our own pid is excluded, so spawn a child that listens.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut child = std::process::Command::new("python3")
            .args([
                "-m",
                "http.server",
                &port.to_string(),
                "--bind",
                "127.0.0.1",
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        let Ok(child) = child.as_mut() else { return }; // python3 unavailable: skip
        let mut found = None;
        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            found = list_services().into_iter().find(|s| s.port == port);
            if found.is_some() {
                break;
            }
        }
        let _ = child.kill();
        let _ = child.wait();
        let service = found.expect("python http.server is listed");
        assert_eq!(service.kind, ServiceKind::Python);
        assert_eq!(service.origin, format!("http://localhost:{port}"));
        assert!(!service.all_interfaces);
    }

    #[test]
    fn project_names_from_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("web");
        std::fs::create_dir(&app).unwrap();
        assert_eq!(project_name(&app).as_deref(), Some("web"));
        std::fs::write(
            app.join("Cargo.toml"),
            "[workspace]\nname = \"no\"\n[package]\nname = \"api-server\"\n",
        )
        .unwrap();
        assert_eq!(project_name(&app).as_deref(), Some("api-server"));
        std::fs::write(app.join("package.json"), r#"{"name":"my-app"}"#).unwrap();
        assert_eq!(project_name(&app).as_deref(), Some("my-app"));
        assert_eq!(project_name(Path::new("/")), None);
    }

    #[test]
    fn excludes_metrics_ports() {
        assert!(is_cloudflared_metrics(20241));
        assert!(is_cloudflared_metrics(20399));
        assert!(!is_cloudflared_metrics(3000));
    }
}
