//! Connector state for the CLI: probed once from the remembered metrics port, since
//! the CLI never runs connectors itself (the app or an Always-on service does).

use std::collections::HashMap;

use teitunnel_core::{Secret, engine::Connectors, runtime::ConnectorState};

/// Connectors as seen from outside the app: a snapshot of each tunnel's `/ready`.
#[derive(Debug, Default)]
pub(crate) struct ProbedConnectors {
    states: HashMap<String, ConnectorState>,
}

impl ProbedConnectors {
    /// Probes the connector of `tunnel_id` on `port` (its `/ready` endpoint).
    pub(crate) async fn probe(&mut self, tunnel_id: &str, port: Option<u16>) {
        let Some(port) = port else { return };
        let Ok(endpoints) = cloudflared::Endpoints::new(port) else {
            return;
        };
        let state = match endpoints.ready().await {
            Ok(ready) if ready.ready_connections > 0 => ConnectorState::Healthy {
                connections: ready.ready_connections,
            },
            Ok(_) => ConnectorState::Connecting,
            Err(_) => return,
        };
        self.states.insert(tunnel_id.to_owned(), state);
    }
}

const NOT_HERE: &str = "Teitunnel runs connectors, not the CLI. Open Teitunnel, or turn on Always-on, to serve this Mac's routes.";

impl Connectors for ProbedConnectors {
    fn state(&self, tunnel_id: &str) -> Option<ConnectorState> {
        self.states.get(tunnel_id).cloned()
    }

    async fn start(
        &self,
        _account: &str,
        _tunnel_id: &str,
        _token: Secret<String>,
    ) -> Result<(), String> {
        Err(NOT_HERE.to_owned())
    }

    async fn stop(&self, _tunnel_id: &str) -> Result<(), String> {
        Err(NOT_HERE.to_owned())
    }

    async fn deleted(&self, _tunnel_id: &str) {}
}
