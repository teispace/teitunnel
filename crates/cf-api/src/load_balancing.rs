//! Cloudflare Load Balancing (a paid add-on): monitors and pools per account, load
//! balancers per zone. Teitunnel balances a route across tunnels the way Cloudflare
//! documents it: one pool whose endpoints are `<tunnel id>.cfargotunnel.com` with the
//! route's hostname as the Host header, and a load balancer named after the hostname
//! (it takes precedence over the DNS record of the same name).
//!
//! Shapes per Cloudflare's OpenAPI schema (`cloudflare/api-schemas`, checked 2026-09-23).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{Client, Result};

/// A health monitor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Monitor {
    /// Id (absent when creating one).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// `http` or `https`.
    #[serde(rename = "type")]
    pub kind: String,
    /// What it's for (Teitunnel writes its ownership marker here).
    #[serde(default)]
    pub description: String,
    /// Path to request.
    #[serde(default)]
    pub path: String,
    /// Status codes counted as healthy, e.g. `2xx`.
    #[serde(default)]
    pub expected_codes: String,
    /// Follow redirects before judging.
    #[serde(default)]
    pub follow_redirects: bool,
    /// Headers sent with the check (the Host header picks the route in the tunnel).
    #[serde(default)]
    pub header: BTreeMap<String, Vec<String>>,
}

/// One endpoint of a pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    /// Display name.
    pub name: String,
    /// `<tunnel id>.cfargotunnel.com` for a tunnel.
    pub address: String,
    /// Whether it gets traffic.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Request headers: `Host` selects the route inside the tunnel.
    #[serde(default)]
    pub header: BTreeMap<String, Vec<String>>,
}

fn enabled() -> bool {
    true
}

/// A pool of endpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pool {
    /// Id (absent when creating one).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// Name: letters, digits, `-` and `_`.
    pub name: String,
    /// Description (Teitunnel's ownership marker).
    #[serde(default)]
    pub description: String,
    /// Whether it's in service.
    #[serde(default = "enabled")]
    pub enabled: bool,
    /// Health monitor id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub monitor: Option<String>,
    /// Endpoints.
    #[serde(default)]
    pub origins: Vec<Origin>,
}

/// A load balancer on a zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoadBalancer {
    /// Id (absent when creating one).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id: String,
    /// The hostname it answers for.
    pub name: String,
    /// Description (Teitunnel's ownership marker).
    #[serde(default)]
    pub description: String,
    /// Pools, in order of preference.
    pub default_pools: Vec<String>,
    /// The pool used when all are unhealthy.
    pub fallback_pool: String,
    /// Proxied through Cloudflare (needed for tunnel endpoints).
    #[serde(default)]
    pub proxied: bool,
}

/// How one endpoint of a pool does, as seen from one Cloudflare region.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginHealth {
    /// The endpoint's address (`<tunnel id>.cfargotunnel.com` for a tunnel).
    pub address: String,
    /// Whether the region's checks pass.
    pub healthy: bool,
    /// Why they fail, in Cloudflare's words.
    pub failure_reason: Option<String>,
    /// The status code of the last check.
    pub response_code: Option<u16>,
}

/// A pool's health per region.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PoolHealth {
    /// Region name (e.g. `Amsterdam, NL`) → its view of each endpoint.
    pub regions: BTreeMap<String, Vec<OriginHealth>>,
}

impl PoolHealth {
    /// Reads `pop_health`: region → `{ healthy, origins: [{ <address>: {...} }] }`.
    /// Anything that doesn't fit is skipped rather than failing the read.
    fn from_result(result: &serde_json::Value) -> Self {
        let mut regions = BTreeMap::new();
        let Some(pops) = result.get("pop_health").and_then(|v| v.as_object()) else {
            return Self { regions };
        };
        for (region, pop) in pops {
            let Some(origins) = pop.get("origins").and_then(|v| v.as_array()) else {
                continue;
            };
            let seen = origins
                .iter()
                .filter_map(|o| o.as_object())
                .flat_map(|o| o.iter())
                .map(|(address, check)| OriginHealth {
                    address: address.clone(),
                    healthy: check
                        .get("healthy")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    failure_reason: check
                        .get("failure_reason")
                        .and_then(|v| v.as_str())
                        .filter(|r| !r.is_empty() && *r != "No failures")
                        .map(str::to_owned),
                    response_code: check
                        .get("response_code")
                        .and_then(serde_json::Value::as_u64)
                        .and_then(|c| u16::try_from(c).ok()),
                })
                .collect();
            regions.insert(region.clone(), seen);
        }
        Self { regions }
    }
}

fn monitors(account: &str) -> String {
    format!(
        "/accounts/{}/load_balancers/monitors",
        crate::encode(account)
    )
}

fn pools(account: &str) -> String {
    format!("/accounts/{}/load_balancers/pools", crate::encode(account))
}

fn balancers(zone: &str) -> String {
    format!("/zones/{}/load_balancers", crate::encode(zone))
}

impl Client {
    /// The account's monitors.
    ///
    /// # Errors
    /// API errors (403 without the add-on or the permission).
    pub async fn lb_monitors(&self, account: &str) -> Result<Vec<Monitor>> {
        self.get_all(&monitors(account)).await
    }

    /// Creates a monitor.
    ///
    /// # Errors
    /// API errors.
    pub async fn create_lb_monitor(&self, account: &str, monitor: &Monitor) -> Result<Monitor> {
        self.post(&monitors(account), &serde_json::to_value(monitor)?)
            .await
    }

    /// Deletes a monitor (a missing one counts as deleted).
    ///
    /// # Errors
    /// API errors.
    pub async fn delete_lb_monitor(&self, account: &str, id: &str) -> Result<()> {
        self.delete(&format!("{}/{}", monitors(account), crate::encode(id)))
            .await
    }

    /// The account's pools.
    ///
    /// # Errors
    /// API errors.
    pub async fn lb_pools(&self, account: &str) -> Result<Vec<Pool>> {
        self.get_all(&pools(account)).await
    }

    /// Creates a pool.
    ///
    /// # Errors
    /// API errors.
    pub async fn create_lb_pool(&self, account: &str, pool: &Pool) -> Result<Pool> {
        self.post(&pools(account), &serde_json::to_value(pool)?)
            .await
    }

    /// Replaces a pool.
    ///
    /// # Errors
    /// API errors.
    pub async fn update_lb_pool(&self, account: &str, id: &str, pool: &Pool) -> Result<Pool> {
        self.put(
            &format!("{}/{}", pools(account), crate::encode(id)),
            &serde_json::to_value(pool)?,
        )
        .await
    }

    /// Deletes a pool.
    ///
    /// # Errors
    /// API errors.
    pub async fn delete_lb_pool(&self, account: &str, id: &str) -> Result<()> {
        self.delete(&format!("{}/{}", pools(account), crate::encode(id)))
            .await
    }

    /// How the pool's endpoints do, per Cloudflare region.
    ///
    /// # Errors
    /// API errors.
    pub async fn lb_pool_health(&self, account: &str, id: &str) -> Result<PoolHealth> {
        let result: serde_json::Value = self
            .get(&format!("{}/{}/health", pools(account), crate::encode(id)))
            .await?;
        Ok(PoolHealth::from_result(&result))
    }

    /// A zone's load balancers.
    ///
    /// # Errors
    /// API errors.
    pub async fn load_balancers(&self, zone: &str) -> Result<Vec<LoadBalancer>> {
        self.get_all(&balancers(zone)).await
    }

    /// Creates a load balancer.
    ///
    /// # Errors
    /// API errors.
    pub async fn create_load_balancer(
        &self,
        zone: &str,
        balancer: &LoadBalancer,
    ) -> Result<LoadBalancer> {
        self.post(&balancers(zone), &serde_json::to_value(balancer)?)
            .await
    }

    /// Deletes a load balancer.
    ///
    /// # Errors
    /// API errors.
    pub async fn delete_load_balancer(&self, zone: &str, id: &str) -> Result<()> {
        self.delete(&format!("{}/{}", balancers(zone), crate::encode(id)))
            .await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::ApiToken;

    fn ok(result: &serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": result,
            "result_info": {"page": 1, "per_page": 50, "total_pages": 1}
        }))
    }

    #[tokio::test]
    async fn creates_a_pool_of_tunnel_endpoints() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/accounts/a1/load_balancers/pools"))
            .respond_with(|request: &Request| {
                let body: serde_json::Value = request.body_json().unwrap();
                let mut created = body.clone();
                created["id"] = json!("p1");
                assert_eq!(body["origins"][0]["address"], "t1.cfargotunnel.com");
                assert_eq!(body["origins"][0]["header"]["Host"][0], "app.xyz.com");
                assert!(body.get("id").is_none(), "no id when creating");
                ok(&created)
            })
            .mount(&server)
            .await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        let pool = Pool {
            id: String::new(),
            name: "teitunnel-app_xyz_com".into(),
            description: "teitunnel:lb=app.xyz.com".into(),
            enabled: true,
            monitor: Some("m1".into()),
            origins: vec![Origin {
                name: "t1".into(),
                address: "t1.cfargotunnel.com".into(),
                enabled: true,
                header: BTreeMap::from([("Host".into(), vec!["app.xyz.com".into()])]),
            }],
        };
        let created = client.create_lb_pool("a1", &pool).await.unwrap();
        assert_eq!(created.id, "p1");
    }

    #[tokio::test]
    async fn reads_pool_health_per_region() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/load_balancers/pools/p1/health"))
            .respond_with(ok(&json!({
                "pool_id": "p1",
                "pop_health": {
                    "Amsterdam, NL": {"healthy": true, "origins": [
                        {"t1.cfargotunnel.com": {"healthy": true, "rtt": "12.1ms",
                            "failure_reason": "No failures", "response_code": 200}},
                        {"t2.cfargotunnel.com": {"healthy": false,
                            "failure_reason": "HTTP timeout occurred", "response_code": 0}}
                    ]},
                    "Broken": {"healthy": true}
                }
            })))
            .mount(&server)
            .await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        let health = client.lb_pool_health("a1", "p1").await.unwrap();
        assert_eq!(
            health.regions.len(),
            1,
            "a region without origins is skipped"
        );
        let seen = &health.regions["Amsterdam, NL"];
        assert_eq!(seen[0].address, "t1.cfargotunnel.com");
        assert!(seen[0].healthy && seen[0].failure_reason.is_none());
        assert!(!seen[1].healthy);
        assert_eq!(
            seen[1].failure_reason.as_deref(),
            Some("HTTP timeout occurred")
        );
    }

    #[tokio::test]
    async fn reads_load_balancers_with_unknown_fields() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/zones/z1/load_balancers"))
            .respond_with(ok(&json!([{
                "id": "lb1", "name": "app.xyz.com", "description": "", "default_pools": ["p1"],
                "fallback_pool": "p1", "proxied": true, "steering_policy": "off", "ttl": 30
            }])))
            .mount(&server)
            .await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        let list = client.load_balancers("z1").await.unwrap();
        assert_eq!(list[0].default_pools, ["p1"]);
    }
}
