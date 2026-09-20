use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OriginRequestConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp_keep_alive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_tls_verify: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_server_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_pool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http_host_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_chunked_encoding: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IngressRule {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    pub service: String, // e.g. "http://localhost:3000" or "http_status:404"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin_request: Option<OriginRequestConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfiguration {
    pub ingress: Vec<IngressRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfigResponse {
    pub tunnel_id: String,
    pub version: Option<i64>,
    pub config: Option<TunnelConfiguration>,
}
