//! A model of applying a plan to a snapshot: what Cloudflare would look like afterwards.
//! Used by the planner's idempotency property test.

use super::types::{
    ObservedRecord, ObservedTunnel, Plan, Snapshot, Step, TunnelRef, ownership_comment,
    tunnel_target,
};

pub(crate) const CREATED_TUNNEL_ID: &str = "new-tunnel";

fn resolve(tunnel: &TunnelRef) -> String {
    match tunnel {
        TunnelRef::Existing(id) => id.clone(),
        TunnelRef::Created => CREATED_TUNNEL_ID.to_owned(),
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
            Step::StopConnector { .. } | Step::Verify { .. } => {}
        }
    }
    next
}
