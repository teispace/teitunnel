//! Every tunnel in an account, for the Tunnels view (Advanced).

use futures_util::{StreamExt, stream};
use serde::Serialize;

use super::{
    cloud::{CloudApi, Connectors},
    local::Local,
    observe::ObserveError,
};
use crate::runtime::ConnectorState;

/// One edge connection of a connector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    /// Edge location, e.g. `ams01`.
    pub colo: String,
    /// When it connected (RFC 3339).
    pub opened_at: String,
}

/// One machine running a tunnel (a cloudflared process), with its edge connections.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ConnectorView {
    /// Connector id.
    pub id: String,
    /// cloudflared version.
    pub version: String,
    /// Public IP it connects from.
    pub origin_ip: String,
    /// It's this Mac's connector.
    pub this_mac: bool,
    /// Edge connections, by location.
    pub connections: Vec<ConnectionView>,
}

/// Groups a tunnel's edge connections by connector: this Mac's first, then the
/// longest-connected.
fn connectors_of(connections: Vec<cf_api::Connection>, local: Option<&str>) -> Vec<ConnectorView> {
    let mut connectors: Vec<ConnectorView> = Vec::new();
    for conn in connections {
        let view = ConnectionView {
            colo: conn.colo_name,
            opened_at: conn.opened_at,
        };
        match connectors.iter_mut().find(|c| c.id == conn.client_id) {
            Some(connector) => connector.connections.push(view),
            None => connectors.push(ConnectorView {
                this_mac: local == Some(conn.client_id.as_str()),
                id: conn.client_id,
                version: conn.client_version,
                origin_ip: conn.origin_ip,
                connections: vec![view],
            }),
        }
    }
    for connector in &mut connectors {
        connector
            .connections
            .sort_by(|a, b| (&a.opened_at, &a.colo).cmp(&(&b.opened_at, &b.colo)));
    }
    connectors.sort_by(|a, b| {
        let first = |c: &ConnectorView| c.connections.first().map(|c| c.opened_at.clone());
        (!a.this_mac, first(a), &a.id).cmp(&(!b.this_mac, first(b), &b.id))
    });
    connectors
}

/// A tunnel in the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TunnelSummary {
    /// Tunnel id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Cloudflare's status: `inactive`, `degraded`, `healthy` or `down`.
    pub status: String,
    /// Created (RFC 3339).
    pub created_at: String,
    /// Routes in its remote configuration (None: configured locally, or unreadable).
    pub routes: Option<u32>,
    /// Machines running it.
    pub connectors: Vec<ConnectorView>,
    /// This Mac's tunnel (created and run by Teitunnel here).
    pub this_mac: bool,
    /// Connector state on this Mac, for this Mac's tunnel.
    pub connector: Option<ConnectorState>,
}

pub(crate) async fn list<C: CloudApi, K: Connectors>(
    api: &C,
    connectors: &K,
    local: &Local,
    account: &str,
) -> Result<Vec<TunnelSummary>, ObserveError> {
    let ours = local.machine_tunnel(account).await?.map(|t| t.tunnel_id);
    let mut tunnels = api.tunnels(account).await?;
    tunnels.sort_by(|a, b| {
        (ours.as_ref() != Some(&a.id), &a.name).cmp(&(ours.as_ref() != Some(&b.id), &b.name))
    });
    let summaries = stream::iter(tunnels)
        .map(|tunnel| {
            let ours = ours.clone();
            async move {
                let routes = if tunnel.remote_config {
                    api.tunnel_config(account, &tunnel.id).await.ok().map(|c| {
                        let count = c.config.map_or(0, |c| {
                            c.ingress.iter().filter(|r| r.hostname.is_some()).count()
                        });
                        u32::try_from(count).unwrap_or(u32::MAX)
                    })
                } else {
                    None
                };
                let this_mac = ours.as_deref() == Some(tunnel.id.as_str());
                let local = if this_mac && !tunnel.connections.is_empty() {
                    connectors.connector_id(&tunnel.id).await
                } else {
                    None
                };
                TunnelSummary {
                    connector: this_mac.then(|| connectors.state(&tunnel.id)).flatten(),
                    connectors: connectors_of(tunnel.connections, local.as_deref()),
                    id: tunnel.id,
                    name: tunnel.name,
                    status: tunnel.status,
                    created_at: tunnel.created_at,
                    routes,
                    this_mac,
                }
            }
        })
        .buffered(4)
        .collect()
        .await;
    Ok(summaries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(client: &str, colo: &str, opened: &str) -> cf_api::Connection {
        cf_api::Connection {
            colo_name: colo.into(),
            client_id: client.into(),
            client_version: "2026.9.1".into(),
            origin_ip: format!("198.51.100.{}", client.len()),
            opened_at: opened.into(),
            is_pending_reconnect: false,
        }
    }

    #[test]
    fn groups_connections_by_machine_with_this_mac_first() {
        let connectors = connectors_of(
            vec![
                conn("server", "fra01", "2026-09-01T00:00:00Z"),
                conn("mac", "ams02", "2026-09-23T00:00:02Z"),
                conn("server", "ams01", "2026-09-01T00:00:01Z"),
                conn("mac", "ams01", "2026-09-23T00:00:01Z"),
            ],
            Some("mac"),
        );
        assert_eq!(
            connectors
                .iter()
                .map(|c| (c.id.as_str(), c.this_mac))
                .collect::<Vec<_>>(),
            [("mac", true), ("server", false)]
        );
        assert_eq!(
            connectors[0]
                .connections
                .iter()
                .map(|c| c.colo.as_str())
                .collect::<Vec<_>>(),
            ["ams01", "ams02"],
            "in the order they connected"
        );
        // Unknown local id: nobody is this Mac, the longest-connected comes first.
        let unknown = connectors_of(
            vec![
                conn("b", "x", "2026-09-02T00:00:00Z"),
                conn("a", "y", "2026-09-01T00:00:00Z"),
            ],
            None,
        );
        assert_eq!(unknown[0].id, "a");
        assert!(unknown.iter().all(|c| !c.this_mac));
    }
}
