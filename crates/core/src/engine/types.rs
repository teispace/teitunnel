use cf_api::{DnsRecord, IngressRule};
use serde::Serialize;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::domain::{Hostname, PathRule, RouteOrigin};

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
    /// Names of the account's tunnels, read only when this Mac has none (a new
    /// tunnel gets a name no other tunnel has).
    pub tunnel_names: Vec<String>,
    /// DNS records for the hostnames involved.
    pub records: Vec<ObservedRecord>,
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
    /// Remove every route and delete the machine tunnel.
    RemoveTunnel,
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
            Self::RemoveRoute { hostname, .. } => Some(vec![hostname]),
            Self::RemoveTunnel => None,
            Self::RestoreConfig { .. } => Some(Vec::new()),
        }
    }

    /// A one-line summary for the activity log.
    pub fn summary(&self) -> String {
        fn target(hostname: &Hostname, path: Option<&PathRule>) -> String {
            path.map_or_else(
                || hostname.to_string(),
                |p| format!("{hostname} (path {})", p.as_str()),
            )
        }
        match self {
            Self::AddRoute { route } => format!(
                "Add {} → {}",
                target(&route.hostname, route.path.as_ref()),
                route.origin
            ),
            Self::UpdateRoute {
                hostname,
                path,
                route,
            } => {
                let before = target(hostname, path.as_ref());
                let after = target(&route.hostname, route.path.as_ref());
                if before == after {
                    format!("Change {before} → {}", route.origin)
                } else {
                    format!("Rename {before} to {after} → {}", route.origin)
                }
            }
            Self::RemoveRoute { hostname, path } => {
                format!("Remove {}", target(hostname, path.as_ref()))
            }
            Self::RemoveTunnel => "Remove every route and delete this Mac's tunnel".to_owned(),
            Self::RestoreConfig { .. } => {
                "Restore this Mac's routes after an outside edit".to_owned()
            }
        }
    }
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
    /// Probe the hostname end to end.
    Verify {
        /// Hostname.
        hostname: String,
    },
}

impl Step {
    /// A one-line description for the plan preview.
    pub fn describe(&self, tunnel_name: &str) -> String {
        match self {
            Self::CreateTunnel { name } => format!("Create tunnel “{name}”"),
            Self::PutConfig { ingress, .. } => {
                let routes = ingress.iter().filter(|r| r.hostname.is_some()).count();
                format!(
                    "Update tunnel “{tunnel_name}” to serve {routes} route{}",
                    if routes == 1 { "" } else { "s" }
                )
            }
            Self::CreateRecord { hostname, .. } => {
                format!("Add DNS record {hostname} → tunnel “{tunnel_name}”")
            }
            Self::UpdateRecord {
                hostname, previous, ..
            } => format!(
                "Point {hostname} at tunnel “{tunnel_name}” (was {} {})",
                previous.kind, previous.content
            ),
            Self::DeleteRecord { record, .. } => format!(
                "Delete DNS record {} ({} {})",
                record.name, record.kind, record.content
            ),
            Self::StopConnector { .. } => "Stop this Mac's connector".to_owned(),
            Self::DeleteTunnel { .. } => format!("Delete tunnel “{tunnel_name}”"),
            Self::Verify { hostname } => format!("Check https://{hostname} works"),
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
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Warning {
    /// A record Teitunnel didn't create will be replaced.
    ReplacesForeignRecord {
        /// Hostname.
        hostname: String,
        /// Existing type.
        kind: String,
        /// Existing content.
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
}

impl Plan {
    /// Whether there's nothing to do (ignoring verification).
    pub fn is_empty(&self) -> bool {
        self.steps.iter().all(|s| !s.is_mutation())
    }
}
