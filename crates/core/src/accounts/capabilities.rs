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
    /// Allowed, but the product isn't turned on for the account yet (Zero Trust).
    NotSetUp,
}

impl From<Access> for Grant {
    fn from(access: Access) -> Self {
        match access {
            Access::Allowed => Self::Yes,
            Access::Denied => Self::No,
            Access::Unknown => Self::Unknown,
            Access::NotEnabled => Self::NotSetUp,
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

/// Granted only if both are: one missing permission is enough to fail, and a product
/// that isn't turned on comes next (a permission to add is said first).
fn both(a: Access, b: Access) -> Grant {
    match (Grant::from(a), Grant::from(b)) {
        (Grant::No, _) | (_, Grant::No) => Grant::No,
        (Grant::NotSetUp, _) | (_, Grant::NotSetUp) => Grant::NotSetUp,
        (Grant::Yes, Grant::Yes) => Grant::Yes,
        _ => Grant::Unknown,
    }
}

/// A Cloudflare permission Teitunnel asks for, and what needs it. The source of the
/// docs' "Permissions and scopes" page (`crates/core/tests/permissions_doc.rs`), which
/// also checks that the table covers every key of the token link and every OAuth scope.
#[doc(hidden)]
#[derive(Debug, Clone, Copy)]
pub struct PermissionUse {
    /// As the dashboard names it; `None` for what only a sign-in asks for.
    pub name: Option<&'static str>,
    /// Its key under `permissionFix` in `locales/en.json`, when the app names it there.
    pub fix_key: Option<&'static str>,
    /// Its key in the pre-filled "Create API token" link, if the link asks for it.
    pub token_key: Option<&'static str>,
    /// The OAuth scopes that grant it at sign-in.
    pub scopes: &'static [&'static str],
    /// Whether routes need it; the rest are for optional features.
    pub required: bool,
    /// What needs it (Markdown).
    pub features: &'static str,
    /// Whether [`probe`] checks it (the rest show up when Cloudflare refuses a change).
    pub probed: bool,
}

/// Every permission and scope Teitunnel asks for, in the order the docs list them.
#[doc(hidden)]
pub const PERMISSION_USES: &[PermissionUse] = &[
    PermissionUse {
        name: Some("Account · Cloudflare Tunnel · Edit"),
        fix_key: Some("tunnels"),
        token_key: Some("argotunnel"),
        scopes: &["argotunnel.write"],
        required: true,
        features: "Creating and running tunnels, routes, shares on your domains, private networks with a token, connector logs",
        probed: true,
    },
    PermissionUse {
        name: Some("Zone · DNS · Edit"),
        fix_key: Some("dns"),
        token_key: Some("dns"),
        scopes: &["dns.write"],
        required: true,
        features: "The DNS records of routes, shares on your domains and reservations (per domain)",
        probed: true,
    },
    PermissionUse {
        name: Some("Zone · Zone · Read"),
        fix_key: Some("zones"),
        token_key: Some("zone"),
        scopes: &["zone.read"],
        required: true,
        features: "Listing your domains",
        probed: true,
    },
    PermissionUse {
        name: Some("Account · Account Settings · Read"),
        fix_key: None,
        token_key: Some("account_settings"),
        scopes: &["account-settings.read"],
        required: true,
        features: "Finding and naming the accounts a credential reaches",
        probed: false,
    },
    PermissionUse {
        name: Some("Account · Access: Apps and Policies · Edit"),
        fix_key: Some("accessApps"),
        token_key: Some("access"),
        scopes: &[
            "access-app.write",
            "access-policy.write",
            "zone-access.write",
        ],
        required: false,
        features: "[Logins](/docs/guides/require-login/) in front of routes, shares and Snapshots (`--allow`)",
        probed: true,
    },
    PermissionUse {
        name: Some("Account · Access: Organizations, Identity Providers, and Groups · Edit"),
        fix_key: Some("accessOrg"),
        token_key: Some("access_acct"),
        scopes: &["access-acct.write"],
        required: false,
        features: "Logins: adding the one-time code login method when the account has none",
        probed: true,
    },
    PermissionUse {
        name: Some("Zone · Analytics · Read"),
        fix_key: Some("analytics"),
        token_key: Some("analytics"),
        scopes: &["analytics.read"],
        required: false,
        features: "[Traffic charts](/docs/guides/analytics/) per route and `teitunnel analytics`",
        probed: true,
    },
    PermissionUse {
        name: Some("Account · Account Analytics · Read"),
        fix_key: Some("accountAnalytics"),
        token_key: Some("account_analytics"),
        scopes: &["account-analytics.read"],
        required: false,
        features: "Traffic numbers Cloudflare keeps per account",
        probed: false,
    },
    PermissionUse {
        name: Some("Account · Workers Scripts · Edit"),
        fix_key: Some("workers"),
        token_key: Some("workers_scripts"),
        scopes: &["workers-scripts.write"],
        required: false,
        features: "[Snapshots](/docs/guides/snapshots/), [offline pages](/docs/guides/offline-page/) and [webhook inboxes](/docs/guides/webhook-inbox/) (Workers on your account)",
        probed: true,
    },
    PermissionUse {
        name: Some("Zone · Workers Routes · Edit"),
        fix_key: Some("workersRoutes"),
        token_key: Some("workers_routes"),
        scopes: &["workers-routes.write"],
        required: false,
        features: "A hostname on your domain for a Snapshot, an offline page or an inbox (per domain)",
        probed: true,
    },
    PermissionUse {
        name: Some("Zone · Zone WAF · Edit"),
        fix_key: Some("zoneWaf"),
        token_key: Some("zone_waf"),
        scopes: &["zone-waf.write"],
        required: false,
        features: "[Edge protection](/docs/guides/protection/): bot and AI crawler rules, rate limits",
        probed: true,
    },
    PermissionUse {
        name: Some("Zone · Transform Rules · Edit"),
        fix_key: Some("transformRules"),
        token_key: Some("zone_transform_rules"),
        scopes: &["zone-transform-rules.write"],
        required: false,
        features: "Edge protection: request and response header rules",
        probed: true,
    },
    PermissionUse {
        name: Some("Account · Access: Service Tokens · Edit"),
        fix_key: Some("serviceTokens"),
        token_key: Some("access_service_token"),
        scopes: &["access-service-token.write"],
        required: false,
        features: "Service tokens that let machines through a login (`teitunnel service-token`)",
        probed: true,
    },
    PermissionUse {
        name: Some("Account · D1 · Edit"),
        fix_key: Some("d1"),
        token_key: Some("d1"),
        scopes: &["d1.write"],
        required: false,
        features: "[Snapshot comments](/docs/guides/comments/) and webhook inboxes (a D1 database on your account)",
        probed: true,
    },
    PermissionUse {
        name: Some("Account · Load Balancing: Monitors and Pools · Edit"),
        fix_key: Some("lbPools"),
        token_key: None,
        scopes: &["load-balancing-monitors-and-pools.write"],
        required: false,
        features: "[Load balancing](/docs/guides/load-balancing/) a route across machines (a paid add-on)",
        probed: false,
    },
    PermissionUse {
        name: Some("Zone · Load Balancers · Edit"),
        fix_key: Some("lbBalancers"),
        token_key: None,
        scopes: &["load-balancers.write"],
        required: false,
        features: "Load balancing a route across machines (a paid add-on)",
        probed: false,
    },
    PermissionUse {
        name: None,
        fix_key: None,
        token_key: None,
        scopes: &["teams-networks.write"],
        required: false,
        features: "[Private networks](/docs/guides/private-networks/) (with an API token, Cloudflare Tunnel · Edit covers them)",
        probed: false,
    },
    PermissionUse {
        name: None,
        fix_key: None,
        token_key: None,
        scopes: &["teams.read"],
        required: false,
        features: "The Doctor's WARP checks (read only)",
        probed: false,
    },
    PermissionUse {
        name: None,
        fix_key: None,
        token_key: None,
        scopes: &["offline_access"],
        required: false,
        features: "Keeping the sign-in (a refresh token)",
        probed: false,
    },
];

/// The pre-filled token link's permissions, `(key, type)`.
#[doc(hidden)]
pub fn token_template_permissions() -> &'static [(&'static str, &'static str)] {
    super::template::PERMISSIONS
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
