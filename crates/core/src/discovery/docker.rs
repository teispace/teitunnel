//! Running Docker containers and their published ports, read from the Docker Engine
//! API on its Unix socket (Docker Desktop, OrbStack, Colima, Podman). No daemon, no
//! containers: discovery just has nothing to add.

use std::{path::PathBuf, time::Duration};

use serde::Deserialize;

/// How long to wait for the daemon (discovery must stay snappy).
const TIMEOUT: Duration = Duration::from_millis(800);

/// A running container with ports published on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Container {
    /// Container name, without the leading slash.
    pub name: String,
    /// Image, e.g. `postgres:17`.
    pub image: String,
    /// Compose project and service, when started by Compose.
    pub compose: Option<(String, String)>,
    /// Host ports published on loopback or all interfaces.
    pub ports: Vec<u16>,
}

impl Container {
    /// What to call it in a picker: `project/service` for Compose, else the name.
    pub fn label(&self) -> String {
        self.compose.as_ref().map_or_else(
            || self.name.clone(),
            |(project, service)| format!("{project}/{service}"),
        )
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct ApiContainer {
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    image: String,
    #[serde(default)]
    ports: Vec<ApiPort>,
    #[serde(default)]
    labels: std::collections::HashMap<String, String>,
}

#[derive(Deserialize)]
struct ApiPort {
    #[serde(rename = "IP", default)]
    ip: String,
    #[serde(rename = "PublicPort", default)]
    public_port: Option<u16>,
    #[serde(rename = "Type", default)]
    kind: String,
}

/// Parses `GET /containers/json`.
pub(crate) fn parse(body: &str) -> Vec<Container> {
    let Ok(list) = serde_json::from_str::<Vec<ApiContainer>>(body) else {
        return Vec::new();
    };
    list.into_iter()
        .filter_map(|c| {
            let mut ports: Vec<u16> = c
                .ports
                .iter()
                .filter(|p| {
                    p.kind == "tcp"
                        && matches!(p.ip.as_str(), "" | "0.0.0.0" | "::" | "127.0.0.1" | "::1")
                })
                .filter_map(|p| p.public_port)
                .collect();
            ports.sort_unstable();
            ports.dedup();
            if ports.is_empty() {
                return None;
            }
            let project = c.labels.get("com.docker.compose.project").cloned();
            let service = c.labels.get("com.docker.compose.service").cloned();
            Some(Container {
                name: c
                    .names
                    .first()
                    .map(|n| n.trim_start_matches('/').to_owned())?,
                image: c.image,
                compose: project.zip(service),
                ports,
            })
        })
        .collect()
}

/// Candidate daemon sockets, most specific first.
fn sockets() -> Vec<PathBuf> {
    let mut list = Vec::new();
    if let Some(host) = std::env::var("DOCKER_HOST")
        .ok()
        .and_then(|h| h.strip_prefix("unix://").map(PathBuf::from))
    {
        list.push(host);
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        for rel in [
            ".docker/run/docker.sock",
            ".orbstack/run/docker.sock",
            ".colima/default/docker.sock",
            ".local/share/containers/podman/machine/podman.sock",
        ] {
            list.push(home.join(rel));
        }
    }
    list.push(PathBuf::from("/var/run/docker.sock"));
    list
}

/// Running containers with published ports, from the first daemon that answers.
pub async fn containers() -> Vec<Container> {
    for socket in sockets() {
        if !socket.exists() {
            continue;
        }
        if let Ok(Some(body)) =
            tokio::time::timeout(TIMEOUT, get(&socket, "/containers/json")).await
        {
            return parse(&body);
        }
    }
    Vec::new()
}

/// A minimal HTTP/1.0 GET over a Unix socket (no chunked encoding to deal with).
#[cfg(unix)]
async fn get(socket: &std::path::Path, path: &str) -> Option<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut stream = tokio::net::UnixStream::connect(socket).await.ok()?;
    stream
        .write_all(format!("GET {path} HTTP/1.0\r\nHost: docker\r\n\r\n").as_bytes())
        .await
        .ok()?;
    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.ok()?;
    let text = String::from_utf8(response).ok()?;
    let (head, body) = text.split_once("\r\n\r\n")?;
    head.starts_with("HTTP/1.")
        .then(|| head.split_whitespace().nth(1) == Some("200"))
        .filter(|ok| *ok)
        .map(|_| body.to_owned())
}

#[cfg(not(unix))]
async fn get(_socket: &std::path::Path, _path: &str) -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"[
      {"Names":["/shop-db-1"],"Image":"postgres:17","Ports":[{"IP":"0.0.0.0","PrivatePort":5432,"PublicPort":5433,"Type":"tcp"},{"IP":"::","PrivatePort":5432,"PublicPort":5433,"Type":"tcp"}],
       "Labels":{"com.docker.compose.project":"shop","com.docker.compose.service":"db"}},
      {"Names":["/web"],"Image":"nginx","Ports":[{"IP":"127.0.0.1","PrivatePort":80,"PublicPort":8088,"Type":"tcp"},{"PrivatePort":443,"Type":"tcp"}],"Labels":{}},
      {"Names":["/worker"],"Image":"busybox","Ports":[],"Labels":{}},
      {"Names":["/dns"],"Image":"coredns","Ports":[{"IP":"0.0.0.0","PrivatePort":53,"PublicPort":1053,"Type":"udp"}],"Labels":{}}
    ]"#;

    #[test]
    fn keeps_containers_with_published_tcp_ports() {
        let list = parse(FIXTURE);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].label(), "shop/db");
        assert_eq!(list[0].ports, [5433]);
        assert_eq!(list[1].label(), "web");
        assert_eq!(list[1].ports, [8088]);
        assert!(parse("not json").is_empty());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn reads_the_daemon_over_its_socket() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("docker.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 512];
            let n = stream.read(&mut buf).await.unwrap();
            assert!(
                String::from_utf8_lossy(&buf[..n]).starts_with("GET /containers/json HTTP/1.0")
            );
            let body = FIXTURE;
            let _ = stream
                .write_all(
                    format!("HTTP/1.0 200 OK\r\nContent-Type: application/json\r\n\r\n{body}")
                        .as_bytes(),
                )
                .await;
        });
        let body = get(&path, "/containers/json").await.unwrap();
        assert_eq!(parse(&body).len(), 2);
    }
}
