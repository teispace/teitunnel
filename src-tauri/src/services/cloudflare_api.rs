use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::AppError;
use crate::models::{
    CloudflareAccount, CloudflareTunnel, CloudflareZone, CreateDnsRecordRequest,
    CreateTunnelRequest, DnsHygieneReport, DnsRecord, IngressRule, OrphanedDnsRecord,
    TunnelConfigResponse, TunnelConfiguration,
};

const BASE_URL: &str = "https://api.cloudflare.com/client/v4";

pub struct CloudflareClient {
    client: reqwest::Client,
    token: String,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    success: bool,
    errors: Option<Vec<ApiError>>,
    result: Option<T>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ApiError {
    code: i64,
    message: String,
}


impl CloudflareClient {
    pub fn new(token: String) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap_or_default();
        Self { client, token }
    }

    fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            AUTHORIZATION,
            HeaderValue::from_str(&format!("Bearer {}", self.token)).unwrap(),
        );
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    /// Verifies that the API token is valid
    pub async fn verify_token(&self) -> Result<bool, AppError> {
        let res = self
            .client
            .get(format!("{}/user/tokens/verify", BASE_URL))
            .headers(self.headers())
            .send()
            .await?;

        if !res.status().is_success() {
            return Ok(false);
        }

        let body: ApiResponse<Value> = res.json().await?;
        Ok(body.success)
    }

    /// Lists accounts accessible by this token
    pub async fn list_accounts(&self) -> Result<Vec<CloudflareAccount>, AppError> {
        let res = self
            .client
            .get(format!("{}/accounts", BASE_URL))
            .headers(self.headers())
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<Vec<CloudflareAccount>> = res.json().await?;
        if !body.success {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to fetch accounts".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body.result.unwrap_or_default())
    }

    /// Lists zones (domains) for a given account
    pub async fn list_zones(&self, account_id: &str) -> Result<Vec<CloudflareZone>, AppError> {
        let res = self
            .client
            .get(format!("{}/zones?account.id={}", BASE_URL, account_id))
            .headers(self.headers())
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<Vec<CloudflareZone>> = res.json().await?;
        if !body.success {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to fetch zones".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body.result.unwrap_or_default())
    }

    /// Lists tunnels in an account (excluding deleted ones)
    pub async fn list_tunnels(&self, account_id: &str) -> Result<Vec<CloudflareTunnel>, AppError> {
        let res = self
            .client
            .get(format!(
                "{}/accounts/{}/cfd_tunnel?is_deleted=false",
                BASE_URL, account_id
            ))
            .headers(self.headers())
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<Vec<CloudflareTunnel>> = res.json().await?;
        if !body.success {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to fetch tunnels".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body.result.unwrap_or_default())
    }

    /// Creates a new remotely-managed tunnel in Cloudflare Zero Trust
    pub async fn create_tunnel(
        &self,
        account_id: &str,
        name: &str,
    ) -> Result<CloudflareTunnel, AppError> {
        let req = CreateTunnelRequest {
            name: name.to_string(),
            config_src: Some("cloudflare".to_string()),
        };

        let res = self
            .client
            .post(format!("{}/accounts/{}/cfd_tunnel", BASE_URL, account_id))
            .headers(self.headers())
            .json(&req)
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<CloudflareTunnel> = res.json().await?;
        if !body.success || body.result.is_none() {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to create tunnel".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body.result.unwrap())
    }

    /// Retrieves the tunnel connection token required by `cloudflared tunnel run --token <TOKEN>`
    pub async fn get_tunnel_token(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<String, AppError> {
        let res = self
            .client
            .get(format!(
                "{}/accounts/{}/cfd_tunnel/{}/token",
                BASE_URL, account_id, tunnel_id
            ))
            .headers(self.headers())
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<String> = res.json().await?;
        if !body.success || body.result.is_none() {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to fetch tunnel token".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body.result.unwrap())
    }

    /// Deletes a tunnel from Cloudflare
    pub async fn delete_tunnel(&self, account_id: &str, tunnel_id: &str) -> Result<(), AppError> {
        let res = self
            .client
            .delete(format!(
                "{}/accounts/{}/cfd_tunnel/{}",
                BASE_URL, account_id, tunnel_id
            ))
            .headers(self.headers())
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<Value> = res.json().await?;
        if !body.success {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to delete tunnel".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(())
    }

    /// Gets the ingress configuration for a tunnel
    pub async fn get_tunnel_configuration(
        &self,
        account_id: &str,
        tunnel_id: &str,
    ) -> Result<TunnelConfiguration, AppError> {
        let res = self
            .client
            .get(format!(
                "{}/accounts/{}/cfd_tunnel/{}/configurations",
                BASE_URL, account_id, tunnel_id
            ))
            .headers(self.headers())
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<TunnelConfigResponse> = res.json().await?;
        if !body.success {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to get tunnel configuration".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body
            .result
            .and_then(|r| r.config)
            .unwrap_or_else(|| TunnelConfiguration { ingress: vec![] }))
    }

    /// Updates the ingress configuration for a tunnel on Cloudflare edge
    pub async fn update_tunnel_configuration(
        &self,
        account_id: &str,
        tunnel_id: &str,
        mut rules: Vec<IngressRule>,
    ) -> Result<TunnelConfiguration, AppError> {
        // Enforce catch-all 404 rule at the end if not present
        let has_catch_all = rules.iter().any(|r| r.service == "http_status:404");
        if !has_catch_all {
            rules.push(IngressRule {
                hostname: None,
                path: None,
                service: "http_status:404".to_string(),
                origin_request: None,
            });
        }

        #[derive(Serialize)]
        struct UpdateConfigPayload {
            config: TunnelConfiguration,
        }

        let payload = UpdateConfigPayload {
            config: TunnelConfiguration { ingress: rules },
        };

        let res = self
            .client
            .put(format!(
                "{}/accounts/{}/cfd_tunnel/{}/configurations",
                BASE_URL, account_id, tunnel_id
            ))
            .headers(self.headers())
            .json(&payload)
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<TunnelConfigResponse> = res.json().await?;
        if !body.success {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to update tunnel configuration".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body
            .result
            .and_then(|r| r.config)
            .unwrap_or_else(|| TunnelConfiguration { ingress: vec![] }))
    }

    /// Lists DNS records for a zone (can filter by CNAME type)
    pub async fn list_dns_records(
        &self,
        zone_id: &str,
        record_type: Option<&str>,
    ) -> Result<Vec<DnsRecord>, AppError> {
        let mut url = format!("{}/zones/{}/dns_records?per_page=100", BASE_URL, zone_id);
        if let Some(t) = record_type {
            url.push_str(&format!("&type={}", t));
        }

        let res = self.client.get(url).headers(self.headers()).send().await?;

        let status = res.status().as_u16();
        let body: ApiResponse<Vec<DnsRecord>> = res.json().await?;
        if !body.success {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to fetch DNS records".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body.result.unwrap_or_default())
    }

    /// Creates a proxied CNAME record pointing a hostname to <tunnel_uuid>.cfargotunnel.com
    pub async fn create_cname_record(
        &self,
        zone_id: &str,
        name: &str,
        tunnel_uuid: &str,
    ) -> Result<DnsRecord, AppError> {
        let target_content = format!("{}.cfargotunnel.com", tunnel_uuid);
        let req = CreateDnsRecordRequest {
            name: name.to_string(),
            record_type: "CNAME".to_string(),
            content: target_content,
            proxied: true,
            comment: Some("Managed by Teitunnel".to_string()),
        };

        let res = self
            .client
            .post(format!("{}/zones/{}/dns_records", BASE_URL, zone_id))
            .headers(self.headers())
            .json(&req)
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<DnsRecord> = res.json().await?;
        if !body.success || body.result.is_none() {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to create DNS record".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(body.result.unwrap())
    }

    /// Deletes a DNS record by ID (Cascade cleanup & hygiene)
    pub async fn delete_dns_record(&self, zone_id: &str, record_id: &str) -> Result<(), AppError> {
        let res = self
            .client
            .delete(format!(
                "{}/zones/{}/dns_records/{}",
                BASE_URL, zone_id, record_id
            ))
            .headers(self.headers())
            .send()
            .await?;

        let status = res.status().as_u16();
        let body: ApiResponse<Value> = res.json().await?;
        if !body.success {
            let msg = body
                .errors
                .and_then(|e| e.first().map(|err| err.message.clone()))
                .unwrap_or_else(|| "Failed to delete DNS record".into());
            return Err(AppError::CloudflareApiError {
                status,
                message: msg,
            });
        }

        Ok(())
    }

    /// DNS Hygiene Scanner: Scans zone for CNAME records pointing to .cfargotunnel.com
    /// and cross-references them against active tunnels to find orphaned records!
    pub async fn scan_dns_hygiene(
        &self,
        account_id: &str,
        zone_id: &str,
    ) -> Result<DnsHygieneReport, AppError> {
        let active_tunnels = self.list_tunnels(account_id).await?;
        let active_uuids: std::collections::HashSet<String> =
            active_tunnels.into_iter().map(|t| t.id).collect();

        let cname_records = self.list_dns_records(zone_id, Some("CNAME")).await?;
        let mut orphaned_records = Vec::new();
        let mut tunnel_cnames_count = 0;

        for record in &cname_records {
            if record.content.ends_with(".cfargotunnel.com") {
                tunnel_cnames_count += 1;
                let target_uuid = record
                    .content
                    .trim_end_matches(".cfargotunnel.com")
                    .to_string();

                if !active_uuids.contains(&target_uuid) {
                    orphaned_records.push(OrphanedDnsRecord {
                        record: record.clone(),
                        target_tunnel_uuid: target_uuid,
                        is_orphaned: true,
                        reason: "Target tunnel UUID does not exist or has been deleted".to_string(),
                    });
                }
            }
        }

        Ok(DnsHygieneReport {
            total_cnames_scanned: cname_records.len(),
            tunnel_cnames_count,
            orphaned_records,
        })
    }
}
