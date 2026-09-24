//! Connector state for the CLI: probed once from the remembered metrics port, since
//! the CLI never runs connectors itself (the app or an Always-on service does).

use std::collections::HashMap;
use teitunnel_core::text::{Text, msg};

use teitunnel_core::{Secret, engine::Connectors, runtime::ConnectorState};

/// Connectors as seen from outside the app: a snapshot of each tunnel's `/ready`.
#[derive(Debug, Default)]
pub(crate) struct ProbedConnectors {
    states: HashMap<String, ConnectorState>,
    ids: HashMap<String, String>,
}

impl ProbedConnectors {
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
