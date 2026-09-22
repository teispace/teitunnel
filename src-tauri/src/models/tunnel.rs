use serde::{Deserialize, Deserializer, Serialize};

pub fn deserialize_null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let opt = Option::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareAccount {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareZone {
    pub id: String,
    pub name: String,
    pub status: String,
    pub paused: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudflareTunnel {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default, alias = "createdAt")]
    pub created_at: Option<String>,
    #[serde(default, alias = "deletedAt")]
    pub deleted_at: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub connections: Vec<TunnelConnection>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub remote_config: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertStatus {
    pub has_cert: bool,
    pub cert_path: Option<String>,
    pub zone_id: Option<String>,
    pub account_id: Option<String>,
    pub api_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConnection {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub features: Option<Vec<String>>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub arch: Option<String>,
    #[serde(default, alias = "coloName")]
    pub colo_name: Option<String>,
    #[serde(default, alias = "isPendingReconnect")]
    pub is_pending_reconnect: Option<bool>,
    #[serde(default, alias = "openedAt")]
    pub opened_at: Option<String>,
    #[serde(default, alias = "clientId")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateTunnelRequest {
    pub name: String,
    pub config_src: Option<String>, // "cloudflare" or "local"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelProcessState {
    pub tunnel_id: String,
    pub pid: Option<u32>,
    pub is_running: bool,
    pub started_at: Option<String>,
    pub metrics_port: Option<u16>,
    pub mode: String, // "remote", "quick", "local"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuickTunnelState {
    pub is_running: bool,
    pub pid: Option<u32>,
    pub local_port: u16,
    pub public_url: Option<String>,
    pub started_at: Option<String>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelLogEvent {
    pub tunnel_id: String,
    pub line: String,
    pub level: String, // "INFO", "WARN", "ERR", "DEBUG"
    pub timestamp: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_null_and_empty_tunnels() {
        let null_str = "null";
        let tunnels: Option<Vec<CloudflareTunnel>> = serde_json::from_str(null_str).unwrap();
        assert!(tunnels.is_none());

        let empty_str = "[]";
        let tunnels: Option<Vec<CloudflareTunnel>> = serde_json::from_str(empty_str).unwrap();
        assert_eq!(tunnels.unwrap().len(), 0);

        let json_with_null_connections = r#"[
            {
                "id": "769741c8-test",
                "name": "my-named-tunnel",
                "createdAt": "2026-09-20T14:00:00Z",
                "deletedAt": null,
                "connections": null
            }
        ]"#;
        let tunnels: Option<Vec<CloudflareTunnel>> =
            serde_json::from_str(json_with_null_connections).unwrap();
        let list = tunnels.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "769741c8-test");
        assert_eq!(list[0].name, "my-named-tunnel");
        assert_eq!(list[0].connections.len(), 0);
        assert_eq!(list[0].remote_config, false);
    }
}

