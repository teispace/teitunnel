//! What a connected credential can do, so the UI can disable what it can't (with the
//! reason) instead of failing later (M2-03).

use cf_api::{Access, Client, NIL_ID, NIL_UUID, Zone};
use serde::Serialize;

/// A permission Teitunnel relies on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Permission {
    /// List domains (Zone Read).
    ZonesRead,
    /// List tunnels (Cloudflare Tunnel Read).
    TunnelsRead,
    /// Create and configure tunnels (Cloudflare Tunnel Edit).
    TunnelsEdit,
    /// Create and remove DNS records (DNS Edit), per domain.
    DnsEdit,
    /// Protect routes with Access (optional).
    AccessEdit,
}

/// The result of probing one permission.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Grant {
    /// Allowed.
    Yes,
    /// Not allowed.
    No,
    /// Couldn't be checked right now.
    Unknown,
}

impl From<Access> for Grant {
    fn from(access: Access) -> Self {
        match access {
            Access::Allowed => Self::Yes,
            Access::Denied => Self::No,
            Access::Unknown => Self::Unknown,
        }
    }
}

/// DNS permission for one domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ZoneGrant {
    /// Zone id.
    pub zone_id: String,
    /// Domain name.
    pub zone_name: String,
    /// Whether DNS records can be edited.
    pub dns_edit: Grant,
}

/// Everything a credential can do in one account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// List domains.
    pub zones_read: Grant,
    /// List tunnels.
    pub tunnels_read: Grant,
    /// Create and configure tunnels.
    pub tunnels_edit: Grant,
    /// Access policies (optional feature).
    pub access_edit: Grant,
    /// DNS editing, per domain.
    pub zones: Vec<ZoneGrant>,
}

impl Capabilities {
    /// Whether routes (tunnel + DNS) can be managed on at least one domain.
    pub fn can_manage_routes(&self) -> bool {
        self.tunnels_edit == Grant::Yes && self.zones.iter().any(|z| z.dns_edit == Grant::Yes)
    }
}

/// Probes `account_id` with `client`. `only_zone` restricts DNS probing (cert.pem
/// credentials work for one zone). All probes are free of side effects.
pub async fn probe(client: &Client, account_id: &str, only_zone: Option<&str>) -> Capabilities {
    let account = format!("/accounts/{account_id}");
    let tunnels = format!("{account}/cfd_tunnel");
    let tunnel_member = format!("{account}/cfd_tunnel/{NIL_UUID}");
    let access_member = format!("{account}/access/apps/{NIL_UUID}");
    // Logins also read (and may add) the one-time PIN login method, which is in the
    // separate "Organizations, Identity Providers, and Groups" permission. Its update
    // endpoint is PUT-only, so a PATCH probe would read 405 as allowed: list it instead.
    let login_methods = format!("{account}/access/identity_providers");
    let (tunnels_read, tunnels_edit, access_apps, access_methods) = tokio::join!(
        client.probe_read(&tunnels),
        client.probe_write(&tunnel_member),
        client.probe_write(&access_member),
        client.probe_read(&login_methods),
    );
    let zones_result: Result<Vec<Zone>, _> = match only_zone {
        Some(zone_id) => client.zone(zone_id).await.map(|zone| vec![zone]),
        None => client.zones(account_id).await,
    };
    let zones_read = match &zones_result {
        Ok(_) => Grant::Yes,
        Err(err) if err.is_auth() => Grant::No,
        Err(_) => Grant::Unknown,
    };
    let mut zones = Vec::new();
    for zone in zones_result.unwrap_or_default() {
        let dns_edit = client
            .probe_write(&format!("/zones/{}/dns_records/{NIL_ID}", zone.id))
            .await
            .into();
        zones.push(ZoneGrant {
            zone_id: zone.id,
            zone_name: zone.name,
            dns_edit,
        });
    }
    Capabilities {
        zones_read,
        tunnels_read: tunnels_read.into(),
        tunnels_edit: tunnels_edit.into(),
        access_edit: both(access_apps, access_methods),
        zones,
    }
}

/// Granted only if both are: one missing permission is enough to fail.
fn both(a: Access, b: Access) -> Grant {
    match (Grant::from(a), Grant::from(b)) {
        (Grant::No, _) | (_, Grant::No) => Grant::No,
        (Grant::Yes, Grant::Yes) => Grant::Yes,
        _ => Grant::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path, path_regex},
    };

    use super::*;

    fn error(status: u16, code: u32) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(serde_json::json!({
            "success": false, "errors": [{"code": code, "message": "x"}], "messages": [], "result": null
        }))
    }

    #[allow(clippy::needless_pass_by_value)]
    fn list(items: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true, "errors": [], "messages": [], "result": items,
            "result_info": {"page": 1, "per_page": 50, "total_pages": 1}
        }))
    }

    #[tokio::test]
    async fn builds_the_capability_map_without_writes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/cfd_tunnel"))
            .respond_with(list(serde_json::json!([])))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path(format!("/accounts/a1/cfd_tunnel/{NIL_UUID}")))
            .respond_with(error(404, 1003))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path(format!("/accounts/a1/access/apps/{NIL_UUID}")))
            .respond_with(error(403, 10000))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/access/identity_providers"))
            .respond_with(list(serde_json::json!([])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/zones"))
            .respond_with(list(serde_json::json!([
                {"id": "z1", "name": "xyz.com", "status": "active", "account": {"id": "a1"}},
                {"id": "z2", "name": "yx.com", "status": "active", "account": {"id": "a1"}}
            ])))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path(format!("/zones/z1/dns_records/{NIL_ID}")))
            .respond_with(error(404, 81044))
            .mount(&server)
            .await;
        Mock::given(method("PATCH"))
            .and(path(format!("/zones/z2/dns_records/{NIL_ID}")))
            .respond_with(error(403, 10000))
            .mount(&server)
            .await;
        // No POST/PUT/DELETE may ever be sent.
        Mock::given(path_regex(".*"))
            .and(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        Mock::given(path_regex(".*"))
            .and(method("DELETE"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;

        let client = Client::with_base(&server.uri(), cf_api::ApiToken::new("t")).unwrap();
        let caps = probe(&client, "a1", None).await;
        assert_eq!(caps.zones_read, Grant::Yes);
        assert_eq!(caps.tunnels_read, Grant::Yes);
        assert_eq!(caps.tunnels_edit, Grant::Yes);
        assert_eq!(caps.access_edit, Grant::No);
        assert_eq!(
            caps.zones
                .iter()
                .map(|z| (z.zone_name.as_str(), z.dns_edit))
                .collect::<Vec<_>>(),
            [("xyz.com", Grant::Yes), ("yx.com", Grant::No)]
        );
        assert!(caps.can_manage_routes());
    }

    #[test]
    fn logins_need_both_access_permissions() {
        use Access::{Allowed, Denied, Unknown};
        assert_eq!(both(Allowed, Allowed), Grant::Yes);
        assert_eq!(both(Allowed, Denied), Grant::No);
        assert_eq!(both(Denied, Allowed), Grant::No);
        assert_eq!(both(Unknown, Denied), Grant::No);
        assert_eq!(both(Allowed, Unknown), Grant::Unknown);
    }
}
