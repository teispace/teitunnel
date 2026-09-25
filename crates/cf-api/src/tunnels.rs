//! Cloudflare Tunnel (cfd_tunnel) endpoints and remote configuration.
//!
//! Configuration models keep every field they don't know in `extra`, so writing a
//! config back never drops settings someone made in the dashboard.

use futures_util::{StreamExt, TryStreamExt, stream};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{Client, Result};

/// A connector connection to the edge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    /// Edge location, e.g. `ams01`.
    #[serde(default)]
    pub colo_name: String,
    /// Connector id.
    #[serde(default)]
    pub client_id: String,
    /// cloudflared version.
    #[serde(default)]
    pub client_version: String,
    /// Public IP the connector connects from.
    #[serde(default)]
    pub origin_ip: String,
    /// When it connected (RFC 3339).
    #[serde(default)]
    pub opened_at: String,
    /// Whether it's reconnecting.
    #[serde(default)]
    pub is_pending_reconnect: bool,
}

/// A named tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tunnel {
    /// Tunnel id (UUID).
    pub id: String,
    /// Name.
    pub name: String,
    /// `inactive`, `degraded`, `healthy` or `down`.
    #[serde(default)]
    pub status: String,
    /// Creation time (RFC 3339).
    #[serde(default)]
    pub created_at: String,
    /// Set when deleted.
    #[serde(default)]
    pub deleted_at: Option<String>,
    /// Whether the configuration is managed remotely (in Cloudflare).
    #[serde(default)]
    pub remote_config: bool,
    /// Active connections.
    #[serde(default)]
    pub connections: Vec<Connection>,
}

/// One ingress rule. The last rule must be a catch-all (no hostname).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IngressRule {
    /// Public hostname; `None` for the catch-all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    /// Path regex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Origin service, e.g. `http://localhost:3000` or `http_status:404`.
    pub service: String,
    /// Per-rule origin settings (kept as-is).
    #[serde(
        rename = "originRequest",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub origin_request: Map<String, Value>,
    /// Fields we don't model.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A tunnel's remote configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TunnelConfig {
    /// Ordered ingress rules.
    #[serde(default)]
    pub ingress: Vec<IngressRule>,
    /// Tunnel-wide origin settings (kept as-is).
    #[serde(
        rename = "originRequest",
        default,
        skip_serializing_if = "Map::is_empty"
    )]
    pub origin_request: Map<String, Value>,
    /// Fields we don't model (`warp-routing`, …).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// A configuration together with its version.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct VersionedConfig {
    /// Monotonic version; a higher version than we last wrote means someone else edited it.
    #[serde(default)]
    pub version: u64,
    /// The configuration (absent for a tunnel that was never configured).
    #[serde(default)]
    pub config: Option<TunnelConfig>,
}

fn tunnels_path(account: &str) -> String {
    format!("/accounts/{}/cfd_tunnel", crate::encode(account))
}

fn tunnel_path(account: &str, tunnel: &str) -> String {
    format!("{}/{}", tunnels_path(account), crate::encode(tunnel))
}

impl Client {
    /// Tunnels in an account that aren't deleted.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn tunnels(&self, account: &str) -> Result<Vec<Tunnel>> {
        let tunnels: Vec<Tunnel> = self
            .get_all(&format!("{}?is_deleted=false", tunnels_path(account)))
            .await?;
        stream::iter(tunnels)
            .map(|tunnel| self.with_connections(account, tunnel))
            .buffered(4)
            .try_collect()
            .await
    }

    /// One tunnel.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn tunnel(&self, account: &str, tunnel: &str) -> Result<Tunnel> {
        let tunnel = self.get(&tunnel_path(account, tunnel)).await?;
        self.with_connections(account, tunnel).await
    }

    /// Fills in `connections`, which the tunnel endpoints stop returning on 2026-10-05
    /// (they move to `…/cfd_tunnel/{id}/connections`), for tunnels Cloudflare reports as
    /// connected; the others have none.
    async fn with_connections(&self, account: &str, mut tunnel: Tunnel) -> Result<Tunnel> {
        if tunnel.connections.is_empty() && matches!(tunnel.status.as_str(), "healthy" | "degraded")
        {
            tunnel.connections = self
                .tunnel_connectors(account, &tunnel.id)
                .await?
                .into_iter()
                .flat_map(|connector| {
                    connector.conns.into_iter().map(move |conn| Connection {
                        colo_name: conn.colo_name,
                        client_id: connector.id.clone(),
                        client_version: connector.version.clone(),
                        origin_ip: conn.origin_ip,
                        opened_at: conn.opened_at,
                        is_pending_reconnect: conn.is_pending_reconnect,
                    })
                })
                .collect();
        }
        Ok(tunnel)
    }

    /// Creates a remotely-managed tunnel. Not retried on server errors (no duplicates).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn create_tunnel(&self, account: &str, name: &str) -> Result<Tunnel> {
        self.post(
            &tunnels_path(account),
            &serde_json::json!({ "name": name, "config_src": "cloudflare" }),
        )
        .await
    }

    /// Renames a tunnel.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn rename_tunnel(&self, account: &str, tunnel: &str, name: &str) -> Result<Tunnel> {
        self.patch(
            &tunnel_path(account, tunnel),
            &serde_json::json!({ "name": name }),
        )
        .await
    }

    /// Deletes a tunnel (it must have no active connections).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_tunnel(&self, account: &str, tunnel: &str) -> Result<()> {
        self.delete(&tunnel_path(account, tunnel)).await
    }

    /// Removes stale connections of a tunnel.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn clean_connections(&self, account: &str, tunnel: &str) -> Result<()> {
        self.delete(&format!("{}/connections", tunnel_path(account, tunnel)))
            .await
    }

    /// The token a connector runs the tunnel with. Treat it as a secret.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn tunnel_token(&self, account: &str, tunnel: &str) -> Result<String> {
        self.get(&format!("{}/token", tunnel_path(account, tunnel)))
            .await
    }

    /// The remote configuration and its version.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn tunnel_config(&self, account: &str, tunnel: &str) -> Result<VersionedConfig> {
        self.get(&format!("{}/configurations", tunnel_path(account, tunnel)))
            .await
    }

    /// Replaces the remote configuration; returns the new version.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn put_tunnel_config(
        &self,
        account: &str,
        tunnel: &str,
        config: &TunnelConfig,
    ) -> Result<VersionedConfig> {
        let body = serde_json::json!({ "config": config });
        self.put(
            &format!("{}/configurations", tunnel_path(account, tunnel)),
            &body,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"{
      "tunnel_id": "t1", "version": 7, "source": "cloudflare",
      "config": {
        "ingress": [
          {"hostname": "app.xyz.com", "service": "http://localhost:3000", "originRequest": {"noTLSVerify": true}},
          {"hostname": "api.xyz.com", "path": "^/v1/", "service": "http://localhost:8080", "futureField": 1},
          {"service": "http_status:404"}
        ],
        "originRequest": {"connectTimeout": 10},
        "warp-routing": {"enabled": false}
      }
    }"#;

    #[test]
    fn keeps_unknown_fields_on_round_trip() {
        let versioned: VersionedConfig = serde_json::from_str(CONFIG).unwrap();
        assert_eq!(versioned.version, 7);
        let config = versioned.config.unwrap();
        assert_eq!(config.ingress.len(), 3);
        assert_eq!(config.ingress[0].origin_request["noTLSVerify"], true);
        assert_eq!(config.ingress[1].extra["futureField"], 1);

        let written = serde_json::to_value(&config).unwrap();
        assert_eq!(written["warp-routing"]["enabled"], false);
        assert_eq!(written["originRequest"]["connectTimeout"], 10);
        assert_eq!(written["ingress"][1]["futureField"], 1);
        assert!(
            written["ingress"][2].get("hostname").is_none(),
            "catch-all stays hostname-less"
        );
        assert!(written["ingress"][2].get("originRequest").is_none());
    }

    #[test]
    fn unconfigured_tunnels_decode() {
        let versioned: VersionedConfig =
            serde_json::from_str(r#"{"version": 0, "config": null}"#).unwrap();
        assert!(versioned.config.is_none());
    }

    #[tokio::test]
    async fn reads_connections_from_their_own_endpoint() {
        use wiremock::{
            Mock, MockServer, ResponseTemplate,
            matchers::{method, path},
        };
        let ok = |result: Value| {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true, "errors": [], "messages": [], "result": result,
                "result_info": {"page": 1, "per_page": 50, "count": 2, "total_count": 2, "total_pages": 1}
            }))
        };
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), crate::ApiToken::new("t")).unwrap();
        // The shape from 2026-10-05: no `connections` in the tunnel.
        Mock::given(method("GET"))
            .and(path("/accounts/a1/cfd_tunnel"))
            .respond_with(ok(serde_json::json!([
                {"id": "t1", "name": "mac", "status": "healthy", "remote_config": true},
                {"id": "t2", "name": "old", "status": "inactive", "remote_config": true}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/cfd_tunnel/t1"))
            .respond_with(ok(
                serde_json::json!({"id": "t1", "name": "mac", "status": "degraded"}),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/cfd_tunnel/t1/connections"))
            .respond_with(ok(serde_json::json!([{
                "id": "c1", "version": "2026.9.1",
                "conns": [
                    {"colo_name": "ams01", "origin_ip": "198.51.100.7", "opened_at": "2026-09-23T00:00:01Z"},
                    {"colo_name": "fra02", "origin_ip": "198.51.100.7", "opened_at": "2026-09-23T00:00:02Z", "is_pending_reconnect": true}
                ]
            }])))
            .expect(2)
            .mount(&server)
            .await;

        let tunnels = client.tunnels("a1").await.unwrap();
        let conns = &tunnels[0].connections;
        assert_eq!(conns.len(), 2);
        assert_eq!(
            (
                conns[0].client_id.as_str(),
                conns[0].client_version.as_str()
            ),
            ("c1", "2026.9.1")
        );
        assert_eq!(conns[1].colo_name, "fra02");
        assert!(conns[1].is_pending_reconnect);
        assert!(
            tunnels[1].connections.is_empty(),
            "an inactive tunnel isn't asked (the mock allows two calls: this list and the get)"
        );
        assert_eq!(
            client.tunnel("a1", "t1").await.unwrap().connections.len(),
            2
        );
    }
}
