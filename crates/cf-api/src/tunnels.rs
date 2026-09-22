//! Cloudflare Tunnel (cfd_tunnel) endpoints and remote configuration.
//!
//! Configuration models keep every field they don't know in `extra`, so writing a
//! config back never drops settings someone made in the dashboard.

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
        self.get_all(&format!("{}?is_deleted=false", tunnels_path(account)))
            .await
    }

    /// One tunnel.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn tunnel(&self, account: &str, tunnel: &str) -> Result<Tunnel> {
        self.get(&tunnel_path(account, tunnel)).await
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
}
