//! The pure planner: `plan(intent, snapshot) -> Plan`. No I/O, deterministic.
//!
//! Order of operations (ARCHITECTURE §4.3):
//! - add: create tunnel → config (the tunnel learns the hostname first, so there's no
//!   404 window) → DNS → verify
//! - remove: config → DNS (only records Teitunnel owns) → tunnel
//! - rename: config (swap the rule) → new DNS → delete the old owned record

use cf_api::IngressRule;

use super::{
    ingress::sort_ingress,
    types::{
        Intent, ObservedRecord, Plan, RouteSpec, Snapshot, Step, TunnelRef, Warning, tunnel_target,
    },
};
use crate::domain::{Hostname, PathRule};

/// Why no plan could be made. Messages are shown to the user.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlanError {
    /// The hostname isn't in any zone of the account.
    #[error("{0} isn't in any of this account's domains. Add the domain to Cloudflare first.")]
    NoZone(String),
    /// A route with this hostname and path already exists.
    #[error("{0} is already routed. Edit that route instead.")]
    RouteExists(String),
    /// No route with this hostname and path.
    #[error("There's no route for {0}.")]
    NoSuchRoute(String),
    /// Nothing to remove.
    #[error("This Mac doesn't have a tunnel yet.")]
    NoTunnel,
}

fn same_route(rule: &IngressRule, hostname: &Hostname, path: Option<&PathRule>) -> bool {
    rule.hostname.as_deref() == Some(hostname.as_str())
        && rule.path.as_deref() == path.map(PathRule::as_str)
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
                    name: self.snapshot.machine_name.clone(),
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

    fn verify(&mut self, hostname: &Hostname) {
        self.steps.push(Step::Verify {
            hostname: hostname.to_string(),
        });
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
        Plan {
            steps,
            warnings: self.warnings,
            requires_confirmation: self.requires_confirmation,
            fingerprint: self.snapshot.fingerprint(),
        }
    }
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
                    // Already configured; only DNS may be missing (idempotent re-apply).
                    let tunnel = b.ensure_tunnel();
                    b.ensure_dns(&route.hostname, &tunnel, &route.id)?;
                    b.verify(&route.hostname);
                    return Ok(b.finish());
                }
                return Err(PlanError::RouteExists(route.hostname.to_string()));
            }
            b.remote_origin_warning(route);
            let tunnel = b.ensure_tunnel();
            let mut desired = rules.clone();
            desired.push(route.to_rule());
            b.put_config(&tunnel, desired);
            b.ensure_dns(&route.hostname, &tunnel, &route.id)?;
            b.verify(&route.hostname);
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
            b.verify(&route.hostname);
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
