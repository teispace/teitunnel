//! A model of applying a plan to a snapshot: what Cloudflare would look like afterwards.
//! Used by the planner's idempotency property test.

use super::{
    access::{AccessRule, AccessState, ObservedAccessApp},
    networks::{NETWORK_COMMENT, NetworkState, ObservedNetworkRoute},
    types::{
        ObservedRecord, ObservedTunnel, Plan, Snapshot, Step, TunnelRef, ownership_comment,
        tunnel_target,
    },
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
                        comment: Some(ownership_comment(route_id)),
                        ttl: 1,
                    },
                });
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
                    r.record.comment = Some(ownership_comment(route_id));
                    r.owned = true;
                }
            }
            Step::DeleteRecord { record, .. } => next.records.retain(|r| r.record.id != record.id),
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
            Step::StopConnector { .. } | Step::Verify { .. } => {}
        }
    }
    next
}
