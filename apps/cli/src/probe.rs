//! Connector state for the CLI: probed once from the remembered metrics port, since
//! the CLI never runs connectors itself (the app or an Always-on service does).

use std::{collections::HashMap, path::Path};
use teitunnel_core::text::{Text, msg};

use teitunnel_core::{
    Secret,
    engine::{Connectors, Local},
    runtime::ConnectorState,
};

use crate::app::{Where, connect};

/// Connectors as seen from outside the app: a snapshot of each tunnel's `/ready`.
#[derive(Debug, Default)]
pub(crate) struct ProbedConnectors {
    states: HashMap<String, ConnectorState>,
    ids: HashMap<String, String>,
}

impl ProbedConnectors {
    /// This machine's connectors for `accounts`, each probed where it runs: on the port
    /// the running app reports, else on the one last remembered. The two differ when
    /// the remembered port was busy as the app started its connector.
    pub(crate) async fn of(dir: &Path, local: &Local, accounts: &[String]) -> Self {
        let running = app_ports(dir).await;
        let mut connectors = Self::default();
        for account in accounts {
            for tunnel in local.tunnels(account).await.unwrap_or_default() {
                let port = running
                    .get(&tunnel.tunnel_id)
                    .copied()
                    .or(tunnel.metrics_port);
                connectors.probe(&tunnel.tunnel_id, port).await;
            }
        }
        connectors
    }

    /// Probes the connector of `tunnel_id` on `port` (its `/ready` endpoint).
    pub(crate) async fn probe(&mut self, tunnel_id: &str, port: Option<u16>) {
        let Some(port) = port else { return };
        let Ok(endpoints) = cloudflared::Endpoints::new(port) else {
            return;
        };
        let Ok(ready) = endpoints.ready().await else {
            return;
        };
        let state = if ready.ready_connections > 0 {
            ConnectorState::Healthy {
                connections: ready.ready_connections,
            }
        } else {
            ConnectorState::Connecting
        };
        self.states.insert(tunnel_id.to_owned(), state);
        if let Some(id) = ready.connector_id {
            self.ids.insert(tunnel_id.to_owned(), id);
        }
    }
}

/// The metrics ports of the running app's connectors, by tunnel id; empty when the app
/// doesn't run (or is too old to say).
async fn app_ports(dir: &Path) -> HashMap<String, u16> {
    let Ok(Some(client)) = connect(dir, Where::Auto).await else {
        return HashMap::new();
    };
    client
        .status()
        .await
        .map(|status| {
            status
                .tunnels
                .into_iter()
                .filter_map(|t| Some((t.id, t.metrics_port?)))
                .collect()
        })
        .unwrap_or_default()
}

const NOT_HERE: &str = "Teitunnel runs connectors, not the CLI. Open Teitunnel, run `teitunnel up`, or turn on Always-on to serve this machine's routes.";

impl Connectors for ProbedConnectors {
    fn state(&self, tunnel_id: &str) -> Option<ConnectorState> {
        self.states.get(tunnel_id).cloned()
    }

    async fn start(
        &self,
        _account: &str,
        _tunnel_id: &str,
        _token: Secret<String>,
    ) -> Result<(), Text> {
        Err(msg::raw(NOT_HERE))
    }

    /// Nothing to stop when no connector of this machine answers (deleting a tunnel
    /// from a CI job after its share ended); one that runs belongs to the app.
    async fn stop(&self, tunnel_id: &str) -> Result<(), Text> {
        if self.states.contains_key(tunnel_id) {
            Err(msg::raw(NOT_HERE))
        } else {
            Ok(())
        }
    }

    async fn deleted(&self, _tunnel_id: &str) {}

    async fn connector_id(&self, tunnel_id: &str) -> Option<String> {
        self.ids.get(tunnel_id).cloned()
    }
}

#[cfg(test)]
mod tests {
    use teitunnel_control::{Limits, protocol::TunnelInfo, testing};
    use teitunnel_core::store::Store;
    use tokio::{
        io::{AsyncReadExt as _, AsyncWriteExt as _},
        net::TcpListener,
    };

    use super::*;

    /// A connector's `/ready`, healthy, with connector id `id`.
    async fn ready(id: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut request = [0u8; 1024];
                let _ = socket.read(&mut request).await;
                let body = format!(r#"{{"status":200,"readyConnections":4,"connectorId":"{id}"}}"#);
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        port
    }

    #[tokio::test]
    async fn finds_the_apps_connector_where_the_app_says_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        let local = Local::new(Store::open_in_memory().unwrap());
        local.set_machine_tunnel("a1", "t1", "Mac").await.unwrap();
        // The remembered port: nothing answers there.
        let gone = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        local.set_metrics_port("t1", gone).await.unwrap();
        let accounts = ["a1".to_owned()];
        let alone = ProbedConnectors::of(dir.path(), &local, &accounts).await;
        assert!(!alone.is_running("t1"));

        let port = ready("c-app").await;
        let app = testing::serve(dir.path(), Limits::default()).await.unwrap();
        app.host.tunnels.lock().unwrap().push(TunnelInfo {
            account_id: "a1".into(),
            id: "t1".into(),
            name: "Mac".into(),
            is_default: true,
            state: "healthy".into(),
            metrics_port: Some(port),
        });
        let connectors = ProbedConnectors::of(dir.path(), &local, &accounts).await;
        assert!(connectors.is_running("t1"));
        assert_eq!(
            connectors.connector_id("t1").await.as_deref(),
            Some("c-app")
        );
    }
}
