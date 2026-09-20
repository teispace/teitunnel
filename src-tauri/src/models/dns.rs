use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsRecord {
    pub id: String,
    pub zone_id: String,
    pub zone_name: Option<String>,
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String, // "CNAME", "A", etc.
    pub content: String,
    pub proxied: bool,
    pub ttl: u32,
    pub comment: Option<String>,
    pub created_on: Option<String>,
    pub modified_on: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateDnsRecordRequest {
    pub name: String,
    #[serde(rename = "type")]
    pub record_type: String,
    pub content: String,
    pub proxied: bool,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrphanedDnsRecord {
    pub record: DnsRecord,
    pub target_tunnel_uuid: String,
    pub is_orphaned: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsHygieneReport {
    pub total_cnames_scanned: usize,
    pub tunnel_cnames_count: usize,
    pub orphaned_records: Vec<OrphanedDnsRecord>,
}
