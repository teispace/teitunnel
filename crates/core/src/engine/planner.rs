//! The pure planner: `plan(intent, snapshot) -> Plan`. No I/O, deterministic.
//!
//! Order of operations (ARCHITECTURE §4.3):
//! - add: create tunnel → config (the tunnel learns the hostname first, so there's no
//!   404 window) → DNS → verify
//! - remove: config → DNS (only records Teitunnel owns) → login → tunnel
//! - rename: config (swap the rule) → new DNS → delete the old owned record
//! - private network: create tunnel → route the range; removing the tunnel removes the
//!   ranges routed to it first
//!
//! A login (Access) goes up before the route goes live and comes down after it's gone,
//! so a protected route is never reachable without one.

use cf_api::IngressRule;

use super::{
    access::{AccessDomainError, AccessRule, access_domain, app_definition},
    ingress::sort_ingress,
    networks::{NetworkState, ObservedNetworkRoute},
    types::{
        Intent, ObservedRecord, Plan, RouteSpec, Snapshot, Step, TunnelRef, Warning, tunnel_target,
    },
};
use crate::domain::{Hostname, PathRule};

use crate::text::{Text, UserText, english_display, msg};

/// Why no plan could be made. Messages are shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// The hostname isn't in any zone of the account.
    NoZone(String),
    /// A route with this hostname and path already exists.
    RouteExists(String),
    /// No route with this hostname and path.
    NoSuchRoute(String),
    /// Nothing to remove.
    NoTunnel,
    /// The record is gone (or never existed).
    NoSuchRecord(String),
    /// Requiring a login needs Cloudflare Zero Trust.
    ZeroTrustNotSetUp,
    /// No login Teitunnel added covers the domain.
    NoSuchLogin(String),
    /// Someone else's Access application already covers the domain.
    AccessAppExists(String),
    /// The route's path can't be protected.
    #[error(transparent)]
    AccessDomain(#[from] AccessDomainError),
    /// The range is already routed to another tunnel.
    NetworkRouted {
        /// The range.
        network: String,
        /// The other tunnel.
        tunnel: String,
    },
    /// This Mac's tunnel doesn't route the range.
    NoSuchNetwork(String),
}

impl UserText for PlanError {
    fn text(&self) -> Text {
        match self {
            Self::AccessDomain(err) => err.text(),
            Self::NoZone(hostname) => msg::error::plan::no_zone(hostname),
            Self::RouteExists(hostname) => msg::error::plan::route_exists(hostname),
            Self::NoSuchRoute(hostname) => msg::error::plan::no_such_route(hostname),
            Self::NoTunnel => msg::error::plan::no_tunnel(),
            Self::NoSuchRecord(hostname) => msg::error::plan::no_such_record(hostname),
            Self::ZeroTrustNotSetUp => msg::error::plan::zero_trust_not_set_up(),
            Self::NoSuchLogin(domain) => msg::error::plan::no_such_login(domain),
            Self::AccessAppExists(domain) => msg::error::plan::access_app_exists(domain),
            Self::NetworkRouted {
                network, tunnel, ..
            } => msg::error::plan::network_routed(network, tunnel),
            Self::NoSuchNetwork(network) => msg::error::plan::no_such_network(network),
        }
    }
}

english_display!(PlanError);

fn same_route(rule: &IngressRule, hostname: &Hostname, path: Option<&PathRule>) -> bool {
    rule.hostname.as_deref() == Some(hostname.as_str())
        && rule.path.as_deref() == path.map(PathRule::as_str)
}

/// `base`, or `base 2`, `base 3`, … if another tunnel already has the name.
fn unique_name(base: &str, taken: &[String]) -> String {
    let free = |name: &str| !taken.iter().any(|t| t.eq_ignore_ascii_case(name));
    if free(base) {
        return base.to_owned();
    }
    (2..)
        .map(|n| format!("{base} {n}"))
        .find(|name| free(name))
        .unwrap_or_else(|| base.to_owned())
}

struct Builder<'a> {
    snapshot: &'a Snapshot,
    steps: Vec<Step>,
    warnings: Vec<Warning>,
    requires_confirmation: bool,
}

impl<'a> Builder<'a> {
    fn new(snapshot: &'a Snapshot) -> Self {
        Self {
            snapshot,
            steps: Vec::new(),
            warnings: Vec::new(),
            requires_confirmation: false,
        }
    }

    /// The tunnel to use, creating it first if this Mac has none.
    fn ensure_tunnel(&mut self) -> TunnelRef {
        match &self.snapshot.tunnel {
            Some(tunnel) => TunnelRef::Existing(tunnel.id.clone()),
            None => {
                self.steps.push(Step::CreateTunnel {
                    name: unique_name(&self.snapshot.machine_name, &self.snapshot.tunnel_names),
                });
                TunnelRef::Created
            }
        }
    }

    fn put_config(&mut self, tunnel: &TunnelRef, desired: Vec<IngressRule>) {
        let current = self
            .snapshot
            .tunnel
            .as_ref()
            .map(|t| t.ingress.clone())
            .unwrap_or_default();
        let desired = sort_ingress(desired);
        if desired != current {
            self.steps.push(Step::PutConfig {
                tunnel: tunnel.clone(),
                ingress: desired,
                expected_version: self.snapshot.tunnel.as_ref().map(|t| t.config_version),
                previous: current,
            });
        }
    }

    fn zone_id(&self, hostname: &Hostname) -> Result<String, PlanError> {
        hostname
            .zone_in(&self.snapshot.zones)
            .map(|zone| zone.id.clone())
            .ok_or_else(|| PlanError::NoZone(hostname.to_string()))
    }

    /// Makes `hostname` resolve to the tunnel.
    fn ensure_dns(
        &mut self,
        hostname: &Hostname,
        tunnel: &TunnelRef,
        route_id: &str,
    ) -> Result<(), PlanError> {
        let zone_id = self.zone_id(hostname)?;
        let target = match tunnel {
            TunnelRef::Existing(id) => Some(tunnel_target(id)),
            TunnelRef::Created => None,
        };
        let existing: Vec<&ObservedRecord> = self
            .snapshot
            .records_named(hostname.as_str())
            .filter(|r| matches!(r.record.kind.as_str(), "A" | "AAAA" | "CNAME"))
            .collect();
        let already_ours = existing.len() == 1
            && target.as_deref().is_some_and(|t| {
                existing[0].record.content.eq_ignore_ascii_case(t) && existing[0].record.proxied
            });
        if already_ours {
            return Ok(());
        }
        let mut records = existing.into_iter();
        match records.next() {
            None => self.steps.push(Step::CreateRecord {
                zone_id,
                hostname: hostname.to_string(),
                tunnel: tunnel.clone(),
                route_id: route_id.to_owned(),
            }),
            Some(first) => {
                self.flag_foreign(first);
                self.steps.push(Step::UpdateRecord {
                    zone_id: zone_id.clone(),
                    record_id: first.record.id.clone(),
                    hostname: hostname.to_string(),
                    tunnel: tunnel.clone(),
                    route_id: route_id.to_owned(),
                    previous: first.record.clone(),
                });
                // Extra A/AAAA records would still answer for the name: remove them.
                for extra in records {
                    self.flag_foreign(extra);
                    self.steps.push(Step::DeleteRecord {
                        zone_id: zone_id.clone(),
                        record: extra.record.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    fn flag_foreign(&mut self, record: &ObservedRecord) {
        if !record.owned {
            self.requires_confirmation = true;
            self.warnings.push(Warning::ReplacesForeignRecord {
                hostname: record.record.name.clone(),
                kind: record.record.kind.clone(),
                content: record.record.content.clone(),
            });
        }
    }

    /// Deletes the DNS record for `hostname` if Teitunnel owns it and it points at the
    /// tunnel. Foreign records are never deleted automatically (ARCHITECTURE §4.7).
    fn release_dns(&mut self, hostname: &str, tunnel_id: &str) {
        let target = tunnel_target(tunnel_id);
        let matching: Vec<ObservedRecord> = self
            .snapshot
            .records_named(hostname)
            .filter(|r| r.record.content.eq_ignore_ascii_case(&target))
            .cloned()
            .collect();
        for record in matching {
            if record.owned {
                self.steps.push(Step::DeleteRecord {
                    zone_id: record.zone_id.clone(),
                    record: record.record.clone(),
                });
            } else {
                self.warnings.push(Warning::KeepsForeignRecord {
                    hostname: hostname.to_owned(),
                });
            }
        }
    }

    /// Requires a login for `domain`: creates Teitunnel's application, or updates it if
    /// it lets in different people. Someone else's application is never changed.
    fn protect(&mut self, domain: &str, rule: &AccessRule) -> Result<(), PlanError> {
        let access = self.snapshot.access.as_ref();
        match access.and_then(|a| a.app(domain)) {
            Some(app) if !app.owned => {
                if app.rule.as_ref() != Some(rule) {
                    return Err(PlanError::AccessAppExists(domain.to_owned()));
                }
            }
            Some(app) => {
                let wanted = app_definition(domain, rule);
                if app.rule.as_ref() != Some(rule) || app.definition.name != wanted.name {
                    self.steps.push(Step::UpdateAccessApp {
                        id: app.id.clone(),
                        app: wanted,
                        previous: app.definition.clone(),
                    });
                }
            }
            None => {
                if access.and_then(|a| a.organization) == Some(false) {
                    return Err(PlanError::ZeroTrustNotSetUp);
                }
                let no_login = access.and_then(|a| a.login_methods) == Some(0);
                if no_login && !self.steps.contains(&Step::AddLoginMethod) {
                    self.steps.push(Step::AddLoginMethod);
                }
                self.steps.push(Step::CreateAccessApp {
                    app: app_definition(domain, rule),
                });
            }
        }
        Ok(())
    }

    /// Removes the login from `domain`, if Teitunnel put it there.
    fn unprotect(&mut self, domain: &str) {
        if let Some(app) = self
            .snapshot
            .access
            .as_ref()
            .and_then(|a| a.app(domain))
            .filter(|a| a.owned)
        {
            self.steps.push(Step::DeleteAccessApp {
                id: app.id.clone(),
                previous: app.definition.clone(),
            });
        }
    }

    /// Checks the route end to end, when a browser could open it (an SSH or TCP route
    /// only answers `cloudflared access`).
    fn verify_route(&mut self, route: &RouteSpec) {
        if route.origin.is_web() {
            self.steps.push(Step::Verify {
                hostname: route.hostname.to_string(),
            });
        }
    }

    fn remote_origin_warning(&mut self, route: &RouteSpec) {
        if !route.origin.is_local() {
            self.warnings.push(Warning::RemoteOrigin {
                origin: route.origin.to_string(),
            });
        }
    }

    fn finish(self) -> Plan {
        // A plan whose only step is Verify has nothing to apply.
        let steps = if self.steps.iter().any(Step::is_mutation) {
            self.steps
        } else {
            Vec::new()
        };
        let tunnel_name = self.snapshot.tunnel.as_ref().map_or_else(
            || {
                steps
                    .iter()
                    .find_map(|s| match s {
                        Step::CreateTunnel { name } => Some(name.clone()),
                        _ => None,
                    })
                    .unwrap_or_else(|| self.snapshot.machine_name.clone())
            },
            |t| t.name.clone(),
        );
        Plan {
            steps,
            warnings: self.warnings,
            requires_confirmation: self.requires_confirmation,
            fingerprint: self.snapshot.fingerprint(),
            tunnel_name,
        }
    }
}

fn route_domain(route: &RouteSpec) -> Result<String, PlanError> {
    Ok(access_domain(&route.hostname, route.path.as_ref())?)
}

/// Plans `intent` against `snapshot`.
///
/// # Errors
/// See [`PlanError`].
pub fn plan(intent: &Intent, snapshot: &Snapshot) -> Result<Plan, PlanError> {
    let mut b = Builder::new(snapshot);
    let rules: Vec<IngressRule> = snapshot.routes().into_iter().cloned().collect();
    match intent {
        Intent::AddRoute { route } => {
            b.zone_id(&route.hostname)?;
            if let Some(existing) = rules
                .iter()
                .find(|r| same_route(r, &route.hostname, route.path.as_ref()))
            {
                if existing.service == route.origin.as_str()
                    && existing.origin_request == route.options
                {
                    // Already configured; only DNS or the login may be missing
                    // (idempotent re-apply).
                    let tunnel = b.ensure_tunnel();
                    if let Some(rule) = &route.access {
                        b.protect(&route_domain(route)?, rule)?;
                    }
                    b.ensure_dns(&route.hostname, &tunnel, &route.id)?;
                    b.verify_route(route);
                    return Ok(b.finish());
                }
                return Err(PlanError::RouteExists(route.hostname.to_string()));
            }
            b.remote_origin_warning(route);
            let tunnel = b.ensure_tunnel();
            if let Some(rule) = &route.access {
                b.protect(&route_domain(route)?, rule)?;
            }
            let mut desired = rules.clone();
            desired.push(route.to_rule());
            b.put_config(&tunnel, desired);
            b.ensure_dns(&route.hostname, &tunnel, &route.id)?;
            b.verify_route(route);
        }
        Intent::UpdateRoute {
            hostname,
            path,
            route,
        } => {
            let tunnel_id = snapshot
                .tunnel
                .as_ref()
                .map(|t| t.id.clone())
                .ok_or(PlanError::NoTunnel)?;
            if !rules.iter().any(|r| same_route(r, hostname, path.as_ref())) {
                return Err(PlanError::NoSuchRoute(hostname.to_string()));
            }
            b.zone_id(&route.hostname)?;
            let renamed = *hostname != route.hostname;
            if renamed
                && rules
                    .iter()
                    .any(|r| same_route(r, &route.hostname, route.path.as_ref()))
            {
                return Err(PlanError::RouteExists(route.hostname.to_string()));
            }
            b.remote_origin_warning(route);
            let old_domain = access_domain(hostname, path.as_ref()).ok();
            let new_domain = match &route.access {
                Some(rule) => {
                    let domain = route_domain(route)?;
                    b.protect(&domain, rule)?;
                    Some(domain)
                }
                None => None,
            };
            let tunnel = TunnelRef::Existing(tunnel_id.clone());
            let desired: Vec<IngressRule> = rules
                .iter()
                .map(|r| {
                    if same_route(r, hostname, path.as_ref()) {
                        route.to_rule()
                    } else {
                        r.clone()
                    }
                })
                .collect();
            b.put_config(&tunnel, desired);
            b.ensure_dns(&route.hostname, &tunnel, &route.id)?;
            if renamed
                && !rules.iter().any(|r| {
                    r.hostname.as_deref() == Some(hostname.as_str())
                        && !same_route(r, hostname, path.as_ref())
                })
            {
                b.release_dns(hostname.as_str(), &tunnel_id);
            }
            if let Some(old) = old_domain.filter(|old| new_domain.as_ref() != Some(old)) {
                b.unprotect(&old);
            }
            b.verify_route(route);
        }
        Intent::RemoveRoute { hostname, path } => {
            let tunnel_id = snapshot
                .tunnel
                .as_ref()
                .map(|t| t.id.clone())
                .ok_or(PlanError::NoTunnel)?;
            if !rules.iter().any(|r| same_route(r, hostname, path.as_ref())) {
                return Err(PlanError::NoSuchRoute(hostname.to_string()));
            }
            let desired: Vec<IngressRule> = rules
                .iter()
                .filter(|r| !same_route(r, hostname, path.as_ref()))
                .cloned()
                .collect();
            let hostname_still_used = desired
                .iter()
                .any(|r| r.hostname.as_deref() == Some(hostname.as_str()));
            if desired.is_empty() {
                b.warnings.push(Warning::TunnelEmpty);
            }
            b.put_config(&TunnelRef::Existing(tunnel_id.clone()), desired);
            if !hostname_still_used {
                b.release_dns(hostname.as_str(), &tunnel_id);
            }
            if let Ok(domain) = access_domain(hostname, path.as_ref()) {
                b.unprotect(&domain);
            }
        }
        Intent::ImportRoutes { routes } => {
            let mut desired = rules.clone();
            let mut added = Vec::new();
            for route in routes {
                b.zone_id(&route.hostname)?;
                match desired
                    .iter()
                    .find(|r| same_route(r, &route.hostname, route.path.as_ref()))
                {
                    Some(existing)
                        if existing.service == route.origin.as_str()
                            && existing.origin_request == route.options => {}
                    Some(_) => return Err(PlanError::RouteExists(route.hostname.to_string())),
                    None => {
                        b.remote_origin_warning(route);
                        desired.push(route.to_rule());
                    }
                }
                added.push(route);
            }
            let tunnel = b.ensure_tunnel();
            b.put_config(&tunnel, desired);
            // One DNS record and one check per hostname, even with several paths.
            let mut seen = std::collections::HashSet::new();
            added.retain(|route| seen.insert(route.hostname.as_str()));
            for route in &added {
                b.ensure_dns(&route.hostname, &tunnel, &route.id)?;
            }
            for route in added {
                b.verify_route(route);
            }
        }
        Intent::DeleteRecord {
            zone_id,
            hostname,
            record_id,
        } => {
            let record = snapshot
                .records
                .iter()
                .find(|r| r.record.id == *record_id)
                .ok_or_else(|| PlanError::NoSuchRecord(hostname.to_string()))?;
            if !record.owned {
                b.requires_confirmation = true;
                b.warnings.push(Warning::DeletesForeignRecord {
                    hostname: record.record.name.clone(),
                    kind: record.record.kind.clone(),
                    content: record.record.content.clone(),
                });
            }
            b.steps.push(Step::DeleteRecord {
                zone_id: zone_id.clone(),
                record: record.record.clone(),
            });
        }
        Intent::RemoveLogin { domain } => {
            let routed = rules.iter().any(|r| {
                r.hostname
                    .as_deref()
                    .and_then(|h| Hostname::parse(h).ok())
                    .is_some_and(|h| {
                        let path = r.path.as_deref().and_then(|p| PathRule::parse(p).ok());
                        access_domain(&h, path.as_ref())
                            .is_ok_and(|d| d.eq_ignore_ascii_case(domain))
                    })
            });
            if routed {
                return Err(PlanError::RouteExists(domain.clone()));
            }
            b.unprotect(domain);
            if b.steps.is_empty() {
                return Err(PlanError::NoSuchLogin(domain.clone()));
            }
        }
        Intent::AddNetwork { network } => {
            let routes: Vec<&ObservedNetworkRoute> = snapshot
                .networks
                .iter()
                .flat_map(NetworkState::in_default_vnet)
                .collect();
            let ours = |r: &ObservedNetworkRoute| {
                snapshot
                    .tunnel
                    .as_ref()
                    .is_some_and(|t| t.id == r.tunnel_id)
            };
            if let Some(existing) = routes.iter().find(|r| r.range() == Some(*network)) {
                if ours(existing) {
                    // Already shared (idempotent re-apply).
                    return Ok(b.finish());
                }
                return Err(PlanError::NetworkRouted {
                    network: network.to_string(),
                    tunnel: existing
                        .tunnel_name
                        .clone()
                        .unwrap_or_else(|| existing.tunnel_id.clone()),
                });
            }
            if !network.is_private() {
                b.requires_confirmation = true;
                b.warnings.push(Warning::PublicNetwork {
                    network: network.to_string(),
                });
            }
            for other in routes.iter().filter(|r| !ours(r)) {
                if other.range().is_some_and(|o| o.overlaps(network)) {
                    b.warnings.push(Warning::OverlapsNetwork {
                        network: network.to_string(),
                        other: other.network.clone(),
                        tunnel: other
                            .tunnel_name
                            .clone()
                            .unwrap_or_else(|| other.tunnel_id.clone()),
                    });
                }
            }
            let tunnel = b.ensure_tunnel();
            b.steps.push(Step::CreateNetworkRoute {
                network: *network,
                tunnel,
            });
        }
        Intent::RemoveNetwork { network } => {
            let tunnel = snapshot.tunnel.as_ref().ok_or(PlanError::NoTunnel)?;
            let routes: Vec<ObservedNetworkRoute> = snapshot
                .networks
                .iter()
                .flat_map(NetworkState::in_default_vnet)
                .filter(|r| r.tunnel_id == tunnel.id && r.range() == Some(*network))
                .cloned()
                .collect();
            if routes.is_empty() {
                return Err(PlanError::NoSuchNetwork(network.to_string()));
            }
            for route in routes {
                b.steps.push(Step::DeleteNetworkRoute { route });
            }
        }
        Intent::RestoreConfig { ingress } => {
            let tunnel = snapshot.tunnel.as_ref().ok_or(PlanError::NoTunnel)?;
            let desired = ingress
                .iter()
                .filter(|r| r.hostname.is_some())
                .cloned()
                .collect();
            b.put_config(&TunnelRef::Existing(tunnel.id.clone()), desired);
        }
        Intent::RemoveTunnel => {
            let tunnel = snapshot.tunnel.as_ref().ok_or(PlanError::NoTunnel)?;
            let mut hostnames: Vec<&str> =
                rules.iter().filter_map(|r| r.hostname.as_deref()).collect();
            hostnames.sort_unstable();
            hostnames.dedup();
            if !rules.is_empty() {
                b.put_config(&TunnelRef::Existing(tunnel.id.clone()), Vec::new());
            }
            for hostname in hostnames {
                b.release_dns(hostname, &tunnel.id);
            }
            // Logins come down once no route reaches them, but before the tunnel is
            // deleted: that step can't be undone, so it stays last.
            let owned: Vec<String> = snapshot
                .access
                .iter()
                .flat_map(|a| a.apps.iter().filter(|app| app.owned))
                .map(|app| app.domain.clone())
                .collect();
            for domain in owned {
                b.unprotect(&domain);
            }
            // Private network routes would point at a tunnel that no longer exists.
            let networks: Vec<ObservedNetworkRoute> = snapshot
                .networks
                .iter()
                .flat_map(|n| n.of_tunnel(&tunnel.id))
                .cloned()
                .collect();
            for route in networks {
                b.steps.push(Step::DeleteNetworkRoute { route });
            }
            b.steps.push(Step::StopConnector {
                tunnel_id: tunnel.id.clone(),
            });
            b.steps.push(Step::DeleteTunnel {
                tunnel_id: tunnel.id.clone(),
            });
        }
    }
    Ok(b.finish())
}

/// A record pointing at `tunnel_id`'s CNAME target, for tests and the simulator.
#[cfg(test)]
pub(crate) fn tunnel_record(id: &str, name: &str, tunnel_id: &str) -> cf_api::DnsRecord {
    cf_api::DnsRecord {
        id: id.to_owned(),
        name: name.to_owned(),
        kind: "CNAME".to_owned(),
        content: tunnel_target(tunnel_id),
        proxied: true,
        comment: None,
        ttl: 1,
    }
}
