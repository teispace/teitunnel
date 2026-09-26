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

mod edge;
mod front;
mod reservations;
mod sites;

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
    /// The hostname is routed on another of this Mac's tunnels.
    RoutedElsewhere {
        /// Hostname.
        hostname: String,
        /// The other tunnel's name.
        tunnel: String,
    },
    /// A tunnel name must be 1–64 characters without control characters.
    InvalidTunnelName,
    /// Another tunnel in the account has the name.
    TunnelNameTaken(String),
    /// A load balancer Teitunnel didn't create already answers for the hostname.
    BalancerExists(String),
    /// Teitunnel doesn't load balance the hostname.
    NotBalanced(String),
    /// A Snapshot with this name already exists.
    SnapshotExists(String),
    /// The Snapshot's Worker is gone.
    NoSuchSnapshot(String),
    /// A route (Teitunnel's DNS record) already uses the hostname.
    HostnameRouted(String),
    /// Another Worker already answers on the hostname.
    HostnameServed {
        /// Hostname.
        hostname: String,
        /// That Worker.
        worker: String,
    },
    /// The account has no workers.dev subdomain yet.
    NoWorkersSubdomain,
    /// A login (Access) needs a hostname on one of the account's domains.
    SnapshotLoginNeedsDomain,
    /// A DNS record Teitunnel didn't create is on the hostname, so it can't be reserved.
    HostnameInUse(String),
    /// The hostname isn't reserved.
    NotReserved(String),
    /// A Free zone's rate limits can't match a hostname (only its whole domain).
    EdgeRateLimitNeedsPro(String),
    /// The account has no login method of the kind the login asks for.
    NoLoginMethod(crate::engine::access::SignIn),
    /// The zone's plan doesn't allow a period that long.
    EdgeRateLimitPeriod {
        /// The zone.
        zone: String,
        /// The longest it allows, in seconds.
        longest: u32,
    },
    /// The zone's plan has no room for another rule of this kind.
    EdgeQuotaFull {
        /// Which quota.
        quota: crate::engine::edge::QuotaKind,
        /// The zone.
        zone: String,
        /// Its limit.
        limit: u32,
    },
    /// The zone's only rate limit is Teitunnel's, with a different limit.
    EdgeRateLimitConflict {
        /// The zone.
        zone: String,
        /// The hostnames it covers.
        hostnames: String,
        /// Its requests.
        requests: u32,
        /// Its period, in seconds.
        period: u32,
    },
    /// A service token's label must be 1–40 characters without control characters.
    InvalidTokenLabel,
    /// The account has as many service tokens as Cloudflare allows.
    ServiceTokenLimit(u32),
    /// Teitunnel already made a token with this label for the hostname.
    ServiceTokenExists(String),
    /// No such service token (any more).
    NoSuchServiceToken(String),
    /// The token wasn't made by Teitunnel.
    ServiceTokenNotOwned(String),
    /// A Worker in front of a hostname needs it proxied through Cloudflare (a route).
    FrontNeedsRoute(String),
    /// Another Worker's route already has the pattern.
    WorkerRouteTaken {
        /// The pattern.
        pattern: String,
        /// That Worker.
        worker: String,
    },
    /// Nothing of Teitunnel's to remove there.
    NoFront(String),
    /// Invalid offline page or inbox settings.
    Front(crate::engine::front::FrontError),
    /// A verifying inbox needs its signing secret.
    InboxNeedsSecret,
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
            Self::RoutedElsewhere { hostname, tunnel } => {
                msg::error::plan::routed_elsewhere(hostname, tunnel)
            }
            Self::InvalidTunnelName => msg::error::plan::invalid_tunnel_name(),
            Self::TunnelNameTaken(name) => msg::error::plan::tunnel_name_taken(name),
            Self::BalancerExists(hostname) => msg::error::plan::balancer_exists(hostname),
            Self::NotBalanced(hostname) => msg::error::plan::not_balanced(hostname),
            Self::SnapshotExists(name) => msg::snapshot::error::exists(name),
            Self::NoSuchSnapshot(name) => msg::snapshot::error::gone(name),
            Self::HostnameRouted(hostname) => msg::snapshot::error::hostname_routed(hostname),
            Self::HostnameServed { hostname, worker } => {
                msg::snapshot::error::hostname_served(hostname, worker)
            }
            Self::NoWorkersSubdomain => msg::snapshot::error::no_workers_subdomain(),
            Self::SnapshotLoginNeedsDomain => msg::snapshot::error::login_needs_domain(),
            Self::HostnameInUse(hostname) => msg::reservations::error::hostname_in_use(hostname),
            Self::NotReserved(hostname) => msg::reservations::error::not_reserved(hostname),
            Self::EdgeRateLimitNeedsPro(zone) => msg::protection::error::rate_limit_needs_pro(zone),
            Self::NoLoginMethod(sign_in) => {
                msg::error::plan::no_login_method(sign_in.name().unwrap_or_default())
            }
            Self::EdgeRateLimitPeriod { zone, longest } => {
                msg::protection::error::rate_limit_period(zone, u64::from(*longest))
            }
            Self::EdgeQuotaFull { quota, zone, limit } => {
                use crate::engine::edge::QuotaKind;
                let limit = u64::from(*limit);
                match quota {
                    QuotaKind::Custom => msg::protection::error::quota_custom(limit, zone),
                    QuotaKind::RateLimit => msg::protection::error::quota_rate_limit(limit, zone),
                    QuotaKind::Transform => msg::protection::error::quota_transform(limit, zone),
                    QuotaKind::Cache => msg::protection::error::quota_cache(limit, zone),
                }
            }
            Self::EdgeRateLimitConflict {
                zone,
                hostnames,
                requests,
                period,
            } => msg::protection::error::rate_limit_conflict(
                zone,
                hostnames,
                u64::from(*requests),
                u64::from(*period),
            ),
            Self::InvalidTokenLabel => msg::protection::error::token_label(),
            Self::ServiceTokenLimit(limit) => {
                msg::protection::error::token_limit(u64::from(*limit))
            }
            Self::ServiceTokenExists(label) => msg::protection::error::token_exists(label),
            Self::NoSuchServiceToken(_) => msg::protection::error::no_such_token(),
            Self::ServiceTokenNotOwned(name) => msg::protection::error::token_not_owned(name),
            Self::FrontNeedsRoute(hostname) => msg::front::error::needs_route(hostname),
            Self::WorkerRouteTaken { pattern, worker } => {
                msg::front::error::route_taken(pattern, worker)
            }
            Self::NoFront(target) => msg::front::error::none(target),
            Self::Front(err) => err.text(),
            Self::InboxNeedsSecret => msg::front::error::needs_secret(),
        }
    }
}

english_display!(PlanError);

/// Removes what Teitunnel attached to `hostname` once no route uses it: its front
/// Workers, edge rules and the service tokens it made for it (after its login, which
/// may name them). Logins go per route.
fn release_hostname(b: &mut Builder<'_>, hostname: &Hostname) -> Result<(), PlanError> {
    release_workers_and_rules(b, hostname)?;
    let tokens: Vec<_> = b
        .snapshot
        .service_tokens
        .iter()
        .flatten()
        .filter(|t| t.owned && t.made_for.as_deref() == Some(hostname.as_str()))
        .cloned()
        .collect();
    b.steps.extend(
        tokens
            .into_iter()
            .map(|token| Step::DeleteServiceToken { token }),
    );
    Ok(())
}

/// Removes `hostname`'s front Workers and edge rules (those observed).
fn release_workers_and_rules(b: &mut Builder<'_>, hostname: &Hostname) -> Result<(), PlanError> {
    front::remove_all(b, hostname.as_str());
    let observed = hostname
        .zone_in(&b.snapshot.zones)
        .is_some_and(|zone| b.snapshot.edge_in(&zone.id).is_some());
    if observed {
        edge::protect(b, hostname, &crate::engine::edge::EdgeProtection::default())?;
    }
    Ok(())
}

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

    /// The pool endpoints for `hostname`: every tunnel that routes it except this
    /// Mac's, plus this Mac's (`own`) when it routes it after the change.
    fn endpoints(&self, own: Option<&TunnelRef>) -> Vec<super::types::PoolEndpoint> {
        use super::types::PoolEndpoint;
        let ours = self.snapshot.tunnel.as_ref().map(|t| t.id.as_str());
        let mut endpoints: Vec<PoolEndpoint> = self
            .snapshot
            .balance
            .iter()
            .flat_map(|b| &b.serving)
            .filter(|t| Some(t.id.as_str()) != ours)
            .map(|t| PoolEndpoint {
                tunnel: TunnelRef::Existing(t.id.clone()),
                name: t.name.clone(),
            })
            .collect();
        if let Some(tunnel) = own {
            let name = self
                .snapshot
                .tunnel
                .as_ref()
                .map_or_else(|| self.snapshot.machine_name.clone(), |t| t.name.clone());
            endpoints.push(PoolEndpoint {
                tunnel: tunnel.clone(),
                name,
            });
        }
        endpoints
    }

    /// Makes Teitunnel's pool for `hostname` send traffic to `endpoints`: a new pool (and
    /// monitor) when there's none, an update when they differ. Returns the pool.
    fn sync_pool(
        &mut self,
        hostname: &str,
        endpoints: Vec<super::types::PoolEndpoint>,
    ) -> Option<super::types::LbRef> {
        use super::types::LbRef;
        let state = self.snapshot.balance.as_ref()?;
        let monitor = match &state.monitor {
            Some(monitor) => LbRef::Existing(monitor.id.clone()),
            None => {
                self.steps.push(Step::CreateLbMonitor {
                    hostname: hostname.to_owned(),
                });
                LbRef::Created
            }
        };
        match &state.pool {
            Some(pool) => {
                let wanted: std::collections::BTreeSet<String> = endpoints
                    .iter()
                    .map(|e| match &e.tunnel {
                        TunnelRef::Existing(id) => super::types::tunnel_target(id),
                        TunnelRef::Created => String::new(),
                    })
                    .collect();
                let current: std::collections::BTreeSet<String> =
                    pool.origins.iter().map(|o| o.address.clone()).collect();
                let same_monitor =
                    matches!(&monitor, LbRef::Existing(id) if pool.monitor.as_deref() == Some(id));
                if wanted != current || !same_monitor {
                    self.steps.push(Step::UpdateLbPool {
                        hostname: hostname.to_owned(),
                        id: pool.id.clone(),
                        monitor,
                        endpoints,
                        previous: pool.clone(),
                    });
                }
                Some(LbRef::Existing(pool.id.clone()))
            }
            None => {
                self.steps.push(Step::CreateLbPool {
                    hostname: hostname.to_owned(),
                    monitor,
                    endpoints,
                });
                Some(LbRef::Created)
            }
        }
    }

    /// Whether Teitunnel load balances the hostname now.
    fn balanced(&self) -> bool {
        self.snapshot
            .balance
            .as_ref()
            .is_some_and(super::balance::BalanceState::active)
    }

    /// For a route of a balanced hostname: joins the pool instead of pointing the DNS
    /// record at this Mac (the load balancer answers for the hostname). Otherwise, DNS.
    fn serve(
        &mut self,
        hostname: &Hostname,
        tunnel: &TunnelRef,
        route_id: &str,
    ) -> Result<(), PlanError> {
        if self.balanced() {
            let endpoints = self.endpoints(Some(tunnel));
            self.sync_pool(hostname.as_str(), endpoints);
            return Ok(());
        }
        self.ensure_dns(hostname, tunnel, route_id)
    }

    /// Removes everything Teitunnel made to balance the hostname.
    fn unbalance(&mut self) {
        let Some(state) = self
            .snapshot
            .balance
            .clone()
            .filter(super::balance::BalanceState::active)
        else {
            return;
        };
        if let Some(balancer) = state.balancer {
            self.steps.push(Step::DeleteLoadBalancer {
                zone_id: state.zone_id.clone(),
                balancer,
            });
        }
        if let Some(pool) = state.pool {
            self.steps.push(Step::DeleteLbPool { pool });
        }
        if let Some(monitor) = state.monitor {
            self.steps.push(Step::DeleteLbMonitor { monitor });
        }
    }

    /// Fails if another of this Mac's tunnels already routes `hostname` (and `path`).
    fn not_elsewhere(&self, hostname: &Hostname, path: Option<&PathRule>) -> Result<(), PlanError> {
        let path = path.map(PathRule::as_str);
        match self
            .snapshot
            .elsewhere
            .iter()
            .find(|r| r.hostname == hostname.as_str() && r.path.as_deref() == path)
        {
            Some(other) => Err(PlanError::RoutedElsewhere {
                hostname: hostname.to_string(),
                tunnel: other.tunnel.clone(),
            }),
            None => Ok(()),
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
        self.take_over(hostname.as_str());
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
                reservations::restore(self, &record);
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
        // The application says who may log in; paths that skip it are applications of
        // their own.
        let people = rule.people_only();
        let methods = access.and_then(|a| a.login_methods.as_deref());
        let method = super::access::login_method(rule.sign_in, methods.unwrap_or_default())
            .map_err(PlanError::NoLoginMethod)?;
        match access.and_then(|a| a.app(domain)) {
            Some(app) if !app.owned => {
                if app.rule.as_ref() != Some(&people) {
                    return Err(PlanError::AccessAppExists(domain.to_owned()));
                }
            }
            Some(app) => {
                // Service tokens keep passing when who may log in changes.
                let wanted = super::access::keep_machines(
                    &app_definition(domain, rule, method),
                    &app.definition,
                );
                // The same people, but a GitHub or Google login method that was replaced.
                let stale = app.definition.allowed_idps != wanted.allowed_idps
                    || super::access::github_methods(&app.definition)
                        .iter()
                        .any(|id| Some(id.as_str()) != method.map(|m| m.id.as_str()));
                if app.rule.as_ref() != Some(&people) || app.definition.name != wanted.name || stale
                {
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
                // A login for any method needs one: a one-time code by email.
                let no_login = methods.is_some_and(<[_]>::is_empty);
                if method.is_none() && no_login && !self.steps.contains(&Step::AddLoginMethod) {
                    self.steps.push(Step::AddLoginMethod);
                }
                self.steps.push(Step::CreateAccessApp {
                    app: app_definition(domain, rule, method),
                });
            }
        }
        self.bypass(domain, &rule.bypass)
    }

    /// Makes the paths under `domain` that skip its login exactly `paths`: an
    /// application letting everyone through each, created or removed as needed. An
    /// application someone else made at one of those paths is never changed.
    fn bypass(&mut self, domain: &str, paths: &[String]) -> Result<(), PlanError> {
        let access = self.snapshot.access.as_ref();
        let current = access
            .map(|a| super::access::bypass_paths(a, domain))
            .unwrap_or_default();
        for path in paths.iter().filter(|p| !current.contains(p)) {
            let at = format!("{domain}{path}");
            if access.and_then(|a| a.app(&at)).is_some() {
                // A login (or anything else) is already there: not Teitunnel's to open.
                return Err(PlanError::AccessAppExists(at));
            }
            self.steps.push(Step::CreateAccessApp {
                app: super::access::bypass_definition(&at),
            });
        }
        for path in current.iter().filter(|p| !paths.contains(p)) {
            self.close_bypass(&format!("{domain}{path}"));
        }
        Ok(())
    }

    fn close_bypass(&mut self, at: &str) {
        if let Some(app) = self
            .snapshot
            .access
            .as_ref()
            .and_then(|a| a.app(at))
            .filter(|a| a.owned && super::access::is_bypass(&a.definition))
        {
            self.steps.push(Step::DeleteAccessApp {
                id: app.id.clone(),
                previous: app.definition.clone(),
            });
        }
    }

    /// Removes every path under `domain` that skips its login.
    fn close_bypasses(&mut self, domain: &str) {
        let paths = self
            .snapshot
            .access
            .as_ref()
            .map(|a| super::access::bypass_paths(a, domain))
            .unwrap_or_default();
        for path in paths {
            self.close_bypass(&format!("{domain}{path}"));
        }
    }

    /// Removes the people's login from `domain` but keeps its service tokens passing
    /// (an application only machines pass); without tokens, the application goes.
    fn drop_login(&mut self, domain: &str) {
        let Some(app) = self
            .snapshot
            .access
            .as_ref()
            .and_then(|a| a.app(domain))
            .filter(|a| a.owned)
        else {
            return;
        };
        if super::access::service_tokens_of(&app.definition).is_empty() {
            self.unprotect(domain);
            return;
        }
        // Without a login there's nothing to skip.
        self.close_bypasses(domain);
        self.steps.push(Step::UpdateAccessApp {
            id: app.id.clone(),
            app: super::access::keep_machines(
                &super::access::machine_only_definition(domain),
                &app.definition,
            ),
            previous: app.definition.clone(),
        });
    }

    /// Removes the login from `domain` (and the paths that skip it), if Teitunnel put
    /// it there.
    fn unprotect(&mut self, domain: &str) {
        self.close_bypasses(domain);
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
            b.not_elsewhere(&route.hostname, route.path.as_ref())?;
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
                    b.serve(&route.hostname, &tunnel, &route.id)?;
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
            b.serve(&route.hostname, &tunnel, &route.id)?;
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
            if *hostname != route.hostname || path != &route.path {
                b.not_elsewhere(&route.hostname, route.path.as_ref())?;
            }
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
                if renamed || path != &route.path {
                    b.unprotect(&old);
                } else {
                    b.drop_login(&old);
                }
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
            // Other computers still serving a balanced hostname keep what's in front
            // of it.
            let mut served_elsewhere = false;
            if !hostname_still_used {
                b.release_dns(hostname.as_str(), &tunnel_id);
                // Leaving a balanced hostname: out of its pool, or, as the last tunnel
                // serving it, the load balancing goes too.
                if b.balanced() {
                    let endpoints = b.endpoints(None);
                    if endpoints.is_empty() {
                        b.unbalance();
                    } else {
                        b.sync_pool(hostname.as_str(), endpoints);
                        served_elsewhere = true;
                    }
                }
            }
            if let Ok(domain) = access_domain(hostname, path.as_ref()) {
                b.unprotect(&domain);
            }
            if !hostname_still_used && !served_elsewhere {
                release_hostname(&mut b, hostname)?;
            }
        }
        Intent::BalanceRoute { hostname } => {
            let zone_id = b.zone_id(hostname)?;
            let tunnel_id = snapshot
                .tunnel
                .as_ref()
                .map(|t| t.id.clone())
                .ok_or(PlanError::NoTunnel)?;
            if !rules
                .iter()
                .any(|r| r.hostname.as_deref() == Some(hostname.as_str()))
            {
                return Err(PlanError::NoSuchRoute(hostname.to_string()));
            }
            let state = snapshot
                .balance
                .as_ref()
                .ok_or_else(|| PlanError::NoZone(hostname.to_string()))?;
            if !state.owned() {
                return Err(PlanError::BalancerExists(hostname.to_string()));
            }
            let endpoints = b.endpoints(Some(&TunnelRef::Existing(tunnel_id)));
            if endpoints.len() == 1 {
                b.warnings.push(Warning::SingleEndpoint {
                    hostname: hostname.to_string(),
                });
            }
            let pool = b.sync_pool(hostname.as_str(), endpoints);
            if state.balancer.is_none()
                && let Some(pool) = pool
            {
                b.steps.push(Step::CreateLoadBalancer {
                    zone_id,
                    hostname: hostname.to_string(),
                    pool,
                });
            }
        }
        Intent::UnbalanceRoute { hostname } => {
            if !snapshot
                .balance
                .as_ref()
                .is_some_and(super::balance::BalanceState::active)
            {
                return Err(PlanError::NotBalanced(hostname.to_string()));
            }
            b.unbalance();
        }
        Intent::CreateTunnel { name } => {
            let name = name.trim();
            if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
                return Err(PlanError::InvalidTunnelName);
            }
            if snapshot
                .tunnel_names
                .iter()
                .any(|taken| taken.eq_ignore_ascii_case(name))
            {
                return Err(PlanError::TunnelNameTaken(name.to_owned()));
            }
            b.steps.push(Step::CreateTunnel {
                name: name.to_owned(),
            });
        }
        Intent::ImportRoutes { routes } => {
            let mut desired = rules.clone();
            let mut added = Vec::new();
            for route in routes {
                b.zone_id(&route.hostname)?;
                b.not_elsewhere(&route.hostname, route.path.as_ref())?;
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
        Intent::CleanUpHostname { hostname } => {
            // Only a hostname nothing can serve: no route here, and no DNS record for
            // another computer's tunnel, a Snapshot or anything else.
            let served = rules
                .iter()
                .any(|r| r.hostname.as_deref() == Some(hostname.as_str()))
                || snapshot.records_named(hostname.as_str()).next().is_some();
            if served {
                return Err(PlanError::HostnameRouted(hostname.to_string()));
            }
            b.unprotect(hostname.as_str());
            release_hostname(&mut b, hostname)?;
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
            for hostname in &hostnames {
                b.release_dns(hostname, &tunnel.id);
            }
            // A hostname whose record pointed here stops resolving: its offline page,
            // inboxes and edge rules go too (a balanced hostname, which other
            // computers may still serve, has no such record and keeps them).
            let target = tunnel_target(&tunnel.id);
            for hostname in &hostnames {
                let Ok(host) = Hostname::parse(hostname) else {
                    continue;
                };
                let resolves_here = snapshot
                    .records_named(hostname)
                    .any(|r| r.owned && r.record.content.eq_ignore_ascii_case(&target));
                if resolves_here {
                    release_workers_and_rules(&mut b, &host)?;
                }
            }
            // The service tokens Teitunnel made for its hostnames would open nothing.
            let doomed: Vec<_> = snapshot
                .service_tokens
                .iter()
                .flatten()
                .filter(|t| {
                    t.owned
                        && t.made_for
                            .as_deref()
                            .is_some_and(|h| hostnames.contains(&h))
                })
                .cloned()
                .collect();
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
            b.steps.extend(
                doomed
                    .into_iter()
                    .map(|token| Step::DeleteServiceToken { token }),
            );
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
        Intent::PublishSnapshot {
            site,
            settings,
            content,
        } => sites::publish(&mut b, site, settings, content)?,
        Intent::UpdateSnapshot {
            site,
            settings,
            content,
            previous,
        } => sites::update(&mut b, site, settings, content, previous)?,
        Intent::RollbackSnapshot {
            site,
            version_id,
            number,
        } => sites::rollback(&mut b, site, version_id, *number)?,
        Intent::DeleteSnapshot { site } => sites::delete(&mut b, site),
        Intent::Reserve { hostname, until } => reservations::reserve(&mut b, hostname, *until)?,
        Intent::Release { hostname } => reservations::release(&mut b, hostname)?,
        Intent::ProtectHostname {
            hostname,
            protection,
        } => edge::protect(&mut b, hostname, protection)?,
        Intent::CreateServiceToken { hostname, label } => {
            edge::create_token(&mut b, hostname, label)?;
        }
        Intent::RevokeServiceToken { hostname, token_id } => {
            edge::revoke_token(&mut b, hostname, token_id)?;
        }
        Intent::RotateServiceToken { token_id, .. } => edge::rotate_token(&mut b, token_id)?,
        Intent::SetOfflinePage { hostname, page } => {
            front::offline(&mut b, hostname, page.as_ref())?;
        }
        Intent::SetInbox {
            hostname,
            path,
            inbox,
            secret,
        } => front::inbox(&mut b, hostname, path, inbox.as_ref(), secret.as_ref())?,
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
