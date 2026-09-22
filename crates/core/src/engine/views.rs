//! What the UI sends and sees: validated change requests, plan previews and the routes
//! overview. Types here cross IPC.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::types::{Intent, Plan, RouteSpec, Snapshot, Step, Warning, ZoneRef, tunnel_target};
use crate::{
    domain::{Hostname, PathRule, RouteOrigin},
    runtime::ConnectorState,
};

/// A route as typed in the add/edit sheet.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteInput {
    /// Public hostname, e.g. `app.example.com`.
    pub hostname: String,
    /// Optional path regex, e.g. `^/api`.
    pub path: Option<String>,
    /// Origin, e.g. `3000` or `http://localhost:3000`.
    pub origin: String,
}

/// A change the user asks for.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Change {
    /// Add a route.
    AddRoute {
        /// The route.
        route: RouteInput,
    },
    /// Edit or rename a route.
    UpdateRoute {
        /// Current hostname.
        hostname: String,
        /// Current path.
        path: Option<String>,
        /// The new definition.
        route: RouteInput,
    },
    /// Remove a route.
    RemoveRoute {
        /// Hostname.
        hostname: String,
        /// Path.
        path: Option<String>,
    },
    /// Remove every route and delete this Mac's tunnel.
    RemoveTunnel,
    /// Undo an outside edit of this Mac's routes.
    RestoreConfig,
}

/// Rejected input, pointing at the field to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct InputError {
    /// `hostname`, `path` or `origin`.
    pub field: &'static str,
    /// What's wrong.
    pub message: String,
}

fn invalid(field: &'static str, err: &impl ToString) -> InputError {
    InputError {
        field,
        message: err.to_string(),
    }
}

pub(crate) fn parse_hostname(input: &str) -> Result<Hostname, InputError> {
    Hostname::parse(input).map_err(|e| invalid("hostname", &e))
}

pub(crate) fn parse_path(input: Option<&str>) -> Result<Option<PathRule>, InputError> {
    input
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(PathRule::parse)
        .transpose()
        .map_err(|e| invalid("path", &e))
}

/// A stable id for a route, written into its DNS record's comment. Derived from the
/// hostname and path, so a preview and the apply that follows agree without state.
pub fn route_id(hostname: &Hostname, path: Option<&PathRule>) -> String {
    let mut hash = Sha256::new();
    hash.update(hostname.as_str());
    hash.update([0]);
    hash.update(path.map_or("", PathRule::as_str));
    hash.finalize()[..6]
        .iter()
        .fold(String::with_capacity(12), |mut out, b| {
            use std::fmt::Write;
            let _ = write!(out, "{b:02x}");
            out
        })
}

impl RouteInput {
    /// Validates the input into a route (no extra origin options).
    ///
    /// # Errors
    /// The first invalid field.
    pub fn to_spec(&self) -> Result<RouteSpec, InputError> {
        let hostname = parse_hostname(&self.hostname)?;
        let path = parse_path(self.path.as_deref())?;
        let origin = RouteOrigin::parse(&self.origin).map_err(|e| invalid("origin", &e))?;
        Ok(RouteSpec {
            id: route_id(&hostname, path.as_ref()),
            hostname,
            path,
            origin,
            options: serde_json::Map::new(),
        })
    }
}

/// Turns a change into an intent against `snapshot` (edits keep the route's existing
/// origin options).
///
/// # Errors
/// Invalid input.
pub(crate) fn to_intent(change: &Change, snapshot: &Snapshot) -> Result<Intent, InputError> {
    Ok(match change {
        Change::AddRoute { route } => Intent::AddRoute {
            route: route.to_spec()?,
        },
        Change::UpdateRoute {
            hostname,
            path,
            route,
        } => {
            let hostname = parse_hostname(hostname)?;
            let path = parse_path(path.as_deref())?;
            let mut spec = route.to_spec()?;
            if let Some(existing) = snapshot.routes().into_iter().find(|r| {
                r.hostname.as_deref() == Some(hostname.as_str())
                    && r.path.as_deref() == path.as_ref().map(PathRule::as_str)
            }) {
                spec.options.clone_from(&existing.origin_request);
            }
            Intent::UpdateRoute {
                hostname,
                path,
                route: spec,
            }
        }
        Change::RemoveRoute { hostname, path } => Intent::RemoveRoute {
            hostname: parse_hostname(hostname)?,
            path: parse_path(path.as_deref())?,
        },
        Change::RemoveTunnel => Intent::RemoveTunnel,
        // Filled in by the engine from the drift record.
        Change::RestoreConfig => Intent::RestoreConfig {
            ingress: Vec::new(),
        },
    })
}

/// What a step does, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum StepKind {
    /// Create the tunnel.
    CreateTunnel,
    /// Update the tunnel's routes.
    PutConfig,
    /// Add a DNS record.
    CreateRecord,
    /// Repoint a DNS record.
    UpdateRecord,
    /// Delete a DNS record.
    DeleteRecord,
    /// Stop this Mac's connector.
    StopConnector,
    /// Delete the tunnel.
    DeleteTunnel,
    /// Check the route works.
    Verify,
}

/// One step of a plan, as shown in the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    /// What it does.
    pub kind: StepKind,
    /// One line for the user.
    pub description: String,
    /// "Copy as command" text, when there's an equivalent command.
    pub command: Option<String>,
}

/// A plan, as shown in the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    /// Steps in order (empty: nothing to change).
    pub steps: Vec<StepView>,
    /// Things to review.
    pub warnings: Vec<Warning>,
    /// Confirmation needed (touches records Teitunnel didn't create).
    pub requires_confirmation: bool,
    /// Pass back to apply, so a change made meanwhile is caught.
    pub fingerprint: String,
}

impl Plan {
    /// The preview of this plan.
    pub fn view(&self, account_id: &str) -> PlanView {
        let steps = self
            .steps
            .iter()
            .map(|step| StepView {
                kind: match step {
                    Step::CreateTunnel { .. } => StepKind::CreateTunnel,
                    Step::PutConfig { .. } => StepKind::PutConfig,
                    Step::CreateRecord { .. } => StepKind::CreateRecord,
                    Step::UpdateRecord { .. } => StepKind::UpdateRecord,
                    Step::DeleteRecord { .. } => StepKind::DeleteRecord,
                    Step::StopConnector { .. } => StepKind::StopConnector,
                    Step::DeleteTunnel { .. } => StepKind::DeleteTunnel,
                    Step::Verify { .. } => StepKind::Verify,
                },
                description: step.describe(&self.tunnel_name),
                command: step.command(account_id, &self.tunnel_name),
            })
            .collect();
        PlanView {
            steps,
            warnings: self.warnings.clone(),
            requires_confirmation: self.requires_confirmation,
            fingerprint: self.fingerprint.clone(),
        }
    }
}

/// Whether a route's DNS record points at this Mac's tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum DnsState {
    /// A proxied CNAME to the tunnel.
    Ok,
    /// No record.
    Missing,
    /// A record pointing somewhere else (or not proxied).
    Elsewhere {
        /// What it points at.
        content: String,
    },
}

/// One route of this Mac's tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteView {
    /// Public hostname.
    pub hostname: String,
    /// Path regex.
    pub path: Option<String>,
    /// Where traffic goes.
    pub origin: String,
    /// Whether the origin is on this Mac.
    pub local: bool,
    /// The zone (domain) it belongs to.
    pub zone: Option<String>,
    /// Its DNS record.
    pub dns: DnsState,
}

/// This Mac's tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TunnelView {
    /// Tunnel id.
    pub id: String,
    /// Name.
    pub name: String,
    /// Connector state on this Mac (`None`: not running).
    pub connector: Option<ConnectorState>,
}

/// Everything the Routes view shows for an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RoutesOverview {
    /// This Mac's tunnel, if it has one.
    pub tunnel: Option<TunnelView>,
    /// Routes, sorted by domain then hostname.
    pub routes: Vec<RouteView>,
    /// Domains routes can use.
    pub zones: Vec<ZoneRef>,
}

/// Builds the overview from a snapshot of every routed hostname.
pub(crate) fn overview(
    snapshot: &Snapshot,
    connector: impl Fn(&str) -> Option<ConnectorState>,
) -> RoutesOverview {
    let target = snapshot.tunnel.as_ref().map(|t| tunnel_target(&t.id));
    let mut routes: Vec<RouteView> = snapshot
        .routes()
        .into_iter()
        .filter_map(|rule| {
            let hostname = rule.hostname.clone()?;
            let parsed = Hostname::parse(&hostname).ok();
            let zone = parsed
                .as_ref()
                .and_then(|h| h.zone_in(&snapshot.zones))
                .map(|z| z.name.clone());
            let records: Vec<_> = snapshot
                .records_named(&hostname)
                .filter(|r| matches!(r.record.kind.as_str(), "A" | "AAAA" | "CNAME"))
                .collect();
            let dns = if records.iter().any(|r| {
                r.record.proxied
                    && target
                        .as_deref()
                        .is_some_and(|t| r.record.content.eq_ignore_ascii_case(t))
            }) {
                DnsState::Ok
            } else if let Some(first) = records.first() {
                DnsState::Elsewhere {
                    content: first.record.content.clone(),
                }
            } else {
                DnsState::Missing
            };
            Some(RouteView {
                local: RouteOrigin::parse(&rule.service).is_ok_and(|o| o.is_local()),
                origin: rule.service.clone(),
                path: rule.path.clone(),
                zone,
                dns,
                hostname,
            })
        })
        .collect();
    routes.sort_by(|a, b| (&a.zone, &a.hostname, &a.path).cmp(&(&b.zone, &b.hostname, &b.path)));
    RoutesOverview {
        tunnel: snapshot.tunnel.as_ref().map(|t| TunnelView {
            id: t.id.clone(),
            name: t.name.clone(),
            connector: connector(&t.id),
        }),
        routes,
        zones: snapshot.zones.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(hostname: &str, path: Option<&str>, origin: &str) -> RouteInput {
        RouteInput {
            hostname: hostname.into(),
            path: path.map(str::to_owned),
            origin: origin.into(),
        }
    }

    #[test]
    fn validates_each_field() {
        assert_eq!(
            input("", None, "3000").to_spec().unwrap_err().field,
            "hostname"
        );
        assert_eq!(
            input("app.xyz.com", Some("(?=x)"), "3000")
                .to_spec()
                .unwrap_err()
                .field,
            "path"
        );
        assert_eq!(
            input("app.xyz.com", None, "nope://x")
                .to_spec()
                .unwrap_err()
                .field,
            "origin"
        );
        let spec = input("App.XYZ.com", Some("  "), "3000").to_spec().unwrap();
        assert_eq!(spec.hostname.as_str(), "app.xyz.com");
        assert_eq!(spec.path, None, "a blank path means none");
        assert_eq!(spec.origin.as_str(), "http://localhost:3000");
    }

    #[test]
    fn route_ids_are_stable_and_distinct() {
        let a = Hostname::parse("app.xyz.com").unwrap();
        let api = PathRule::parse("^/api").unwrap();
        assert_eq!(route_id(&a, None), route_id(&a, None));
        assert_ne!(route_id(&a, None), route_id(&a, Some(&api)));
        assert_eq!(route_id(&a, None).len(), 12);
    }
}
