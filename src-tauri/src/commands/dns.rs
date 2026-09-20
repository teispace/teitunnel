use crate::error::AppError;
use crate::models::{DnsHygieneReport, DnsRecord};
use crate::services::{CloudflareClient, KeyringStore};

#[tauri::command]
pub async fn list_dns_records(
    zone_id: String,
    record_type: Option<String>,
    token: Option<String>,
) -> Result<Vec<DnsRecord>, AppError> {
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

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
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

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
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client.delete_dns_record(&zone_id, &record_id).await
}

#[tauri::command]
pub async fn scan_dns_hygiene(
    account_id: String,
    zone_id: String,
    token: Option<String>,
) -> Result<DnsHygieneReport, AppError> {
    let tok = match token {
        Some(t) => t,
        None => KeyringStore::get_token()?.ok_or_else(|| {
            AppError::KeyringError("No Cloudflare API token configured".into())
        })?,
    };

    let client = CloudflareClient::new(tok);
    client.scan_dns_hygiene(&account_id, &zone_id).await
}
