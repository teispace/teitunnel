use serde::{Deserialize, Serialize};

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
    pub status: Option<String>,
    pub created_at: Option<String>,
    pub deleted_at: Option<String>,
    #[serde(default)]
    pub connections: Vec<TunnelConnection>,
    #[serde(default)]
    pub remote_config: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConnection {
    pub id: String,
    pub features: Option<Vec<String>>,
    pub version: Option<String>,
    pub arch: Option<String>,
    pub colo_name: Option<String>,
    pub is_pending_reconnect: Option<bool>,
    pub opened_at: Option<String>,
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
