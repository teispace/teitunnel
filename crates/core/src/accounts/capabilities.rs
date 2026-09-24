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
    /// Read traffic analytics (Zone ▸ Analytics ▸ Read; optional).
    Analytics,
    /// Publish Snapshots as Workers (optional).
    WorkersEdit,
    /// Edge rules: custom and rate limiting rules (Zone WAF) and header rules
    /// (Transform Rules), optional.
    EdgeRules,
    /// Access service tokens (optional).
    ServiceTokens,
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
    /// Whether Workers can answer on its hostnames (Snapshots' Custom Domains).
    pub workers_routes: Grant,
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
    /// Traffic analytics (optional feature), probed on the first domain.
    pub analytics: Grant,
    /// Workers (Snapshots, optional feature).
    pub workers_edit: Grant,
    /// Edge rules (optional feature), probed on the first domain.
    pub edge_rules: Grant,
    /// Access service tokens (optional feature).
    pub service_tokens: Grant,
    /// D1 databases: Snapshot comments and webhook inboxes (optional feature).
    pub d1: Grant,
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
    // Snapshots are Workers: PATCHing a missing Worker's settings is 404 when allowed.
    let worker_member =
        format!("{account}/workers/scripts/teitunnel-permission-check/script-settings");
    let service_tokens = format!("{account}/access/service_tokens");
    // D1: PATCHing a database that doesn't exist is 404 when D1 Write is granted.
    let d1_member = format!("{account}/d1/database/{NIL_UUID}");
    let (tunnels_read, tunnels_edit, access_apps, access_methods, workers_edit, tokens, d1) = tokio::join!(
        client.probe_read(&tunnels),
        client.probe_write(&tunnel_member),
        client.probe_write(&access_member),
        client.probe_read(&login_methods),
        client.probe_write(&worker_member),
        client.probe_read(&service_tokens),
        client.probe_write(&d1_member),
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
    let zones_list = zones_result.unwrap_or_default();
    // Every domain in an account shares the token's analytics grant in practice
    // (tokens are made for "all zones" from the template); one probe is enough.
    let analytics = match zones_list.first() {
        Some(zone) => client.probe_analytics(&zone.id).await.into(),
        None => Grant::Unknown,
    };
    // Edge rules: reading a phase's entry point (404 when there's none) needs the
    // same permission as writing it; custom rules (Zone WAF) and header rules
    // (Transform Rules) are separate permissions, and both are needed.
    let edge_rules = match zones_list.first() {
        Some(zone) => {
            let entrypoint =
                |phase: &str| format!("/zones/{}/rulesets/phases/{phase}/entrypoint", zone.id);
            let (custom, headers) = (
                entrypoint(cf_api::PHASE_CUSTOM),
                entrypoint(cf_api::PHASE_REQUEST_HEADERS),
            );
            let (waf, transform) =
                tokio::join!(client.probe_read(&custom), client.probe_read(&headers));
            both(waf, transform)
        }
        None => Grant::Unknown,
    };
    let mut zones = Vec::new();
    for zone in zones_list {
        let dns = format!("/zones/{}/dns_records/{NIL_ID}", zone.id);
        let routes = format!("/zones/{}/workers/routes", zone.id);
        let (dns_edit, workers_routes) =
            tokio::join!(client.probe_write(&dns), client.probe_read(&routes));
        zones.push(ZoneGrant {
            zone_id: zone.id,
            zone_name: zone.name,
            dns_edit: dns_edit.into(),
            workers_routes: workers_routes.into(),
        });
    }
    Capabilities {
        zones_read,
        tunnels_read: tunnels_read.into(),
        tunnels_edit: tunnels_edit.into(),
        access_edit: both(access_apps, access_methods),
        analytics,
        workers_edit: workers_edit.into(),
        edge_rules,
        service_tokens: tokens.into(),
        d1: d1.into(),
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
        Mock::given(method("PATCH"))
            .and(path(
                "/accounts/a1/workers/scripts/teitunnel-permission-check/script-settings",
            ))
            .respond_with(error(404, 10007))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/zones/z1/rulesets/phases/http_request_firewall_custom/entrypoint",
            ))
            .respond_with(error(404, 10003))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(
                "/zones/z1/rulesets/phases/http_request_late_transform/entrypoint",
            ))
            .respond_with(error(403, 10000))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/accounts/a1/access/service_tokens"))
            .respond_with(list(serde_json::json!([])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/zones/z1/workers/routes"))
            .respond_with(list(serde_json::json!([])))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/zones/z2/workers/routes"))
            .respond_with(error(403, 10000))
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
        // Analytics is probed with a read-only GraphQL query (the zone's limits).
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": null, "errors": [{"message": "zones ['z1'] are not authorized"}]
            })))
            .expect(1)
            .mount(&server)
            .await;
        // No other POST, and no PUT/DELETE, may ever be sent.
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
        assert_eq!(caps.analytics, Grant::No);
        assert_eq!(caps.workers_edit, Grant::Yes);
        assert_eq!(caps.edge_rules, Grant::No, "Transform Rules is missing");
        assert_eq!(caps.service_tokens, Grant::Yes);
        assert_eq!(
            caps.zones
                .iter()
                .map(|z| z.workers_routes)
                .collect::<Vec<_>>(),
            [Grant::Yes, Grant::No]
        );
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
