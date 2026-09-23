//! What the activity log records about an applied plan: what kind of change it was,
//! each step with how it ended (and its "Copy as command"), and a before/after list of
//! the routes and DNS records it touched. Pure, so it's snapshot-testable.

use std::collections::{BTreeMap, BTreeSet};

use cf_api::IngressRule;
use serde::{Deserialize, Serialize};

use crate::text::{Text, msg, msg::activity::delta};

use super::{
    executor::StepState,
    types::{Intent, Plan, Step},
    views::StepView,
};

/// What kind of change an entry records (for filtering).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    /// A route was added.
    AddRoute,
    /// A route was changed or renamed.
    UpdateRoute,
    /// A route was removed.
    RemoveRoute,
    /// Every route was removed and the tunnel deleted.
    RemoveTunnel,
    /// Routes were imported from a cloudflared setup.
    ImportRoutes,
    /// A DNS record was deleted (Doctor cleanup).
    DeleteRecord,
    /// Routes were restored after an outside edit.
    RestoreConfig,
    /// A login whose route was gone was removed (Doctor cleanup).
    RemoveLogin,
    /// A private network was shared.
    AddNetwork,
    /// A private network stopped being shared.
    RemoveNetwork,
    /// Another tunnel was created for this Mac.
    CreateTunnel,
}

impl From<&Intent> for ActivityKind {
    fn from(intent: &Intent) -> Self {
        match intent {
            Intent::AddRoute { .. } => Self::AddRoute,
            Intent::UpdateRoute { .. } => Self::UpdateRoute,
            Intent::RemoveRoute { .. } => Self::RemoveRoute,
            Intent::RemoveTunnel => Self::RemoveTunnel,
            Intent::ImportRoutes { .. } => Self::ImportRoutes,
            Intent::DeleteRecord { .. } => Self::DeleteRecord,
            Intent::RestoreConfig { .. } => Self::RestoreConfig,
            Intent::RemoveLogin { .. } => Self::RemoveLogin,
            Intent::AddNetwork { .. } => Self::AddNetwork,
            Intent::RemoveNetwork { .. } => Self::RemoveNetwork,
            Intent::CreateTunnel { .. } => Self::CreateTunnel,
        }
    }
}

/// Where a change happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum DeltaArea {
    /// The tunnel's routes (ingress).
    Route,
    /// A DNS record.
    Dns,
    /// A private network route.
    Network,
    /// A route's login (Cloudflare Access).
    Access,
}

/// One thing that changed: absent `before` means added, absent `after` removed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Delta {
    /// Route or DNS record.
    pub area: DeltaArea,
    /// Public hostname.
    pub hostname: String,
    /// Path rule, for routes that have one.
    pub path: Option<String>,
    /// What it was.
    pub before: Option<Text>,
    /// What it became.
    pub after: Option<Text>,
}

/// A step of the applied plan and how it ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RecordedStep {
    /// The step as previewed (with its command).
    pub step: StepView,
    /// Its final state.
    pub state: StepState,
}

/// The structured part of an activity entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ActivityRecord {
    /// What kind of change.
    pub kind: ActivityKind,
    /// Every hostname involved, sorted.
    pub hostnames: Vec<String>,
    /// The tunnel's name.
    pub tunnel: String,
    /// The steps that change something, in order.
    pub steps: Vec<RecordedStep>,
    /// What changed, routes first.
    pub changes: Vec<Delta>,
    /// What was asked (absent in entries from before messages had keys).
    #[serde(default)]
    pub summary: Option<Text>,
    /// Why it failed, when it did.
    #[serde(default)]
    pub error: Option<Text>,
    /// What couldn't be undone after a failure.
    #[serde(default)]
    pub leftovers: Vec<Text>,
    /// The routes were applied but this Mac's connector couldn't be started.
    #[serde(default)]
    pub connector_error: Option<Text>,
}

impl ActivityRecord {
    /// The record of applying `plan` for `intent`; `states` holds each step's last
    /// reported state by index (steps never reported count as skipped).
    pub fn new(
        intent: &Intent,
        plan: &Plan,
        account_id: &str,
        states: &[Option<StepState>],
    ) -> Self {
        let steps = plan
            .steps
            .iter()
            .enumerate()
            .filter(|(_, step)| !matches!(step, Step::Verify { .. }))
            .map(|(index, step)| RecordedStep {
                step: step.view(account_id, &plan.tunnel_name),
                state: states
                    .get(index)
                    .cloned()
                    .flatten()
                    .unwrap_or(StepState::Skipped),
            })
            .collect();
        let changes = deltas(plan);
        let mut hostnames: BTreeSet<String> = changes.iter().map(|d| d.hostname.clone()).collect();
        if let Some(named) = intent.hostnames() {
            hostnames.extend(named.into_iter().map(ToString::to_string));
        }
        Self {
            kind: intent.into(),
            hostnames: hostnames.into_iter().collect(),
            tunnel: plan.tunnel_name.clone(),
            steps,
            changes,
            summary: Some(intent.summary()),
            error: None,
            leftovers: Vec::new(),
            connector_error: None,
        }
    }
}

/// A rule as one line: its service, plus the names of any origin settings.
fn describe_rule(rule: &IngressRule) -> Text {
    if rule.origin_request.is_empty() {
        return msg::raw(&rule.service);
    }
    let settings: Vec<&str> = rule.origin_request.keys().map(String::as_str).collect();
    msg::raw(format!("{} · {}", rule.service, settings.join(", ")))
}

/// A DNS record's value, e.g. `A 192.0.2.1`.
fn record_value(kind: &str, content: &str) -> Text {
    msg::raw(format!("{kind} {content}"))
}

type RouteKey = (String, Option<String>);

fn routes(ingress: &[IngressRule]) -> BTreeMap<RouteKey, &IngressRule> {
    ingress
        .iter()
        .filter_map(|rule| Some(((rule.hostname.clone()?, rule.path.clone()), rule)))
        .collect()
}

/// Who a login lets in, as a line for the before/after list.
fn login_summary(app: &cf_api::NewAccessApp) -> Text {
    super::access::AccessRule::from_new(app).map_or_else(delta::login_custom, |rule| {
        delta::login_required(rule.people())
    })
}

/// `deltas` with their text in English, shaped as they were stored before messages had
/// keys, so snapshots read naturally and show wording changes.
#[cfg(test)]
pub(crate) fn english(deltas: &[Delta]) -> Vec<EnglishDelta> {
    deltas
        .iter()
        .map(|d| EnglishDelta {
            area: d.area,
            hostname: d.hostname.clone(),
            path: d.path.clone(),
            before: d.before.as_ref().map(Text::english),
            after: d.after.as_ref().map(Text::english),
        })
        .collect()
}

/// A [`Delta`] with its text in English (tests).
#[cfg(test)]
#[derive(Serialize)]
pub(crate) struct EnglishDelta {
    area: DeltaArea,
    hostname: String,
    path: Option<String>,
    before: Option<String>,
    after: Option<String>,
}

/// What `plan` changes, from its steps' before and after values.
pub fn deltas(plan: &Plan) -> Vec<Delta> {
    let tunnel_target = delta::tunnel_target(&plan.tunnel_name);
    let mut out = Vec::new();
    for step in &plan.steps {
        match step {
            Step::PutConfig {
                ingress, previous, ..
            } => {
                let (before, after) = (routes(previous), routes(ingress));
                let keys: BTreeSet<&RouteKey> = before.keys().chain(after.keys()).collect();
                for key in keys {
                    let (old, new) = (before.get(key), after.get(key));
                    if old == new {
                        continue;
                    }
                    out.push(Delta {
                        area: DeltaArea::Route,
                        hostname: key.0.clone(),
                        path: key.1.clone(),
                        before: old.map(|r| describe_rule(r)),
                        after: new.map(|r| describe_rule(r)),
                    });
                }
            }
            Step::CreateRecord { hostname, .. } => out.push(Delta {
                area: DeltaArea::Dns,
                hostname: hostname.clone(),
                path: None,
                before: None,
                after: Some(tunnel_target.clone()),
            }),
            Step::UpdateRecord {
                hostname, previous, ..
            } => out.push(Delta {
                area: DeltaArea::Dns,
                hostname: hostname.clone(),
                path: None,
                before: Some(record_value(&previous.kind, &previous.content)),
                after: Some(tunnel_target.clone()),
            }),
            Step::DeleteRecord { record, .. } => out.push(Delta {
                area: DeltaArea::Dns,
                hostname: record.name.clone(),
                path: None,
                before: Some(record_value(&record.kind, &record.content)),
                after: None,
            }),
            Step::CreateAccessApp { app } => out.push(Delta {
                area: DeltaArea::Access,
                hostname: app.domain.clone(),
                path: None,
                before: None,
                after: Some(login_summary(app)),
            }),
            Step::UpdateAccessApp { app, previous, .. } => out.push(Delta {
                area: DeltaArea::Access,
                hostname: app.domain.clone(),
                path: None,
                before: Some(login_summary(previous)),
                after: Some(login_summary(app)),
            }),
            Step::DeleteAccessApp { previous, .. } => out.push(Delta {
                area: DeltaArea::Access,
                hostname: previous.domain.clone(),
                path: None,
                before: Some(login_summary(previous)),
                after: None,
            }),
            Step::CreateNetworkRoute { network, .. } => out.push(Delta {
                area: DeltaArea::Network,
                hostname: network.to_string(),
                path: None,
                before: None,
                after: Some(delta::routed_to(&plan.tunnel_name)),
            }),
            Step::DeleteNetworkRoute { route } => out.push(Delta {
                area: DeltaArea::Network,
                hostname: route.network.clone(),
                path: None,
                before: Some(delta::routed_to(
                    route.tunnel_name.as_deref().unwrap_or(&plan.tunnel_name),
                )),
                after: None,
            }),
            Step::AddLoginMethod
            | Step::CreateTunnel { .. }
            | Step::StopConnector { .. }
            | Step::DeleteTunnel { .. }
            | Step::Verify { .. } => {}
        }
    }
    out.sort_by(|a, b| (a.area, &a.hostname, &a.path).cmp(&(b.area, &b.hostname, &b.path)));
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::engine::types::TunnelRef;

    fn rule(value: serde_json::Value) -> IngressRule {
        serde_json::from_value(value).unwrap()
    }

    fn plan(steps: Vec<Step>) -> Plan {
        Plan {
            steps,
            warnings: Vec::new(),
            requires_confirmation: false,
            fingerprint: String::new(),
            tunnel_name: "Mac".into(),
        }
    }

    #[test]
    fn lists_route_and_record_changes_but_not_unchanged_rules() {
        let catch_all = json!({ "service": "http_status:404" });
        let previous = vec![
            rule(json!({ "hostname": "a.xyz.com", "service": "http://localhost:3000" })),
            rule(json!({ "hostname": "b.xyz.com", "service": "http://localhost:4000" })),
            rule(
                json!({ "hostname": "c.xyz.com", "path": "^/api", "service": "http://localhost:5000" }),
            ),
            rule(catch_all.clone()),
        ];
        let ingress = vec![
            rule(json!({ "hostname": "a.xyz.com", "service": "http://localhost:3000" })),
            rule(
                json!({ "hostname": "b.xyz.com", "service": "http://localhost:4000",
                         "originRequest": { "noTLSVerify": true, "httpHostHeader": "b" } }),
            ),
            rule(json!({ "hostname": "d.xyz.com", "service": "http://localhost:6000" })),
            rule(catch_all),
        ];
        let previous_record: cf_api::DnsRecord = serde_json::from_value(json!({
            "id": "r1", "type": "A", "name": "d.xyz.com", "content": "203.0.113.7",
            "proxied": false, "ttl": 1
        }))
        .unwrap();
        let plan = plan(vec![
            Step::PutConfig {
                tunnel: TunnelRef::Existing("t".into()),
                ingress,
                expected_version: Some(1),
                previous,
            },
            Step::UpdateRecord {
                zone_id: "z".into(),
                record_id: "r1".into(),
                hostname: "d.xyz.com".into(),
                tunnel: TunnelRef::Existing("t".into()),
                route_id: "route".into(),
                previous: previous_record,
            },
            Step::Verify {
                hostname: "d.xyz.com".into(),
            },
        ]);
        insta::assert_yaml_snapshot!(english(&deltas(&plan)));
    }

    #[test]
    fn unreported_steps_count_as_skipped_and_verify_is_left_out() {
        let plan = plan(vec![
            Step::CreateTunnel { name: "Mac".into() },
            Step::Verify {
                hostname: "a.xyz.com".into(),
            },
        ]);
        let intent = Intent::RemoveTunnel;
        let record = ActivityRecord::new(&intent, &plan, "acc", &[]);
        assert_eq!(record.kind, ActivityKind::RemoveTunnel);
        assert_eq!(record.steps.len(), 1);
        assert_eq!(record.steps[0].state, StepState::Skipped);
        // Records survive a round trip through the store's JSON.
        let json = serde_json::to_string(&record).unwrap();
        assert_eq!(
            serde_json::from_str::<ActivityRecord>(&json).unwrap(),
            record
        );
    }
}
