//! Private networks: CIDR routes that send WARP clients' traffic for a range through a
//! tunnel, the virtual networks they live in, and the Zero Trust device settings that
//! decide whether clients send that traffic at all.
//!
//! Shapes per Cloudflare's API reference (`…/teamnet/routes`, `…/teamnet/virtual_networks`,
//! `…/devices/settings`, `…/devices/policy`), checked 2026-09-23. Since cloudflared
//! 2023.9.0 a route is all a tunnel needs; there's no `warp-routing` switch any more.

use serde::Deserialize;
use serde_json::json;

use crate::{Client, Result, resources::encode};

/// A route from a private IP range to a tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct NetworkRoute {
    /// Route id.
    pub id: String,
    /// The range, in CIDR notation.
    pub network: String,
    /// The tunnel it goes to.
    #[serde(default)]
    pub tunnel_id: String,
    /// That tunnel's name.
    #[serde(default)]
    pub tunnel_name: Option<String>,
    /// The virtual network it's in.
    #[serde(default)]
    pub virtual_network_id: Option<String>,
    /// A remark, e.g. Teitunnel's ownership mark.
    #[serde(default)]
    pub comment: String,
}

/// A virtual network (a namespace for overlapping private ranges).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct VirtualNetwork {
    /// Id.
    pub id: String,
    /// Name.
    #[serde(default)]
    pub name: String,
    /// Routes created without a virtual network go here.
    #[serde(default)]
    pub is_default_network: bool,
}

/// Account-wide WARP client settings that private networks depend on.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
pub struct DeviceSettings {
    /// Gateway proxies TCP ("Allow Secure Web Gateway to proxy traffic").
    #[serde(default)]
    pub gateway_proxy_enabled: Option<bool>,
    /// … and UDP.
    #[serde(default)]
    pub gateway_udp_proxy_enabled: Option<bool>,
}

/// One entry of a Split Tunnels list: an address range or a host.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct SplitTunnelEntry {
    /// A CIDR range.
    #[serde(default)]
    pub address: Option<String>,
    /// A domain.
    #[serde(default)]
    pub host: Option<String>,
    /// Shown in the client.
    #[serde(default)]
    pub description: Option<String>,
}

/// The default device profile's Split Tunnels: WARP sends everything except `exclude`,
/// or (include mode) only `include`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct DefaultDeviceProfile {
    /// Exclude mode's list.
    #[serde(default)]
    pub exclude: Option<Vec<SplitTunnelEntry>>,
    /// Include mode's list.
    #[serde(default)]
    pub include: Option<Vec<SplitTunnelEntry>>,
}

fn routes_path(account: &str) -> String {
    format!("/accounts/{}/teamnet/routes", encode(account))
}

impl Client {
    /// Every private network route in the account that isn't deleted.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn network_routes(&self, account: &str) -> Result<Vec<NetworkRoute>> {
        self.get_all(&format!("{}?is_deleted=false", routes_path(account)))
            .await
    }

    /// Routes `network` (CIDR) to `tunnel` in `virtual_network` (the default one when
    /// `None`). Not retried on server errors (no duplicates).
    ///
    /// # Errors
    /// API errors, e.g. when the range is already routed.
    pub async fn create_network_route(
        &self,
        account: &str,
        network: &str,
        tunnel: &str,
        comment: &str,
        virtual_network: Option<&str>,
    ) -> Result<NetworkRoute> {
        let mut body = json!({ "network": network, "tunnel_id": tunnel, "comment": comment });
        if let Some(id) = virtual_network {
            body["virtual_network_id"] = json!(id);
        }
        self.post(&routes_path(account), &body).await
    }

    /// Deletes a route (a missing one counts as deleted).
    ///
    /// # Errors
    /// API or network errors.
    pub async fn delete_network_route(&self, account: &str, id: &str) -> Result<()> {
        self.delete(&format!("{}/{}", routes_path(account), encode(id)))
            .await
    }

    /// The account's virtual networks.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn virtual_networks(&self, account: &str) -> Result<Vec<VirtualNetwork>> {
        self.get_all(&format!(
            "/accounts/{}/teamnet/virtual_networks?is_deleted=false",
            encode(account)
        ))
        .await
    }

    /// Account-wide WARP client settings.
    ///
    /// # Errors
    /// API or network errors (reading needs Zero Trust read access).
    pub async fn device_settings(&self, account: &str) -> Result<DeviceSettings> {
        self.get(&format!("/accounts/{}/devices/settings", encode(account)))
            .await
    }

    /// The default device profile's Split Tunnels.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn default_device_profile(&self, account: &str) -> Result<DefaultDeviceProfile> {
        self.get(&format!("/accounts/{}/devices/policy", encode(account)))
            .await
    }
}

#[cfg(test)]
mod tests {
    use serde_json::Value;
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{method, path, query_param},
    };

    use super::*;
    use crate::ApiToken;

    fn ok(result: &Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": result,
            "result_info": {"page": 1, "per_page": 50, "total_pages": 1}
        }))
    }

    #[tokio::test]
    async fn manages_network_routes() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("GET"))
            .and(path("/accounts/a1/teamnet/routes"))
            .and(query_param("is_deleted", "false"))
            .respond_with(ok(&json!([{
                "id": "r1", "network": "192.168.1.0/24", "tunnel_id": "t1", "tunnel_name": "Mac",
                "virtual_network_id": "v1", "virtual_network_name": "default",
                "comment": "teitunnel", "created_at": "2026-09-23T00:00:00Z", "deleted_at": null,
                "tun_type": "cfd_tunnel"
            }])))
            .mount(&server)
            .await;
        let routes = client.network_routes("a1").await.unwrap();
        assert_eq!(routes[0].network, "192.168.1.0/24");
        assert_eq!(routes[0].tunnel_name.as_deref(), Some("Mac"));

        Mock::given(method("POST"))
            .and(path("/accounts/a1/teamnet/routes"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(
                    body,
                    json!({"network": "10.0.0.0/24", "tunnel_id": "t1", "comment": "teitunnel"})
                );
                ok(&json!({"id": "r2", "network": "10.0.0.0/24", "tunnel_id": "t1", "comment": "teitunnel"}))
            })
            .mount(&server)
            .await;
        let route = client
            .create_network_route("a1", "10.0.0.0/24", "t1", "teitunnel", None)
            .await
            .unwrap();
        assert_eq!(route.id, "r2");

        Mock::given(method("DELETE"))
            .and(path("/accounts/a1/teamnet/routes/r2"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        client.delete_network_route("a1", "r2").await.unwrap();
    }

    #[tokio::test]
    async fn reads_what_warp_clients_will_do() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("GET"))
            .and(path("/accounts/a1/teamnet/virtual_networks"))
            .respond_with(ok(&json!([
                {"id": "v1", "name": "default", "is_default_network": true},
                {"id": "v2", "name": "lab", "is_default_network": false}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/devices/settings"))
            .respond_with(ok(&json!({"gateway_proxy_enabled": true})))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/devices/policy"))
            .respond_with(ok(&json!({
                "exclude": [{"address": "192.168.0.0/16", "description": "RFC 1918"},
                            {"host": "*.local"}],
                "include": null
            })))
            .mount(&server)
            .await;
        let vnets = client.virtual_networks("a1").await.unwrap();
        assert!(vnets[0].is_default_network);
        let settings = client.device_settings("a1").await.unwrap();
        assert_eq!(settings.gateway_proxy_enabled, Some(true));
        assert_eq!(settings.gateway_udp_proxy_enabled, None);
        let profile = client.default_device_profile("a1").await.unwrap();
        let exclude = profile.exclude.unwrap();
        assert_eq!(exclude[0].address.as_deref(), Some("192.168.0.0/16"));
        assert_eq!(exclude[1].host.as_deref(), Some("*.local"));
        assert_eq!(profile.include, None);
    }
}
