//! Every tunnel in an account, for the Tunnels view (Advanced).

use futures_util::{StreamExt, stream};
use serde::Serialize;

use super::{
    cloud::{CloudApi, Connectors},
    local::Local,
    observe::ObserveError,
};
use crate::runtime::ConnectorState;

/// One connector connected to the edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ConnectionView {
    /// Edge location, e.g. `ams01`.
    pub colo: String,
    /// cloudflared version.
    pub version: String,
    /// Public IP it connects from.
    pub origin_ip: String,
    /// When it connected (RFC 3339).
    pub opened_at: String,
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
    /// Edge connections.
    pub connections: Vec<ConnectionView>,
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
                TunnelSummary {
                    connector: this_mac.then(|| connectors.state(&tunnel.id)).flatten(),
                    connections: tunnel
                        .connections
                        .into_iter()
                        .map(|c| ConnectionView {
                            colo: c.colo_name,
                            version: c.client_version,
                            origin_ip: c.origin_ip,
                            opened_at: c.opened_at,
                        })
                        .collect(),
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
