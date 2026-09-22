use crate::error::AppError;
use crate::models::{DnsHygieneReport, DnsRecord};
use crate::services::{BinaryManager, CloudflareClient};
use super::resolve_token;

#[tauri::command]
pub async fn list_dns_records(
    zone_id: String,
    record_type: Option<String>,
    token: Option<String>,
) -> Result<Vec<DnsRecord>, AppError> {
    let tok = resolve_token(token)?;
    let client = CloudflareClient::new(tok);
    client
        .list_dns_records(&zone_id, record_type.as_deref())
        .await
}

#[tauri::command]
pub async fn create_dns_cname(
    zone_id: String,
    name: String,
    tunnel_uuid: String,
    token: Option<String>,
) -> Result<DnsRecord, AppError> {
    let tok_res = resolve_token(token);

    // 1. If token is resolved, attempt Cloudflare REST API first
    if let Ok(ref tok) = tok_res {
        let client = CloudflareClient::new(tok.clone());
        if let Ok(rec) = client.create_cname_record(&zone_id, &name, &tunnel_uuid).await {
            return Ok(rec);
        }
    }

    // 2. Fallback to cloudflared CLI tunnel route dns if binary is present
    if let Some(binary) = BinaryManager::find_binary() {
        let output = tokio::process::Command::new(binary)
            .arg("tunnel")
            .arg("route")
            .arg("dns")
            .arg("-f")
            .arg(&tunnel_uuid)
            .arg(&name)
            .output()
            .await;

        if let Ok(out) = output {
            if out.status.success() {
                // If token is present, fetch the newly created record details
                if let Ok(ref tok) = tok_res {
                    let client = CloudflareClient::new(tok.clone());
                    if let Ok(records) = client.list_dns_records(&zone_id, Some("CNAME")).await {
                        if let Some(found) = records.into_iter().find(|r| r.name == name || r.name.starts_with(&name)) {
                            return Ok(found);
                        }
                    }
                }

                return Ok(DnsRecord {
                    id: format!("cli-{}", chrono::Utc::now().timestamp_millis()),
                    zone_id: zone_id.clone(),
                    zone_name: None,
                    name: name.clone(),
                    record_type: "CNAME".into(),
                    content: format!("{}.cfargotunnel.com", tunnel_uuid),
                    proxied: true,
                    ttl: 1,
                    comment: Some("Managed by Teitunnel".into()),
                    created_on: Some(chrono::Utc::now().to_rfc3339()),
                    modified_on: Some(chrono::Utc::now().to_rfc3339()),
                });
            } else {
                let err_text = String::from_utf8_lossy(&out.stderr);
                if !err_text.trim().is_empty() {
                    return Err(AppError::ProcessError(format!("cloudflared route dns failed: {}", err_text.trim())));
                }
            }
        }
    }

    // 3. If fallback also failed, return direct API error
    let tok = tok_res?;
    let client = CloudflareClient::new(tok);
    client
        .create_cname_record(&zone_id, &name, &tunnel_uuid)
        .await
}

#[tauri::command]
pub async fn delete_dns_record(
    zone_id: String,
    record_id: String,
    token: Option<String>,
) -> Result<(), AppError> {
    let tok = resolve_token(token)?;
    let client = CloudflareClient::new(tok);
    client.delete_dns_record(&zone_id, &record_id).await
}

#[tauri::command]
pub async fn scan_dns_hygiene(
    account_id: String,
    zone_id: String,
    token: Option<String>,
) -> Result<DnsHygieneReport, AppError> {
    let tok = resolve_token(token)?;
    let client = CloudflareClient::new(tok);
    client.scan_dns_hygiene(&account_id, &zone_id).await
}
