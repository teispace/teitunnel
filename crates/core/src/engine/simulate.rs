//! A model of applying a plan to a snapshot: what Cloudflare would look like afterwards.
//! Used by the planner's idempotency property test.

use super::{
    access::{AccessRule, AccessState, ObservedAccessApp},
    networks::{NETWORK_COMMENT, NetworkState, ObservedNetworkRoute},
    ownership::route_comment,
    types::{ObservedRecord, ObservedTunnel, Plan, Snapshot, Step, TunnelRef, tunnel_target},
};

pub(crate) const CREATED_TUNNEL_ID: &str = "new-tunnel";

fn resolve(tunnel: &TunnelRef) -> String {
    match tunnel {
        TunnelRef::Existing(id) => id.clone(),
        TunnelRef::Created => CREATED_TUNNEL_ID.to_owned(),
    }
}

fn empty_access() -> AccessState {
    AccessState {
        organization: Some(true),
        login_methods: Some(0),
        apps: Vec::new(),
    }
}

/// Applies `plan` to `snapshot` as a perfect executor would.
pub(crate) fn apply(snapshot: &Snapshot, plan: &Plan) -> Snapshot {
    let mut next = snapshot.clone();
    let mut record_ids = 0;
    let (owner, now) = (snapshot.owner.clone(), snapshot.now);
    for step in &plan.steps {
        match step {
            Step::CreateTunnel { name } => {
                next.tunnel = Some(ObservedTunnel {
                    id: CREATED_TUNNEL_ID.into(),
                    name: name.clone(),
                    config_version: 0,
                    ingress: Vec::new(),
                });
            }
            Step::PutConfig { ingress, .. } => {
                if let Some(tunnel) = next.tunnel.as_mut() {
                    tunnel.ingress = ingress.clone();
                    tunnel.config_version += 1;
                }
            }
            Step::CreateRecord {
                zone_id,
                hostname,
                tunnel,
                route_id,
            } => {
                record_ids += 1;
                next.records.push(ObservedRecord {
                    zone_id: zone_id.clone(),
                    owned: true,
                    record: cf_api::DnsRecord {
                        id: format!("sim-{record_ids}"),
                        name: hostname.clone(),
                        kind: "CNAME".into(),
                        content: tunnel_target(&resolve(tunnel)),
                        proxied: true,
                        comment: Some(route_comment(route_id, &owner, None, now)),
                        ttl: 1,
                    },
                });
                next.held
                    .retain(|h| !h.hostname.eq_ignore_ascii_case(hostname));
            }
            Step::UpdateRecord {
                record_id,
                tunnel,
                route_id,
                ..
            } => {
                if let Some(r) = next.records.iter_mut().find(|r| r.record.id == *record_id) {
                    r.record.kind = "CNAME".into();
                    r.record.content = tunnel_target(&resolve(tunnel));
                    r.record.proxied = true;
                    r.record.comment = Some(route_comment(
                        route_id,
                        &owner,
                        r.record.comment.as_deref(),
                        now,
                    ));
                    r.owned = true;
                    let name = r.record.name.clone();
                    next.held
                        .retain(|h| !h.hostname.eq_ignore_ascii_case(&name));
                }
            }
            Step::DeleteRecord { record, .. } => {
                next.records.retain(|r| r.record.id != record.id);
                next.held
                    .retain(|h| !h.hostname.eq_ignore_ascii_case(&record.name));
            }
            Step::CreateReservation {
                zone_id,
                hostname,
                until,
            } => {
                record_ids += 1;
                next.held
                    .retain(|h| !h.hostname.eq_ignore_ascii_case(hostname));
                next.records.push(ObservedRecord {
                    zone_id: zone_id.clone(),
                    owned: true,
                    record: cf_api::DnsRecord {
                        id: format!("sim-{record_ids}"),
                        name: hostname.clone(),
                        kind: super::ownership::LEASE_KIND.into(),
                        content: super::ownership::LEASE_ADDRESS.into(),
                        proxied: true,
                        comment: Some(
                            super::ownership::Ownership::lease(&next.owner, *until).render(),
                        ),
                        ttl: 1,
                    },
                });
            }
            Step::SetLease {
                record,
                lease,
                until,
                ..
            } => {
                if let Some(r) = next.records.iter_mut().find(|r| r.record.id == record.id) {
                    let mut ownership = r
                        .record
                        .comment
                        .as_deref()
                        .and_then(super::ownership::Ownership::parse)
                        .unwrap_or_else(|| super::ownership::Ownership::lease(&next.owner, None));
                    ownership.lease = *lease || ownership.marker == super::ownership::Marker::Lease;
                    ownership.until = if *lease { *until } else { None };
                    r.record.comment = Some(ownership.render());
                }
            }
            Step::DeleteTunnel { .. } => next.tunnel = None,
            Step::AddLoginMethod => {
                let access = next.access.get_or_insert_with(empty_access);
                access.login_methods = Some(access.login_methods.unwrap_or(0) + 1);
            }
            Step::CreateAccessApp { app } => {
                record_ids += 1;
                next.access
                    .get_or_insert_with(empty_access)
                    .apps
                    .push(ObservedAccessApp {
                        id: format!("sim-app-{record_ids}"),
                        domain: app.domain.clone(),
                        owned: true,
                        rule: AccessRule::from_new(app),
                        definition: app.clone(),
                    });
            }
            Step::UpdateAccessApp { id, app, .. } => {
                if let Some(existing) = next
                    .access
                    .as_mut()
                    .and_then(|a| a.apps.iter_mut().find(|a| a.id == *id))
                {
                    existing.domain.clone_from(&app.domain);
                    existing.rule = AccessRule::from_new(app);
                    existing.definition = app.clone();
                }
            }
            Step::DeleteAccessApp { id, .. } => {
                if let Some(access) = next.access.as_mut() {
                    access.apps.retain(|a| a.id != *id);
                }
            }
            Step::CreateNetworkRoute { network, tunnel } => {
                record_ids += 1;
                let state = next.networks.get_or_insert_with(|| NetworkState {
                    default_vnet: None,
                    routes: Vec::new(),
                });
                let virtual_network_id = state.default_vnet.clone();
                state.routes.push(ObservedNetworkRoute {
                    id: format!("sim-net-{record_ids}"),
                    network: network.to_string(),
                    tunnel_id: resolve(tunnel),
                    tunnel_name: None,
                    virtual_network_id,
                    comment: NETWORK_COMMENT.into(),
                });
            }
            Step::DeleteNetworkRoute { route } => {
                if let Some(state) = next.networks.as_mut() {
                    state.routes.retain(|r| r.id != route.id);
                }
            }
            Step::CreateLbMonitor { hostname } => {
                if let Some(state) = next.balance.as_mut() {
                    let mut monitor = super::balance::monitor_for(hostname);
                    monitor.id = "sim-monitor".into();
                    state.monitor = Some(monitor);
                }
            }
            Step::CreateLbPool {
                hostname,
                endpoints,
                ..
            }
            | Step::UpdateLbPool {
                hostname,
                endpoints,
                ..
            } => {
                let tunnel = next
                    .tunnel
                    .as_ref()
                    .map(|t| t.id.clone())
                    .unwrap_or_default();
                if let Some(state) = next.balance.as_mut() {
                    let origins = endpoints
                        .iter()
                        .map(|e| {
                            let id = match &e.tunnel {
                                TunnelRef::Existing(id) => id.clone(),
                                TunnelRef::Created => tunnel.clone(),
                            };
                            super::balance::origin_for(hostname, &id, &e.name)
                        })
                        .collect();
                    let monitor = state.monitor.as_ref().map(|m| m.id.clone());
                    let id = state
                        .pool
                        .as_ref()
                        .map_or_else(|| "sim-pool".to_owned(), |p| p.id.clone());
                    state.pool = Some(cf_api::Pool {
                        id,
                        name: super::balance::pool_name(hostname),
                        description: super::balance::marker(hostname),
                        enabled: true,
                        monitor,
                        origins,
                    });
                }
            }
            Step::CreateLoadBalancer { hostname, .. } => {
                if let Some(state) = next.balance.as_mut() {
                    let pool = state
                        .pool
                        .as_ref()
                        .map(|p| p.id.clone())
                        .unwrap_or_default();
                    state.balancer = Some(cf_api::LoadBalancer {
                        id: "sim-lb".into(),
                        name: hostname.clone(),
                        description: super::balance::marker(hostname),
                        default_pools: vec![pool.clone()],
                        fallback_pool: pool,
                        proxied: true,
                    });
                }
            }
            Step::DeleteLoadBalancer { .. } => {
                if let Some(state) = next.balance.as_mut() {
                    state.balancer = None;
                }
            }
            Step::DeleteLbPool { .. } => {
                if let Some(state) = next.balance.as_mut() {
                    state.pool = None;
                }
            }
            Step::DeleteLbMonitor { .. } => {
                if let Some(state) = next.balance.as_mut() {
                    state.monitor = None;
                }
            }
            // A new secret changes nothing the planner reads, like these.
            Step::StopConnector { .. }
            | Step::Verify { .. }
            | Step::UploadSnapshotFiles { .. }
            | Step::RotateServiceToken { .. } => {}
            Step::CreateSnapshotWorker { .. } | Step::PublishSnapshotVersion { .. } => {
                if let Some(site) = next.site.as_mut() {
                    site.exists = true;
                    site.active_version = Some("new-version".into());
                }
            }
            Step::RollBackSnapshot { version_id, .. } => {
                if let Some(site) = next.site.as_mut() {
                    site.active_version = Some(version_id.clone());
                }
            }
            Step::EnableWorkersDev { .. } | Step::DisableWorkersDev { .. } => {
                if let Some(site) = next.site.as_mut() {
                    site.workers_dev = matches!(step, Step::EnableWorkersDev { .. });
                }
            }
            Step::AttachSnapshotDomain {
                zone_id,
                hostname,
                script,
            } => {
                if let Some(site) = next.site.as_mut() {
                    site.domains.push(cf_api::WorkerDomain {
                        id: format!("domain-{hostname}"),
                        hostname: hostname.clone(),
                        service: script.clone(),
                        zone_id: zone_id.clone(),
                        zone_name: String::new(),
                    });
                }
            }
            Step::DetachSnapshotDomain { domain } => {
                if let Some(site) = next.site.as_mut() {
                    site.domains.retain(|d| d.id != domain.id);
                }
            }
            Step::DeleteSnapshotWorker { .. } => {
                if let Some(site) = next.site.as_mut() {
                    site.exists = false;
                    site.active_version = None;
                    site.domains.clear();
                    site.workers_dev = false;
                }
            }
            Step::CreateEdgeRule { phase, rule, .. } => {
                record_ids += 1;
                if let Some(ruleset) = next
                    .edge
                    .as_mut()
                    .and_then(|e| e.rulesets.iter_mut().find(|r| r.phase == *phase))
                {
                    ruleset
                        .id
                        .get_or_insert_with(|| format!("sim-ruleset-{phase}"));
                    ruleset.rules.push(super::edge::ObservedRule {
                        id: format!("sim-rule-{record_ids}"),
                        rule: rule.clone(),
                        owned: true,
                    });
                }
            }
            Step::UpdateEdgeRule { rule_id, rule, .. } => {
                if let Some(found) = next.edge.as_mut().and_then(|e| {
                    e.rulesets
                        .iter_mut()
                        .flat_map(|r| &mut r.rules)
                        .find(|r| r.id == *rule_id)
                }) {
                    found.rule = rule.clone();
                }
            }
            Step::DeleteEdgeRule { rule_id, .. } => {
                if let Some(edge) = next.edge.as_mut() {
                    for ruleset in &mut edge.rulesets {
                        ruleset.rules.retain(|r| r.id != *rule_id);
                    }
                }
            }
            Step::CreateServiceToken { name, .. } => {
                next.service_tokens.get_or_insert_with(Vec::new).push(
                    super::edge::ObservedServiceToken {
                        id: "sim-token".into(),
                        name: name.clone(),
                        client_id: "sim-token.access".into(),
                        expires_at: None,
                        owned: true,
                    },
                );
            }
            Step::AllowServiceToken { domain, app, .. } => {
                let definition = super::access::with_service_token(
                    &app.as_ref().map_or_else(
                        || super::access::machine_only_definition(domain),
                        |(_, d)| d.clone(),
                    ),
                    "sim-token",
                );
                let access = next.access.get_or_insert_with(empty_access);
                match app {
                    Some((id, _)) => {
                        if let Some(existing) = access.apps.iter_mut().find(|a| a.id == *id) {
                            existing.rule = AccessRule::from_new(&definition);
                            existing.definition = definition;
                        }
                    }
                    None => access.apps.push(ObservedAccessApp {
                        id: "sim-machines".into(),
                        domain: domain.clone(),
                        owned: true,
                        rule: None,
                        definition,
                    }),
                }
            }
            Step::DeleteServiceToken { token } => {
                if let Some(tokens) = next.service_tokens.as_mut() {
                    tokens.retain(|t| t.id != token.id);
                }
            }
            Step::CreateDatabase { .. } => {
                next.database = Some(super::front::DatabaseState {
                    id: Some("new-database".into()),
                });
            }
            Step::PutFrontWorker { script, config, .. } => {
                if let Some(front) = next.front.as_mut() {
                    match front.fronts.iter_mut().find(|f| {
                        f.config.kind() == config.kind() && f.config.path() == config.path()
                    }) {
                        Some(existing) => {
                            existing.config = config.clone();
                            existing.exists = true;
                        }
                        None => front.fronts.push(super::front::ObservedFront {
                            config: config.clone(),
                            script: script.clone(),
                            exists: true,
                            route: None,
                        }),
                    }
                }
            }
            Step::CreateWorkerRoute {
                pattern,
                script,
                kind,
                path,
                ..
            } => {
                if let Some(front) = next.front.as_mut().and_then(|f| {
                    f.fronts
                        .iter_mut()
                        .find(|x| x.config.kind() == *kind && x.config.path() == path)
                }) {
                    front.route = Some(cf_api::WorkerRoute {
                        id: format!("route-{script}"),
                        pattern: pattern.clone(),
                        script: Some(script.clone()),
                        request_limit_fail_open: Some(true),
                    });
                }
            }
            Step::DeleteWorkerRoute { route, .. } => {
                if let Some(state) = next.front.as_mut() {
                    for front in &mut state.fronts {
                        if front.route.as_ref().is_some_and(|r| r.id == route.id) {
                            front.route = None;
                        }
                    }
                }
            }
            Step::DeleteFrontWorker { script, .. } => {
                if let Some(state) = next.front.as_mut() {
                    state.fronts.retain(|f| f.script != *script);
                }
            }
        }
    }
    next
}
