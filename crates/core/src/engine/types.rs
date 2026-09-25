use cf_api::{DnsRecord, IngressRule, NewAccessApp};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::sites::{SiteContent, SiteFile, SiteSettings, SiteSpec};
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
    /// A Snapshot's Worker; read only when a change involves one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site: Option<super::sites::SiteState>,
    /// The hostnames involved that someone else holds (another machine's route, or a
    /// reservation that hasn't ended).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<super::ownership::Hold>,
    /// Who this is, written into the DNS comments Teitunnel makes (not part of the
    /// fingerprint).
    #[serde(skip)]
    pub owner: String,
    /// When it was observed (milliseconds since the epoch; not part of the fingerprint).
    #[serde(skip)]
    pub now: u64,
    /// Zones' edge rules, one entry per zone; read only when a change protects a
    /// hostname or removes routes from zones where Teitunnel has rules.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edge: Vec<super::edge::EdgeState>,
    /// The account's Access service tokens; read only when a change involves one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_tokens: Option<Vec<super::edge::ObservedServiceToken>>,
    /// The account's D1 database for comments and inboxes; read only when a change
    /// needs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub database: Option<super::front::DatabaseState>,
    /// Worker routes on hostnames (offline page, webhook inbox), one entry per
    /// hostname; read only when a change involves them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub front: Vec<super::front::FrontState>,
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

    /// The edge rules observed for zone `zone_id`.
    pub(crate) fn edge_in(&self, zone_id: &str) -> Option<&super::edge::EdgeState> {
        self.edge.iter().find(|e| e.zone_id == zone_id)
    }

    /// The Workers observed in front of `hostname`.
    pub(crate) fn front_of(&self, hostname: &str) -> Option<&super::front::FrontState> {
        self.front
            .iter()
            .find(|f| f.hostname.eq_ignore_ascii_case(hostname))
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
    /// Remove what Teitunnel attached to a hostname that no longer has a route: its
    /// front Workers, edge rules, service tokens and login.
    CleanUpHostname {
        /// The hostname.
        hostname: Hostname,
    },
    /// Put back the routes Teitunnel last wrote, undoing an edit made elsewhere.
    RestoreConfig {
        /// The ingress Teitunnel last applied.
        ingress: Vec<IngressRule>,
    },
    /// Publish a new Snapshot.
    PublishSnapshot {
        /// The Snapshot.
        site: SiteSpec,
        /// How it answers.
        settings: SiteSettings,
        /// Its files.
        content: SiteContent,
    },
    /// Publish a new version of a Snapshot (new files and/or settings), and bring its
    /// login in line with `site.access`.
    UpdateSnapshot {
        /// The Snapshot, with the login it should have.
        site: SiteSpec,
        /// How the new version answers.
        settings: SiteSettings,
        /// Its files.
        content: SiteContent,
        /// The live version's files (to count what changed).
        previous: Vec<SiteFile>,
    },
    /// Make an earlier version of a Snapshot live again.
    RollbackSnapshot {
        /// The Snapshot.
        site: SiteSpec,
        /// Cloudflare's id of the version.
        version_id: String,
        /// Its number, for messages.
        number: u32,
    },
    /// Delete a Snapshot: its address, login and Worker.
    DeleteSnapshot {
        /// The Snapshot.
        site: SiteSpec,
    },
    /// Reserve a hostname for this owner (a lease in DNS, M12-11).
    Reserve {
        /// The hostname.
        hostname: Hostname,
        /// When the lease ends (milliseconds since the epoch); `None`: never.
        until: Option<u64>,
    },
    /// Give up the reservation of a hostname.
    Release {
        /// The hostname.
        hostname: Hostname,
    },
    /// Make Cloudflare's edge enforce `protection` for one hostname (bots, AI crawlers,
    /// a rate limit, header rules); the default removes Teitunnel's rules.
    ProtectHostname {
        /// The hostname.
        hostname: Hostname,
        /// What to enforce.
        protection: super::edge::EdgeProtection,
    },
    /// Create a service token that passes the hostname's login (for machines).
    CreateServiceToken {
        /// The hostname.
        hostname: Hostname,
        /// What it's for, e.g. `CI`.
        label: String,
    },
    /// Stop a service token passing the hostname's login and delete it.
    RevokeServiceToken {
        /// The hostname.
        hostname: Hostname,
        /// Token id.
        token_id: String,
    },
    /// Give a service token a new secret.
    RotateServiceToken {
        /// The hostname.
        hostname: Hostname,
        /// Token id.
        token_id: String,
    },
    /// Show a page of the person's own instead of Cloudflare's error while this computer
    /// is off (`None` removes it).
    SetOfflinePage {
        /// The hostname.
        hostname: Hostname,
        /// The page.
        page: Option<super::front::OfflinePage>,
    },
    /// Keep webhooks to a path while this computer is off and deliver them later
    /// (`None` removes the inbox; waiting webhooks stay until their retention ends).
    SetInbox {
        /// The hostname.
        hostname: Hostname,
        /// The path, e.g. `/webhooks/`.
        path: String,
        /// Its settings.
        inbox: Option<super::front::InboxSettings>,
        /// A new signing secret to send (from the keychain; never serialized). `None`
        /// keeps the one the Worker has.
        #[serde(skip)]
        secret: Option<crate::Secret<String>>,
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
            | Self::UnbalanceRoute { hostname }
            | Self::Reserve { hostname, .. }
            | Self::Release { hostname }
            | Self::ProtectHostname { hostname, .. }
            | Self::CreateServiceToken { hostname, .. }
            | Self::RevokeServiceToken { hostname, .. }
            | Self::RotateServiceToken { hostname, .. }
            | Self::SetOfflinePage { hostname, .. }
            | Self::SetInbox { hostname, .. }
            | Self::CleanUpHostname { hostname } => Some(vec![hostname]),
            Self::RemoveTunnel => None,
            Self::RestoreConfig { .. }
            | Self::RemoveLogin { .. }
            | Self::AddNetwork { .. }
            | Self::RemoveNetwork { .. }
            | Self::CreateTunnel { .. } => Some(Vec::new()),
            Self::ImportRoutes { routes } => Some(routes.iter().map(|r| &r.hostname).collect()),
            Self::PublishSnapshot { site, .. }
            | Self::UpdateSnapshot { site, .. }
            | Self::RollbackSnapshot { site, .. }
            | Self::DeleteSnapshot { site } => Some(site.address.hostname().into_iter().collect()),
        }
    }

    /// The Snapshot a change is about.
    pub fn site(&self) -> Option<&SiteSpec> {
        match self {
            Self::PublishSnapshot { site, .. }
            | Self::UpdateSnapshot { site, .. }
            | Self::RollbackSnapshot { site, .. }
            | Self::DeleteSnapshot { site } => Some(site),
            _ => None,
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
            Self::CleanUpHostname { hostname } => m::clean_up_hostname(hostname.as_str()),
            Self::AddNetwork { network } => m::add_network(network),
            Self::RemoveNetwork { network } => m::remove_network(network),
            Self::ImportRoutes { routes } => m::import_routes(routes.len() as u64),
            Self::CreateTunnel { name } => m::create_tunnel(name),
            Self::BalanceRoute { hostname } => m::balance_route(hostname),
            Self::UnbalanceRoute { hostname } => m::unbalance_route(hostname),
            Self::PublishSnapshot { site, content, .. } => {
                crate::text::msg::snapshot::summary::publish(
                    content.files.len() as u64,
                    &site.name,
                    site.label(),
                )
            }
            Self::UpdateSnapshot { site, .. } => {
                crate::text::msg::snapshot::summary::update(&site.name)
            }
            Self::RollbackSnapshot { site, number, .. } => {
                crate::text::msg::snapshot::summary::rollback(&site.name, u64::from(*number))
            }
            Self::DeleteSnapshot { site } => {
                crate::text::msg::snapshot::summary::delete(&site.name)
            }
            Self::Reserve { hostname, until } => match until {
                Some(until) => crate::text::msg::reservations::summary::reserve_until(
                    hostname,
                    super::ownership::format_until(*until),
                ),
                None => crate::text::msg::reservations::summary::reserve(hostname),
            },
            Self::Release { hostname } => {
                crate::text::msg::reservations::summary::release(hostname)
            }
            Self::ProtectHostname {
                hostname,
                protection,
            } => {
                if protection.is_off() {
                    crate::text::msg::protection::summary::unprotect(hostname)
                } else {
                    crate::text::msg::protection::summary::protect(hostname)
                }
            }
            Self::CreateServiceToken { hostname, label } => {
                crate::text::msg::protection::summary::create_token(label, hostname)
            }
            Self::RevokeServiceToken { hostname, .. } => {
                crate::text::msg::protection::summary::revoke_token(hostname)
            }
            Self::RotateServiceToken { hostname, .. } => {
                crate::text::msg::protection::summary::rotate_token(hostname)
            }
            Self::SetOfflinePage { hostname, page } => match page {
                Some(_) => crate::text::msg::front::summary::offline_on(hostname),
                None => crate::text::msg::front::summary::offline_off(hostname),
            },
            Self::SetInbox {
                hostname,
                path,
                inbox,
                ..
            } => match inbox {
                Some(_) => crate::text::msg::front::summary::inbox_on(format!("{hostname}{path}")),
                None => crate::text::msg::front::summary::inbox_off(format!("{hostname}{path}")),
            },
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

/// Which service token a step refers to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum TokenRef {
    /// One that exists.
    Existing(String),
    /// The one created earlier in the same plan.
    Created,
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
    /// Upload the files of a Snapshot version that Cloudflare doesn't have yet.
    UploadSnapshotFiles {
        /// The Worker.
        script: String,
        /// The files.
        content: SiteContent,
        /// Files new or changed since the live version.
        changed_files: u64,
        /// Their size.
        changed_bytes: u64,
    },
    /// Create the Snapshot's Worker with the uploaded files (live at once; it has no
    /// address yet).
    CreateSnapshotWorker {
        /// Local Snapshot id.
        snapshot: String,
        /// The Worker.
        script: String,
        /// How it answers.
        settings: SiteSettings,
    },
    /// Upload a new version with the uploaded files, then make it live.
    PublishSnapshotVersion {
        /// Local Snapshot id.
        snapshot: String,
        /// The Worker.
        script: String,
        /// How it answers.
        settings: SiteSettings,
        /// The version live before, for rollback.
        previous: Option<String>,
    },
    /// Make an earlier version live again.
    RollBackSnapshot {
        /// Local Snapshot id.
        snapshot: String,
        /// The Worker.
        script: String,
        /// The version to make live.
        version_id: String,
        /// Its number.
        number: u32,
        /// The version live before, for rollback.
        previous: Option<String>,
    },
    /// Answer on the account's workers.dev subdomain.
    EnableWorkersDev {
        /// The Worker.
        script: String,
        /// The address, for review.
        address: String,
    },
    /// Stop answering on workers.dev.
    DisableWorkersDev {
        /// The Worker.
        script: String,
        /// The address, for review.
        address: String,
    },
    /// Serve a hostname with the Snapshot (Cloudflare adds the DNS record).
    AttachSnapshotDomain {
        /// Zone id.
        zone_id: String,
        /// The hostname.
        hostname: String,
        /// The Worker.
        script: String,
    },
    /// Stop serving a hostname with the Snapshot.
    DetachSnapshotDomain {
        /// The Custom Domain, for rollback.
        domain: cf_api::WorkerDomain,
    },
    /// Delete the Snapshot's Worker with every version (always the last step).
    DeleteSnapshotWorker {
        /// Local Snapshot id.
        snapshot: String,
        /// The Worker.
        script: String,
    },
    /// Hold a hostname nobody routes with a placeholder record (a proxied `AAAA 100::`
    /// carrying the lease in its comment).
    CreateReservation {
        /// Zone id.
        zone_id: String,
        /// The hostname.
        hostname: String,
        /// When the lease ends (milliseconds since the epoch); `None`: never.
        until: Option<u64>,
    },
    /// Change the lease a Teitunnel record carries (only its comment changes).
    SetLease {
        /// Zone id.
        zone_id: String,
        /// The record as it is (for rollback and review).
        record: DnsRecord,
        /// Whether it holds a reservation afterwards.
        lease: bool,
        /// When the lease ends.
        until: Option<u64>,
    },
    /// Add one of Teitunnel's rules to a phase (creating its entry point when the zone
    /// has none).
    CreateEdgeRule {
        /// Zone id.
        zone_id: String,
        /// The phase.
        phase: String,
        /// Its entry point ruleset, if the zone has one.
        ruleset_id: Option<String>,
        /// Which rule.
        kind: super::edge::RuleKind,
        /// The hostnames it covers.
        hostnames: Vec<String>,
        /// The rule.
        rule: cf_api::NewRule,
    },
    /// Change one of Teitunnel's rules.
    UpdateEdgeRule {
        /// Zone id.
        zone_id: String,
        /// The phase's entry point ruleset.
        ruleset_id: String,
        /// Rule id.
        rule_id: String,
        /// Which rule.
        kind: super::edge::RuleKind,
        /// The hostnames it covers afterwards.
        hostnames: Vec<String>,
        /// The new definition.
        rule: cf_api::NewRule,
        /// What it was, for rollback.
        previous: cf_api::NewRule,
    },
    /// Remove one of Teitunnel's rules.
    DeleteEdgeRule {
        /// Zone id.
        zone_id: String,
        /// The phase.
        phase: String,
        /// The phase's entry point ruleset.
        ruleset_id: String,
        /// Rule id.
        rule_id: String,
        /// Which rule.
        kind: super::edge::RuleKind,
        /// The hostnames it covered.
        hostnames: Vec<String>,
        /// What it was, for rollback.
        previous: cf_api::NewRule,
        /// Its 1-based position, to put it back there.
        position: u32,
    },
    /// Create an Access service token (its secret is shown once).
    CreateServiceToken {
        /// The hostname it's for.
        hostname: String,
        /// Its name.
        name: String,
    },
    /// Let a service token through the hostname's login: adds it to the "Machines"
    /// (Service Auth) policy of Teitunnel's Access application, creating an application
    /// only machines pass when there's none.
    AllowServiceToken {
        /// The Access domain.
        domain: String,
        /// Teitunnel's application for it, if there is one, and its definition.
        app: Option<(String, NewAccessApp)>,
        /// The token.
        token: TokenRef,
    },
    /// Delete a service token (always after its policy stopped using it).
    DeleteServiceToken {
        /// The token.
        token: super::edge::ObservedServiceToken,
    },
    /// Give a service token a new secret.
    RotateServiceToken {
        /// The token.
        token: super::edge::ObservedServiceToken,
    },
    /// Create the account's D1 database for comments and webhook inboxes, with its
    /// tables.
    CreateDatabase {
        /// Its name.
        name: String,
    },
    /// Upload (or replace) a Worker in front of a route: the offline page or a webhook
    /// inbox. It serves nothing until its route exists.
    PutFrontWorker {
        /// The hostname.
        hostname: String,
        /// Its zone.
        zone_id: String,
        /// The Worker.
        script: String,
        /// What it's deployed with.
        config: super::front::FrontConfig,
        /// What it had before, when it's replaced (for rollback).
        previous: Option<super::front::FrontConfig>,
        /// The D1 database (inboxes).
        database: Option<super::front::DatabaseRef>,
        /// A new signing secret (never serialized).
        #[serde(skip)]
        secret: Option<crate::Secret<String>>,
    },
    /// Run a front Worker for requests matching a pattern (fails open).
    CreateWorkerRoute {
        /// The hostname.
        hostname: String,
        /// Its zone.
        zone_id: String,
        /// E.g. `app.example.com/*`.
        pattern: String,
        /// The Worker.
        script: String,
        /// Which Worker.
        kind: super::front::FrontKind,
        /// The inbox path (`""` for the offline page).
        path: String,
    },
    /// Stop running a front Worker for a pattern.
    DeleteWorkerRoute {
        /// The hostname.
        hostname: String,
        /// Its zone.
        zone_id: String,
        /// The route (for rollback and review).
        route: cf_api::WorkerRoute,
        /// Which Worker.
        kind: super::front::FrontKind,
        /// The inbox path.
        path: String,
    },
    /// Delete a front Worker (after its route).
    DeleteFrontWorker {
        /// The hostname.
        hostname: String,
        /// Its zone.
        zone_id: String,
        /// The Worker.
        script: String,
        /// What it was deployed with (to put it back on rollback).
        previous: super::front::FrontConfig,
        /// The D1 database it used.
        database: Option<String>,
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
            Self::UploadSnapshotFiles {
                content,
                changed_files,
                changed_bytes,
                ..
            } => crate::text::msg::snapshot::step::upload(
                *changed_files,
                content.files.len() as u64,
                crate::text::msg::raw(super::sites::format_bytes(*changed_bytes)),
            ),
            Self::CreateSnapshotWorker { script, .. } => {
                crate::text::msg::snapshot::step::create_worker(script)
            }
            Self::PublishSnapshotVersion { .. } => crate::text::msg::snapshot::step::publish(),
            Self::RollBackSnapshot { number, .. } => {
                crate::text::msg::snapshot::step::roll_back(u64::from(*number))
            }
            Self::EnableWorkersDev { address, .. } => {
                crate::text::msg::snapshot::step::enable_workers_dev(address)
            }
            Self::DisableWorkersDev { address, .. } => {
                crate::text::msg::snapshot::step::disable_workers_dev(address)
            }
            Self::AttachSnapshotDomain { hostname, .. } => {
                crate::text::msg::snapshot::step::attach_domain(hostname)
            }
            Self::DetachSnapshotDomain { domain } => {
                crate::text::msg::snapshot::step::detach_domain(&domain.hostname)
            }
            Self::DeleteSnapshotWorker { script, .. } => {
                crate::text::msg::snapshot::step::delete_worker(script)
            }
            Self::CreateReservation {
                hostname, until, ..
            } => {
                use crate::text::msg::reservations::step as r;
                match until {
                    Some(until) => {
                        r::create_until(hostname, super::ownership::format_until(*until))
                    }
                    None => r::create(hostname),
                }
            }
            Self::SetLease {
                record,
                lease,
                until,
                ..
            } => {
                use crate::text::msg::reservations::step as r;
                match (lease, until) {
                    (true, Some(until)) => {
                        r::keep_until(&record.name, super::ownership::format_until(*until))
                    }
                    (true, None) => r::keep(&record.name),
                    (false, _) => r::end(&record.name),
                }
            }
            Self::CreateEdgeRule {
                kind,
                hostnames,
                rule,
                ..
            } => super::edge::describe_rule(super::edge::Verb::Add, *kind, hostnames, rule),
            Self::UpdateEdgeRule {
                kind,
                hostnames,
                rule,
                ..
            } => super::edge::describe_rule(super::edge::Verb::Change, *kind, hostnames, rule),
            Self::DeleteEdgeRule {
                kind,
                hostnames,
                previous,
                ..
            } => super::edge::describe_rule(super::edge::Verb::Remove, *kind, hostnames, previous),
            Self::CreateServiceToken { name, .. } => {
                crate::text::msg::protection::step::create_token(name)
            }
            Self::AllowServiceToken { domain, app, .. } => {
                if app.is_some() {
                    crate::text::msg::protection::step::allow_token(domain)
                } else {
                    crate::text::msg::protection::step::machine_only(domain)
                }
            }
            Self::DeleteServiceToken { token } => {
                crate::text::msg::protection::step::delete_token(&token.name)
            }
            Self::RotateServiceToken { token } => {
                crate::text::msg::protection::step::rotate_token(&token.name)
            }
            Self::CreateDatabase { name } => crate::text::msg::front::step::create_database(name),
            Self::PutFrontWorker {
                hostname,
                config,
                previous,
                ..
            } => {
                use crate::text::msg::front::step as f;
                match (config, previous.is_some()) {
                    (super::front::FrontConfig::Offline { .. }, false) => f::put_offline(hostname),
                    (super::front::FrontConfig::Offline { .. }, true) => {
                        f::update_offline(hostname)
                    }
                    (super::front::FrontConfig::Inbox { path, .. }, false) => {
                        f::put_inbox(format!("{hostname}{path}"))
                    }
                    (super::front::FrontConfig::Inbox { path, .. }, true) => {
                        f::update_inbox(format!("{hostname}{path}"))
                    }
                }
            }
            Self::CreateWorkerRoute { pattern, .. } => {
                crate::text::msg::front::step::create_route(pattern)
            }
            Self::DeleteWorkerRoute { route, .. } => {
                crate::text::msg::front::step::delete_route(&route.pattern)
            }
            Self::DeleteFrontWorker { script, .. } => {
                crate::text::msg::front::step::delete_worker(script)
            }
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
            Self::RollBackSnapshot {
                script, version_id, ..
            } => {
                let body = serde_json::json!({
                    "strategy": "percentage",
                    "versions": [{ "version_id": version_id, "percentage": 100 }],
                });
                Some(format!(
                    "curl -X POST {auth} -H 'Content-Type: application/json' {API}/accounts/{account_id}/workers/scripts/{script}/deployments --data '{body}'"
                ))
            }
            Self::EnableWorkersDev { script, .. } | Self::DisableWorkersDev { script, .. } => {
                let enabled = matches!(self, Self::EnableWorkersDev { .. });
                Some(format!(
                    "curl -X POST {auth} -H 'Content-Type: application/json' {API}/accounts/{account_id}/workers/scripts/{script}/subdomain --data '{{\"enabled\":{enabled}}}'"
                ))
            }
            Self::AttachSnapshotDomain {
                zone_id,
                hostname,
                script,
            } => {
                let body = serde_json::json!({ "hostname": hostname, "zone_id": zone_id, "service": script });
                Some(format!(
                    "curl -X PUT {auth} -H 'Content-Type: application/json' {API}/accounts/{account_id}/workers/domains --data '{body}'"
                ))
            }
            Self::DetachSnapshotDomain { domain } => Some(format!(
                "curl -X DELETE {auth} {API}/accounts/{account_id}/workers/domains/{}",
                domain.id
            )),
            Self::DeleteSnapshotWorker { script, .. } | Self::DeleteFrontWorker { script, .. } => {
                Some(format!(
                    "curl -X DELETE {auth} '{API}/accounts/{account_id}/workers/scripts/{script}?force=true'"
                ))
            }
            Self::CreateEdgeRule {
                zone_id,
                phase,
                ruleset_id,
                rule,
                ..
            } => {
                let body = serde_json::to_string(rule).unwrap_or_default();
                Some(match ruleset_id {
                    Some(id) => format!(
                        "curl -X POST {auth} -H 'Content-Type: application/json' {API}/zones/{zone_id}/rulesets/{id}/rules --data '{body}'"
                    ),
                    None => {
                        let ruleset = serde_json::json!({
                            "name": "default", "kind": "zone", "phase": phase, "rules": [rule],
                        });
                        format!(
                            "curl -X POST {auth} -H 'Content-Type: application/json' {API}/zones/{zone_id}/rulesets --data '{ruleset}'"
                        )
                    }
                })
            }
            Self::UpdateEdgeRule {
                zone_id,
                ruleset_id,
                rule_id,
                rule,
                ..
            } => {
                let body = serde_json::to_string(rule).unwrap_or_default();
                Some(format!(
                    "curl -X PATCH {auth} -H 'Content-Type: application/json' {API}/zones/{zone_id}/rulesets/{ruleset_id}/rules/{rule_id} --data '{body}'"
                ))
            }
            Self::DeleteEdgeRule {
                zone_id,
                ruleset_id,
                rule_id,
                ..
            } => Some(format!(
                "curl -X DELETE {auth} {API}/zones/{zone_id}/rulesets/{ruleset_id}/rules/{rule_id}"
            )),
            Self::DeleteServiceToken { token } => Some(format!(
                "curl -X DELETE {auth} {API}/accounts/{account_id}/access/service_tokens/{}",
                token.id
            )),
            Self::CreateDatabase { name } => {
                let body = serde_json::json!({ "name": name });
                Some(format!(
                    "curl -X POST {auth} -H 'Content-Type: application/json' {API}/accounts/{account_id}/d1/database --data '{body}'"
                ))
            }
            Self::CreateWorkerRoute {
                zone_id,
                pattern,
                script,
                ..
            } => {
                let body = serde_json::json!({
                    "pattern": pattern, "script": script, "request_limit_fail_open": true,
                });
                Some(format!(
                    "curl -X POST {auth} -H 'Content-Type: application/json' {API}/zones/{zone_id}/workers/routes --data '{body}'"
                ))
            }
            Self::DeleteWorkerRoute { zone_id, route, .. } => Some(format!(
                "curl -X DELETE {auth} {API}/zones/{zone_id}/workers/routes/{}",
                route.id
            )),
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
    /// Someone else holds the hostname (another machine's route, or a reservation that
    /// hasn't ended): going ahead takes it over, which needs a confirmation.
    HeldBy {
        /// Hostname.
        hostname: String,
        /// Who (`person@machine`); `None` when an older Teitunnel made it.
        owner: Option<String>,
        /// Until when (milliseconds since the epoch); `None`: no end.
        #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
        until: Option<u64>,
        /// Reserved or routed.
        kind: super::ownership::HoldKind,
    },
    /// How much of a plan quota the zone uses after the change.
    EdgeQuota {
        /// Which quota.
        quota: super::edge::QuotaKind,
        /// The zone.
        zone: String,
        /// Rules after the change (Teitunnel's and others').
        used: u32,
        /// What the zone's plan allows.
        limit: u32,
    },
    /// The hostname has no login, so the new one lets in only service tokens: people
    /// opening it in a browser are refused.
    MachineOnly {
        /// The Access domain.
        domain: String,
    },
    /// Every request matching the pattern runs a Worker, counted against the account's
    /// 100,000 free Worker requests a day (past it the site keeps working without it).
    WorkerRequests {
        /// The route pattern.
        pattern: String,
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
