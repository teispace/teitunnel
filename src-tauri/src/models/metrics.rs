use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelMetrics {
    pub tunnel_id: String,
    pub timestamp: String,
    pub active_connections: usize,
    pub colos: Vec<String>,
    pub avg_rtt_ms: f64,
    pub total_requests: u64,
    pub response_2xx: u64,
    pub response_4xx: u64,
    pub response_5xx: u64,
    pub bytes_in: u64,
    pub bytes_out: u64,
}
