use cf_api::{DnsRecord, IngressRule, NewAccessApp};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::domain::{Hostname, PathRule, PrivateNetwork, RouteOrigin};
use crate::text::Text;

/// The DNS comment that marks a record as created by Teitunnel for a route.
pub fn ownership_comment(route_id: &str) -> String {
    format!("teitunnel:route={route_id}")
}

/// The CNAME target of a tunnel.
pub fn tunnel_target(tunnel_id: &str) -> String {
    format!("{tunnel_id}.cfargotunnel.com")
}

/// A route as the user describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RouteSpec {
    /// Stable id (written into the DNS comment).
    pub id: String,
    /// Public hostname.
    pub hostname: Hostname,
    /// Optional path regex.
    pub path: Option<PathRule>,
    /// Where traffic goes.
    pub origin: RouteOrigin,
    /// Extra `originRequest` settings (kept as-is).
    pub options: Map<String, Value>,
    /// Require a login (Cloudflare Access) for these people.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<super::access::AccessRule>,
}

impl RouteSpec {
    pub(crate) fn to_rule(&self) -> IngressRule {
        IngressRule {
            hostname: Some(self.hostname.to_string()),
            path: self.path.as_ref().map(|p| p.as_str().to_owned()),
            service: self.origin.to_string(),
            origin_request: self.options.clone(),
            extra: Map::new(),
        }
    }
}

/// A zone the account can use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ZoneRef {
    /// Zone id.
    pub id: String,
    /// Apex name.
    pub name: String,
}

impl AsRef<str> for ZoneRef {
    fn as_ref(&self) -> &str {
        &self.name
    }
}

/// The machine tunnel as observed.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ObservedTunnel {
    /// Tunnel id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Config version at observation time.
    pub config_version: u64,
    /// Current ingress rules (including the catch-all).
    pub ingress: Vec<IngressRule>,
}

/// A route on another of this Mac's tunnels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RouteElsewhere {
    /// That tunnel's name.
    pub tunnel: String,
    /// Hostname.
    pub hostname: String,
    /// Path, if any.
    pub path: Option<String>,
}

/// A DNS record in one of the zones, with whether Teitunnel owns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedRecord {
    /// Zone id.
    pub zone_id: String,
    /// The record.
    pub record: DnsRecord,
    /// Created by Teitunnel (ownership comment or ownership index).
    pub owned: bool,
}

/// A consistent read of everything a plan depends on.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Snapshot {
    /// Account id.
    pub account_id: String,
    /// Name to give a new machine tunnel.
    pub machine_name: String,
    /// Zones in the account.
    pub zones: Vec<ZoneRef>,
    /// This Mac's tunnel, if it exists.
    pub tunnel: Option<ObservedTunnel>,
    /// Names of the account's tunnels, read only when a tunnel will be created (it gets
    /// a name no other tunnel has).
    pub tunnel_names: Vec<String>,
    /// Routes on this Mac's other tunnels in the account (a hostname is routed once).
    pub elsewhere: Vec<RouteElsewhere>,
    /// DNS records for the hostnames involved.
    pub records: Vec<ObservedRecord>,
    /// Access for the domains involved; read only when a change involves a login.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access: Option<super::access::AccessState>,
    /// Private network routes; read only when a change involves them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub networks: Option<super::networks::NetworkState>,
    /// Load balancing for the hostname involved; read only when a change involves it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub balance: Option<super::balance::BalanceState>,
}

impl Snapshot {
    /// A hash of everything a plan depends on. If it changed between planning and
    /// applying, the executor re-plans (staleness guard).
    pub fn fingerprint(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        Sha256::digest(json)
            .iter()
            .fold(String::with_capacity(64), |mut out, b| {
                use std::fmt::Write;
                let _ = write!(out, "{b:02x}");
                out
            })
    }

    pub(crate) fn records_named<'a>(
        &'a self,
        name: &'a str,
    ) -> impl Iterator<Item = &'a ObservedRecord> + 'a {
        self.records
            .iter()
            .filter(move |r| r.record.name.eq_ignore_ascii_case(name))
    }

    pub(crate) fn routes(&self) -> Vec<&IngressRule> {
        self.tunnel
            .iter()
            .flat_map(|t| t.ingress.iter())
            .filter(|rule| rule.hostname.is_some())
            .collect()
    }
}

/// What the user asked for.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Intent {
    /// Add a new route.
    AddRoute {
        /// The route.
        route: RouteSpec,
    },
    /// Change a route (origin, options, path, or hostname — a rename).
    UpdateRoute {
        /// Current hostname.
        hostname: Hostname,
        /// Current path.
        path: Option<PathRule>,
        /// The new definition.
        route: RouteSpec,
    },
    /// Remove a route (and its DNS record, if Teitunnel owns it).
    RemoveRoute {
        /// Hostname.
        hostname: Hostname,
        /// Path.
        path: Option<PathRule>,
    },
    /// Remove every route and delete the tunnel.
    RemoveTunnel,
    /// Create another tunnel for this Mac (routes can then be put on it).
    CreateTunnel {
        /// Its name.
        name: String,
    },
    /// Load balance a route across every tunnel that routes its hostname.
    BalanceRoute {
        /// Hostname.
        hostname: Hostname,
    },
    /// Stop load balancing a route (its DNS record serves it again).
    UnbalanceRoute {
        /// Hostname.
        hostname: Hostname,
    },
    /// Add several routes at once (importing an existing cloudflared setup). Routes
    /// that already exist unchanged are skipped.
    ImportRoutes {
        /// The routes.
        routes: Vec<RouteSpec>,
    },
    /// Delete one DNS record (Doctor cleanup of orphans).
    DeleteRecord {
        /// Zone id.
        zone_id: String,
        /// The record's name.
        hostname: Hostname,
        /// Record id.
        record_id: String,
    },
    /// Let WARP clients reach a private range through this Mac's tunnel.
    AddNetwork {
        /// The range.
        network: PrivateNetwork,
    },
    /// Stop routing a private range to this Mac's tunnel.
    RemoveNetwork {
        /// The range.
        network: PrivateNetwork,
    },
    /// Remove a login Teitunnel added whose route is gone (Doctor cleanup).
    RemoveLogin {
        /// The Access domain (hostname and optional path).
        domain: String,
    },
    /// Put back the routes Teitunnel last wrote, undoing an edit made elsewhere.
    RestoreConfig {
        /// The ingress Teitunnel last applied.
        ingress: Vec<IngressRule>,
    },
}

impl Intent {
    /// Hostnames whose DNS records the plan depends on (`None` = every routed hostname).
    pub fn hostnames(&self) -> Option<Vec<&Hostname>> {
        match self {
            Self::AddRoute { route } => Some(vec![&route.hostname]),
            Self::UpdateRoute {
                hostname, route, ..
            } => Some(vec![hostname, &route.hostname]),
            Self::RemoveRoute { hostname, .. }
            | Self::DeleteRecord { hostname, .. }
            | Self::BalanceRoute { hostname }
            | Self::UnbalanceRoute { hostname } => Some(vec![hostname]),
            Self::RemoveTunnel => None,
            Self::RestoreConfig { .. }
            | Self::RemoveLogin { .. }
            | Self::AddNetwork { .. }
            | Self::RemoveNetwork { .. }
            | Self::CreateTunnel { .. } => Some(Vec::new()),
            Self::ImportRoutes { routes } => Some(routes.iter().map(|r| &r.hostname).collect()),
        }
    }

    /// A one-line summary for the activity log.
    pub fn summary(&self) -> Text {
        use crate::text::msg::plan::summary as m;
        fn target(hostname: &Hostname, path: Option<&PathRule>) -> String {
            path.map_or_else(
                || hostname.to_string(),
                |p| format!("{hostname} {}", p.as_str()),
            )
        }
        match self {
            Self::AddRoute { route } => {
                m::add_route(target(&route.hostname, route.path.as_ref()), &route.origin)
            }
            Self::UpdateRoute {
                hostname,
                path,
                route,
            } => {
                let before = target(hostname, path.as_ref());
                let after = target(&route.hostname, route.path.as_ref());
                if before == after {
                    m::change_route(before, &route.origin)
                } else {
                    m::rename_route(before, after, &route.origin)
                }
            }
            Self::RemoveRoute { hostname, path } => {
                m::remove_route(target(hostname, path.as_ref()))
            }
            Self::RemoveTunnel => m::remove_tunnel(),
            Self::RestoreConfig { .. } => m::restore_config(),
            Self::DeleteRecord { hostname, .. } => m::delete_record(hostname),
            Self::RemoveLogin { domain } => m::remove_login(domain),
            Self::AddNetwork { network } => m::add_network(network),
            Self::RemoveNetwork { network } => m::remove_network(network),
            Self::ImportRoutes { routes } => m::import_routes(routes.len() as u64),
            Self::CreateTunnel { name } => m::create_tunnel(name),
            Self::BalanceRoute { hostname } => m::balance_route(hostname),
            Self::UnbalanceRoute { hostname } => m::unbalance_route(hostname),
        }
    }
}

/// Which monitor or pool a step refers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum LbRef {
    /// One that exists.
    Existing(String),
    /// The one created earlier in the same plan.
    Created,
}

/// One tunnel as a pool endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PoolEndpoint {
    /// The tunnel.
    pub tunnel: TunnelRef,
    /// Its name (the endpoint's name).
    pub name: String,
}

/// Which tunnel a step refers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum TunnelRef {
    /// An existing tunnel.
    Existing(String),
    /// The tunnel created earlier in the same plan.
    Created,
}

/// One change, in execution order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Step {
    /// Create the machine tunnel.
    CreateTunnel {
        /// Name.
        name: String,
    },
    /// Replace the tunnel's ingress rules.
    PutConfig {
        /// Target tunnel.
        tunnel: TunnelRef,
        /// New ingress (sorted, catch-all last).
        ingress: Vec<IngressRule>,
        /// Version the plan was made against (None for a new tunnel).
        expected_version: Option<u64>,
        /// Previous ingress, for rollback.
        previous: Vec<IngressRule>,
    },
    /// Create a proxied CNAME to the tunnel.
    CreateRecord {
        /// Zone id.
        zone_id: String,
        /// Record name.
        hostname: String,
        /// Target tunnel.
        tunnel: TunnelRef,
        /// Route id for the ownership comment.
        route_id: String,
    },
    /// Point an existing record at the tunnel.
    UpdateRecord {
        /// Zone id.
        zone_id: String,
        /// Record id.
        record_id: String,
        /// Record name.
        hostname: String,
        /// Target tunnel.
        tunnel: TunnelRef,
        /// Route id for the ownership comment.
        route_id: String,
        /// What it was, for rollback and review.
        previous: DnsRecord,
    },
    /// Delete a record.
    DeleteRecord {
        /// Zone id.
        zone_id: String,
        /// The record (for rollback and review).
        record: DnsRecord,
    },
    /// Stop this Mac's connector for the tunnel.
    StopConnector {
        /// Tunnel id.
        tunnel_id: String,
    },
    /// Delete the tunnel.
    DeleteTunnel {
        /// Tunnel id.
        tunnel_id: String,
    },
    /// Add One-time PIN as a login method (the account has none).
    AddLoginMethod,
    /// Create the Access application that requires a login for a route.
    CreateAccessApp {
        /// Its definition.
        app: NewAccessApp,
    },
    /// Change who can reach a protected route (or its domain, after a rename).
    UpdateAccessApp {
        /// Application id.
        id: String,
        /// The new definition.
        app: NewAccessApp,
        /// What it was, for rollback.
        previous: NewAccessApp,
    },
    /// Remove the login from a route.
    DeleteAccessApp {
        /// Application id.
        id: String,
        /// What it was, for rollback.
        previous: NewAccessApp,
    },
    /// Route a private range to the tunnel (default virtual network).
    CreateNetworkRoute {
        /// The range.
        network: PrivateNetwork,
        /// Target tunnel.
        tunnel: TunnelRef,
    },
    /// Remove a private range's route.
    DeleteNetworkRoute {
        /// The route (for rollback and review).
        route: super::networks::ObservedNetworkRoute,
    },
    /// Create the health monitor for a balanced hostname.
    CreateLbMonitor {
        /// Hostname.
        hostname: String,
    },
    /// Create the pool of tunnels serving a hostname.
    CreateLbPool {
        /// Hostname.
        hostname: String,
        /// Its monitor.
        monitor: LbRef,
        /// Endpoints.
        endpoints: Vec<PoolEndpoint>,
    },
    /// Change which tunnels a hostname's pool sends traffic to.
    UpdateLbPool {
        /// Hostname.
        hostname: String,
        /// The pool.
        id: String,
        /// Its monitor.
        monitor: LbRef,
        /// Endpoints.
        endpoints: Vec<PoolEndpoint>,
        /// What it was, for rollback.
        previous: cf_api::Pool,
    },
    /// Put a load balancer in front of the hostname.
    CreateLoadBalancer {
        /// Zone id.
        zone_id: String,
        /// Hostname.
        hostname: String,
        /// Its pool.
        pool: LbRef,
    },
    /// Remove the hostname's load balancer.
    DeleteLoadBalancer {
        /// Zone id.
        zone_id: String,
        /// The load balancer, for rollback.
        balancer: cf_api::LoadBalancer,
    },
    /// Remove the hostname's pool.
    DeleteLbPool {
        /// The pool, for rollback.
        pool: cf_api::Pool,
    },
    /// Remove the hostname's health monitor.
    DeleteLbMonitor {
        /// The monitor, for rollback.
        monitor: cf_api::Monitor,
    },
    /// Probe the hostname end to end.
    Verify {
        /// Hostname.
        hostname: String,
    },
}

impl Step {
    /// A one-line description for the plan preview.
    pub fn describe(&self, tunnel_name: &str) -> Text {
        use crate::text::msg::plan::step as m;
        let people = |app: &NewAccessApp| {
            super::access::AccessRule::from_new(app).map_or_else(String::new, |r| r.people())
        };
        match self {
            Self::CreateTunnel { name } => m::create_tunnel(name),
            Self::PutConfig { ingress, .. } => m::put_config(
                ingress.iter().filter(|r| r.hostname.is_some()).count() as u64,
                tunnel_name,
            ),
            Self::CreateRecord { hostname, .. } => m::create_record(hostname, tunnel_name),
            Self::UpdateRecord {
                hostname, previous, ..
            } => m::update_record(hostname, tunnel_name, &previous.kind, &previous.content),
            Self::DeleteRecord { record, .. } => {
                m::delete_record(&record.name, &record.kind, &record.content)
            }
            Self::StopConnector { .. } => m::stop_connector(),
            Self::DeleteTunnel { .. } => m::delete_tunnel(tunnel_name),
            Self::AddLoginMethod => m::add_login_method(),
            Self::CreateAccessApp { app } => m::create_access_app(&app.domain, people(app)),
            Self::UpdateAccessApp { app, .. } => m::update_access_app(people(app), &app.domain),
            Self::DeleteAccessApp { previous, .. } => m::delete_access_app(&previous.domain),
            Self::CreateNetworkRoute { network, .. } => {
                m::create_network_route(network, tunnel_name)
            }
            Self::DeleteNetworkRoute { route } => m::delete_network_route(&route.network),
            Self::CreateLbMonitor { hostname } => m::create_lb_monitor(hostname),
            Self::CreateLbPool {
                hostname,
                endpoints,
                ..
            } => m::create_lb_pool(endpoints.len() as u64, hostname),
            Self::UpdateLbPool {
                hostname,
                endpoints,
                ..
            } => m::update_lb_pool(endpoints.len() as u64, hostname),
            Self::CreateLoadBalancer { hostname, .. } => m::create_load_balancer(hostname),
            Self::DeleteLoadBalancer { balancer, .. } => m::delete_load_balancer(&balancer.name),
            Self::DeleteLbPool { pool } => m::delete_lb_pool(&pool.name),
            Self::DeleteLbMonitor { .. } => m::delete_lb_monitor(),
            Self::Verify { hostname } => m::verify(hostname),
        }
    }

    /// An equivalent shell command for "Copy as command" (secrets as variables).
    pub fn command(&self, account_id: &str, tunnel_name: &str) -> Option<String> {
        const API: &str = "https://api.cloudflare.com/client/v4";
        let auth = r#"-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN""#;
        match self {
            Self::CreateTunnel { name } => Some(format!("cloudflared tunnel create '{name}'")),
            Self::CreateRecord { hostname, .. } => Some(format!(
                "cloudflared tunnel route dns '{tunnel_name}' {hostname}"
            )),
            Self::DeleteRecord { zone_id, record } => Some(format!(
                "curl -X DELETE {auth} {API}/zones/{zone_id}/dns_records/{}",
                record.id
            )),
            Self::PutConfig {
                tunnel: TunnelRef::Existing(id),
                ingress,
                ..
            } => {
                let body = serde_json::json!({ "config": { "ingress": ingress } });
                Some(format!(
                    "curl -X PUT {auth} -H 'Content-Type: application/json' {API}/accounts/{account_id}/cfd_tunnel/{id}/configurations --data '{body}'"
                ))
            }
            Self::DeleteTunnel { .. } => Some(format!("cloudflared tunnel delete '{tunnel_name}'")),
            Self::CreateAccessApp { app } => {
                let body = serde_json::to_string(app).unwrap_or_default();
                Some(format!(
                    "curl -X POST {auth} -H 'Content-Type: application/json' {API}/accounts/{account_id}/access/apps --data '{body}'"
                ))
            }
            Self::UpdateAccessApp { id, app, .. } => {
                let body = serde_json::to_string(app).unwrap_or_default();
                Some(format!(
                    "curl -X PUT {auth} -H 'Content-Type: application/json' {API}/accounts/{account_id}/access/apps/{id} --data '{body}'"
                ))
            }
            Self::DeleteAccessApp { id, .. } => Some(format!(
                "curl -X DELETE {auth} {API}/accounts/{account_id}/access/apps/{id}"
            )),
            Self::CreateNetworkRoute { network, .. } => Some(format!(
                "cloudflared tunnel route ip add {network} '{tunnel_name}'"
            )),
            Self::DeleteNetworkRoute { route } => Some(format!(
                "cloudflared tunnel route ip delete {}",
                route.network
            )),
            Self::Verify { hostname } => Some(format!("curl -I https://{hostname}")),
            _ => None,
        }
    }

    /// Whether this step changes something in Cloudflare (verify and connector steps don't).
    pub fn is_mutation(&self) -> bool {
        !matches!(self, Self::Verify { .. } | Self::StopConnector { .. })
    }
}

/// Something the user should know before applying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Warning {
    /// Only one tunnel serves the hostname: there's nothing to fail over to yet.
    SingleEndpoint {
        /// Hostname.
        hostname: String,
    },
    /// A record Teitunnel didn't create will be replaced.
    ReplacesForeignRecord {
        /// Hostname.
        hostname: String,
        /// Existing type.
        kind: String,
        /// Existing content.
        content: String,
    },
    /// A record Teitunnel didn't create will be deleted.
    DeletesForeignRecord {
        /// Hostname.
        hostname: String,
        /// Type.
        kind: String,
        /// Content.
        content: String,
    },
    /// A record Teitunnel didn't create is left in place.
    KeepsForeignRecord {
        /// Hostname.
        hostname: String,
    },
    /// No routes are left on the tunnel.
    TunnelEmpty,
    /// The origin isn't on this Mac, so it must be reachable from here.
    RemoteOrigin {
        /// The origin.
        origin: String,
    },
    /// The range isn't private address space: WARP clients would send traffic for those
    /// public addresses to this Mac instead of the internet.
    PublicNetwork {
        /// The range.
        network: String,
    },
    /// Part of the range is already routed to another tunnel; the more specific route
    /// wins for the addresses both cover.
    OverlapsNetwork {
        /// The range being added.
        network: String,
        /// The other route's range.
        other: String,
        /// The other route's tunnel.
        tunnel: String,
    },
}

/// A reviewed, ordered set of steps.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// Steps in execution order. Empty means "nothing to change".
    pub steps: Vec<Step>,
    /// Things to review.
    pub warnings: Vec<Warning>,
    /// Must the user confirm (it touches something Teitunnel doesn't own)?
    pub requires_confirmation: bool,
    /// Snapshot fingerprint the plan was made against.
    pub fingerprint: String,
    /// The machine tunnel's name (existing, or the one it'll be created with).
    pub tunnel_name: String,
}

impl Plan {
    /// Whether there's nothing to do (ignoring verification).
    pub fn is_empty(&self) -> bool {
        self.steps.iter().all(|s| !s.is_mutation())
    }
}
