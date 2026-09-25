//! What the UI sends and sees: validated change requests, plan previews and the routes
//! overview. Types here cross IPC.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    access::{AccessRule, access_domain},
    networks::NETWORK_COMMENT,
    types::{Intent, Plan, RouteSpec, Snapshot, Step, Warning, ZoneRef, tunnel_target},
};
use crate::{
    domain::{ClientAccess, Hostname, OriginOptions, PathRule, PrivateNetwork, RouteOrigin},
    runtime::ConnectorState,
    text::{Text, UserText, english_display},
};

/// A route as typed in the add/edit sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteInput {
    /// Public hostname, e.g. `app.example.com`.
    pub hostname: String,
    /// Optional path regex, e.g. `^/api`.
    pub path: Option<String>,
    /// Origin, e.g. `3000` or `http://localhost:3000`.
    pub origin: String,
    /// Require a login for these people. On an edit, `None` removes the login
    /// Teitunnel added; on an add, it leaves any existing login alone.
    #[serde(default)]
    pub access: Option<AccessRule>,
    /// Origin settings. On an edit, `None` keeps the route's current ones.
    #[serde(default)]
    pub options: Option<Box<OriginOptions>>,
}

/// A change the user asks for (or a Doctor fix proposes).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Load balance a route across the tunnels that route its hostname.
    BalanceRoute {
        /// Hostname.
        hostname: String,
    },
    /// Stop load balancing a route.
    UnbalanceRoute {
        /// Hostname.
        hostname: String,
    },
    /// Create another tunnel for this Mac.
    CreateTunnel {
        /// Its name.
        name: String,
    },
    /// Undo an outside edit of this Mac's routes.
    RestoreConfig,
    /// Remove a login Teitunnel added whose route is gone.
    RemoveLogin {
        /// The Access domain, e.g. `app.example.com` or `app.example.com/admin`.
        domain: String,
    },
    /// Remove what Teitunnel attached to a hostname without routes (Doctor).
    CleanUpHostname {
        /// The hostname.
        hostname: String,
    },
    /// Add several routes at once (import from an existing cloudflared setup).
    ImportRoutes {
        /// The routes.
        routes: Vec<RouteInput>,
    },
    /// Let WARP clients reach a private range through this Mac's tunnel.
    AddNetwork {
        /// An IP address or CIDR range, e.g. `192.168.1.0/24`.
        network: String,
    },
    /// Stop sharing a private range.
    RemoveNetwork {
        /// The range.
        network: String,
    },
    /// Delete one DNS record (an orphan found by the Doctor).
    DeleteRecord {
        /// Zone id.
        zone_id: String,
        /// The record's name.
        hostname: String,
        /// Record id.
        record_id: String,
    },
    /// Reserve a hostname for this owner, so teammates sharing the account see it's
    /// taken (M12-11). Reserving it again changes the end date.
    ReserveHostname {
        /// The hostname.
        hostname: String,
        /// When the reservation ends: `2026-12-31` (end of that day, UTC) or
        /// `2026-12-31T18:00Z`; `None` or empty: no end.
        #[serde(default)]
        until: Option<String>,
    },
    /// Give up a hostname's reservation (a route there stays).
    ReleaseHostname {
        /// The hostname.
        hostname: String,
    },
    /// Enforce protection at Cloudflare's edge for a hostname (the default removes
    /// Teitunnel's rules).
    ProtectHostname {
        /// The hostname.
        hostname: String,
        /// What to enforce.
        protection: super::edge::EdgeProtection,
    },
}

/// Rejected input, pointing at the field to fix.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub struct InputError {
    /// `hostname`, `path`, `origin`, `access` or `network`.
    pub field: &'static str,
    /// What's wrong.
    pub message: Text,
}

impl UserText for InputError {
    fn text(&self) -> Text {
        self.message.clone()
    }
}

english_display!(InputError);

fn invalid(field: &'static str, err: &impl UserText) -> InputError {
    InputError {
        field,
        message: err.text(),
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
    fn validated_options(&self) -> Result<OriginOptions, InputError> {
        self.options
            .as_ref()
            .map_or(Ok(OriginOptions::default()), |o| o.validated())
            .map_err(|e| InputError {
                field: "options",
                message: e.text(),
            })
    }

    /// Validates the input into a route.
    ///
    /// # Errors
    /// The first invalid field.
    pub fn to_spec(&self) -> Result<RouteSpec, InputError> {
        let hostname = parse_hostname(&self.hostname)?;
        let path = parse_path(self.path.as_deref())?;
        let origin = RouteOrigin::parse(&self.origin).map_err(|e| invalid("origin", &e))?;
        let access = self
            .access
            .as_ref()
            .map(|rule| {
                access_domain(&hostname, path.as_ref()).map_err(|e| invalid("path", &e))?;
                rule.normalized().map_err(|e| invalid("access", &e))
            })
            .transpose()?;
        let mut options = serde_json::Map::new();
        self.validated_options()?.apply(&mut options);
        Ok(RouteSpec {
            id: route_id(&hostname, path.as_ref()),
            hostname,
            path,
            origin,
            options,
            access,
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
            // Settings made elsewhere (the dashboard, a config file) stay; the known ones
            // change only when the edit carries them.
            if let Some(existing) = snapshot.routes().into_iter().find(|r| {
                r.hostname.as_deref() == Some(hostname.as_str())
                    && r.path.as_deref() == path.as_ref().map(PathRule::as_str)
            }) {
                spec.options.clone_from(&existing.origin_request);
                if route.options.is_some() {
                    route.validated_options()?.apply(&mut spec.options);
                }
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
        Change::CreateTunnel { name } => Intent::CreateTunnel { name: name.clone() },
        Change::BalanceRoute { hostname } => Intent::BalanceRoute {
            hostname: parse_hostname(hostname)?,
        },
        Change::UnbalanceRoute { hostname } => Intent::UnbalanceRoute {
            hostname: parse_hostname(hostname)?,
        },
        Change::ImportRoutes { routes } => Intent::ImportRoutes {
            routes: routes
                .iter()
                .map(RouteInput::to_spec)
                .collect::<Result<_, _>>()?,
        },
        Change::DeleteRecord {
            zone_id,
            hostname,
            record_id,
        } => Intent::DeleteRecord {
            zone_id: zone_id.clone(),
            hostname: parse_hostname(hostname)?,
            record_id: record_id.clone(),
        },
        Change::AddNetwork { network } => Intent::AddNetwork {
            network: PrivateNetwork::parse(network).map_err(|e| invalid("network", &e))?,
        },
        Change::RemoveNetwork { network } => Intent::RemoveNetwork {
            network: PrivateNetwork::parse(network).map_err(|e| invalid("network", &e))?,
        },
        Change::RemoveLogin { domain } => Intent::RemoveLogin {
            domain: domain.trim().to_ascii_lowercase(),
        },
        Change::CleanUpHostname { hostname } => Intent::CleanUpHostname {
            hostname: parse_hostname(hostname)?,
        },
        // Filled in by the engine from the drift record.
        Change::RestoreConfig => Intent::RestoreConfig {
            ingress: Vec::new(),
        },
        Change::ReserveHostname { hostname, until } => Intent::Reserve {
            hostname: parse_hostname(hostname)?,
            until: parse_lease_end(until.as_deref(), crate::domain_shares::now_ms())?,
        },
        Change::ReleaseHostname { hostname } => Intent::Release {
            hostname: parse_hostname(hostname)?,
        },
        Change::ProtectHostname {
            hostname,
            protection,
        } => Intent::ProtectHostname {
            hostname: parse_hostname(hostname)?,
            protection: protection
                .normalized()
                .map_err(|e| invalid("protection", &e))?,
        },
    })
}

/// A reservation's end as typed (`2026-12-31`, `2026-12-31T18:00Z`; empty: none), which
/// must be after `now`.
///
/// # Errors
/// Not a date, or in the past (field `until`).
pub fn parse_lease_end(input: Option<&str>, now: u64) -> Result<Option<u64>, InputError> {
    use crate::text::msg::reservations::error as m;
    let Some(input) = input.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let until = super::ownership::parse_until(input).ok_or_else(|| InputError {
        field: "until",
        message: m::invalid_until(),
    })?;
    if until <= now {
        return Err(InputError {
            field: "until",
            message: m::until_past(),
        });
    }
    Ok(Some(until))
}

/// What a step does, for its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
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
    /// Add a login method.
    LoginMethod,
    /// Create, change or remove a route's login.
    AccessApp,
    /// Route or stop routing a private network.
    NetworkRoute,
    /// Load balance a route, or stop.
    LoadBalancer,
    /// Check the route works.
    Verify,
    /// Upload, publish, roll back or delete a Snapshot.
    Snapshot,
    /// Give a Snapshot its address, or take it away.
    SnapshotAddress,
    /// Reserve a hostname, renew or end a reservation.
    Reservation,
    /// Add, change or remove an edge rule (bots, rate limit, headers).
    EdgeRule,
    /// Create, rotate or delete a service token, or let one through a login.
    ServiceToken,
    /// Create the account's D1 database (comments, webhook inboxes).
    Database,
    /// Add, change or remove a Worker in front of a route (offline page, webhook inbox).
    FrontWorker,
}

/// One step of a plan, as shown in the preview.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    /// What it does.
    pub kind: StepKind,
    /// One line for the user.
    pub description: Text,
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

impl Step {
    /// This step as shown in the preview (and recorded in the activity log).
    pub fn view(&self, account_id: &str, tunnel_name: &str) -> StepView {
        StepView {
            kind: match self {
                Self::CreateTunnel { .. } => StepKind::CreateTunnel,
                Self::PutConfig { .. } => StepKind::PutConfig,
                Self::CreateRecord { .. } => StepKind::CreateRecord,
                Self::UpdateRecord { .. } => StepKind::UpdateRecord,
                Self::DeleteRecord { .. } => StepKind::DeleteRecord,
                Self::StopConnector { .. } => StepKind::StopConnector,
                Self::DeleteTunnel { .. } => StepKind::DeleteTunnel,
                Self::AddLoginMethod => StepKind::LoginMethod,
                Self::CreateAccessApp { .. }
                | Self::UpdateAccessApp { .. }
                | Self::DeleteAccessApp { .. } => StepKind::AccessApp,
                Self::CreateNetworkRoute { .. } | Self::DeleteNetworkRoute { .. } => {
                    StepKind::NetworkRoute
                }
                Self::CreateLbMonitor { .. }
                | Self::CreateLbPool { .. }
                | Self::UpdateLbPool { .. }
                | Self::CreateLoadBalancer { .. }
                | Self::DeleteLoadBalancer { .. }
                | Self::DeleteLbPool { .. }
                | Self::DeleteLbMonitor { .. } => StepKind::LoadBalancer,
                Self::Verify { .. } => StepKind::Verify,
                Self::UploadSnapshotFiles { .. }
                | Self::CreateSnapshotWorker { .. }
                | Self::PublishSnapshotVersion { .. }
                | Self::RollBackSnapshot { .. }
                | Self::DeleteSnapshotWorker { .. } => StepKind::Snapshot,
                Self::EnableWorkersDev { .. }
                | Self::DisableWorkersDev { .. }
                | Self::AttachSnapshotDomain { .. }
                | Self::DetachSnapshotDomain { .. } => StepKind::SnapshotAddress,
                Self::CreateReservation { .. } | Self::SetLease { .. } => StepKind::Reservation,
                Self::CreateEdgeRule { .. }
                | Self::UpdateEdgeRule { .. }
                | Self::DeleteEdgeRule { .. } => StepKind::EdgeRule,
                Self::CreateServiceToken { .. }
                | Self::AllowServiceToken { .. }
                | Self::DeleteServiceToken { .. }
                | Self::RotateServiceToken { .. } => StepKind::ServiceToken,
                Self::CreateDatabase { .. } => StepKind::Database,
                Self::PutFrontWorker { .. }
                | Self::CreateWorkerRoute { .. }
                | Self::DeleteWorkerRoute { .. }
                | Self::DeleteFrontWorker { .. } => StepKind::FrontWorker,
            },
            description: self.describe(tunnel_name),
            command: self.command(account_id, tunnel_name),
        }
    }
}

impl Plan {
    /// The preview of this plan.
    pub fn view(&self, account_id: &str) -> PlanView {
        PlanView {
            steps: self
                .steps
                .iter()
                .map(|step| step.view(account_id, &self.tunnel_name))
                .collect(),
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
        /// Who holds the name now, when it's another Teitunnel (their route or
        /// reservation).
        #[serde(rename = "heldBy")]
        held_by: Option<super::ownership::Hold>,
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
    /// Who may reach it, when Teitunnel added a login.
    pub access: Option<AccessRule>,
    /// What visitors run to reach it, for SSH, RDP, SMB and TCP routes.
    pub client: Option<ClientAccess>,
    /// The tunnel of this Mac's that carries it.
    pub tunnel_id: Option<String>,
    /// A share on your domain: removed when the share stops.
    pub temporary: bool,
    /// Load balanced across tunnels (Cloudflare Load Balancing).
    pub balanced: bool,
    /// Visitors get the "paused" page ([`crate::pause`]).
    pub paused: bool,
    /// Its origin settings.
    pub options: OriginOptions,
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
    /// The machine tunnel: where routes go unless another is chosen.
    pub is_default: bool,
}

/// Everything the Routes view shows for an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RoutesOverview {
    /// This Mac's default tunnel, if it has one.
    pub tunnel: Option<TunnelView>,
    /// Every tunnel of this Mac's in the account, the default first, then by name.
    pub tunnels: Vec<TunnelView>,
    /// Routes, sorted by domain then hostname.
    pub routes: Vec<RouteView>,
    /// Domains routes can use.
    pub zones: Vec<ZoneRef>,
    /// Private networks shared through this Mac's tunnel, sorted; `None` when the
    /// credential can't read them.
    pub networks: Option<Vec<NetworkView>>,
}

/// A private network shared through this Mac's tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct NetworkView {
    /// The range, e.g. `192.168.1.0/24`.
    pub network: String,
    /// In private address space (a public range takes those addresses over for WARP
    /// clients).
    pub private: bool,
    /// Teitunnel added it (otherwise it was added in the dashboard or with cloudflared).
    pub owned: bool,
}

/// How a route is doing, from its DNS record and this Mac's connector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum RouteHealth {
    /// Serving.
    Live,
    /// No DNS record for the hostname.
    NoDns,
    /// The record points somewhere else.
    DnsElsewhere,
    /// The connector is starting or connecting.
    Connecting,
    /// The connector crashed and is restarting.
    Restarting,
    /// The connector lost its connection.
    ConnectionLost,
    /// The connector keeps exiting.
    KeepsStopping,
    /// The connector isn't running.
    Stopped,
}

impl RouteHealth {
    /// Whether the route serves.
    pub fn is_live(self) -> bool {
        self == Self::Live
    }

    /// A few words for the menu bar and the CLI.
    pub fn text(self) -> Text {
        use crate::text::msg::route::status as m;
        match self {
            Self::Live => m::live(),
            Self::NoDns => m::no_dns(),
            Self::DnsElsewhere => m::dns_elsewhere(),
            Self::Connecting => m::connecting(),
            Self::Restarting => m::restarting(),
            Self::ConnectionLost => m::connection_lost(),
            Self::KeepsStopping => m::keeps_stopping(),
            Self::Stopped => m::stopped(),
        }
    }
}

impl RoutesOverview {
    /// Each route's hostname and health (menu bar, CLI).
    pub fn statuses(&self) -> Vec<(String, RouteHealth)> {
        self.routes
            .iter()
            .map(|route| {
                // Each route is as healthy as the connector of the tunnel carrying it.
                let connector = self
                    .tunnels
                    .iter()
                    .chain(&self.tunnel)
                    .find(|t| route.tunnel_id.as_deref().is_none_or(|id| id == t.id))
                    .and_then(|t| t.connector.as_ref());
                let health = match (&route.dns, connector) {
                    (DnsState::Missing, _) => RouteHealth::NoDns,
                    (DnsState::Elsewhere { .. }, _) => RouteHealth::DnsElsewhere,
                    (_, Some(ConnectorState::Healthy { .. })) => RouteHealth::Live,
                    (_, Some(ConnectorState::Starting | ConnectorState::Connecting)) => {
                        RouteHealth::Connecting
                    }
                    (_, Some(ConnectorState::Crashed { .. })) => RouteHealth::Restarting,
                    (_, Some(ConnectorState::Degraded)) => RouteHealth::ConnectionLost,
                    (_, Some(ConnectorState::CrashLoop { .. })) => RouteHealth::KeepsStopping,
                    _ => RouteHealth::Stopped,
                };
                (route.hostname.clone(), health)
            })
            .collect()
    }
}

/// Builds the overview from a snapshot of every routed hostname.
pub(crate) fn overview(
    snapshot: &Snapshot,
    is_default: bool,
    connector: impl Fn(&str) -> Option<ConnectorState>,
) -> RoutesOverview {
    let target = snapshot.tunnel.as_ref().map(|t| tunnel_target(&t.id));
    let mut routes: Vec<RouteView> = snapshot
        .routes()
        .into_iter()
        .filter_map(|rule| {
            let hostname = rule.hostname.clone()?;
            let parsed = Hostname::parse(&hostname).ok();
            let path = rule.path.as_deref().and_then(|p| PathRule::parse(p).ok());
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
                    held_by: snapshot
                        .held
                        .iter()
                        .find(|h| h.hostname.eq_ignore_ascii_case(&hostname))
                        .cloned(),
                }
            } else {
                DnsState::Missing
            };
            let access = snapshot.access.as_ref().and_then(|a| {
                let domain = access_domain(parsed.as_ref()?, path.as_ref()).ok()?;
                let app = a.app(&domain).filter(|app| app.owned)?;
                app.rule.clone()
            });
            let origin = RouteOrigin::parse(&rule.service).ok();
            Some(RouteView {
                access,
                client: parsed
                    .as_ref()
                    .zip(origin.as_ref())
                    .and_then(|(h, o)| ClientAccess::of(h, o)),
                local: origin.is_some_and(|o| o.is_local()),
                origin: rule.service.clone(),
                path: rule.path.clone(),
                zone,
                dns,
                hostname,
                tunnel_id: snapshot.tunnel.as_ref().map(|t| t.id.clone()),
                temporary: false,
                balanced: false,
                paused: false,
                options: OriginOptions::from_map(&rule.origin_request),
            })
        })
        .collect();
    routes.sort_by(|a, b| (&a.zone, &a.hostname, &a.path).cmp(&(&b.zone, &b.hostname, &b.path)));
    let tunnel = snapshot.tunnel.as_ref().map(|t| TunnelView {
        id: t.id.clone(),
        name: t.name.clone(),
        connector: connector(&t.id),
        is_default,
    });
    RoutesOverview {
        tunnels: tunnel.iter().cloned().collect(),
        tunnel: tunnel.filter(|t| t.is_default),
        routes,
        zones: snapshot.zones.clone(),
        networks: snapshot.networks.as_ref().map(|state| {
            let Some(tunnel) = &snapshot.tunnel else {
                return Vec::new();
            };
            let mut networks: Vec<(PrivateNetwork, NetworkView)> = state
                .of_tunnel(&tunnel.id)
                .filter_map(|r| {
                    let range = r.range()?;
                    Some((
                        range,
                        NetworkView {
                            network: range.to_string(),
                            private: range.is_private(),
                            owned: r.comment == NETWORK_COMMENT,
                        },
                    ))
                })
                .collect();
            networks.sort_by_key(|a| a.0);
            networks.dedup_by(|a, b| a.0 == b.0);
            networks.into_iter().map(|(_, view)| view).collect()
        }),
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
            access: None,
            options: None,
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

    #[test]
    fn route_statuses_combine_dns_and_connector() {
        let route = |host: &str, dns: DnsState| RouteView {
            options: OriginOptions::default(),
            access: None,
            client: None,
            tunnel_id: None,
            temporary: false,
            balanced: false,
            paused: false,
            hostname: host.into(),
            path: None,
            origin: "http://localhost:3000".into(),
            local: true,
            zone: None,
            dns,
        };
        let mut overview = RoutesOverview {
            tunnel: Some(TunnelView {
                id: "t".into(),
                name: "Mac".into(),
                connector: Some(ConnectorState::Healthy { connections: 4 }),
                is_default: true,
            }),
            tunnels: Vec::new(),
            routes: vec![
                route("a.xyz.com", DnsState::Ok),
                route("b.xyz.com", DnsState::Missing),
            ],
            zones: Vec::new(),
            networks: None,
        };
        assert_eq!(
            overview.statuses(),
            [
                ("a.xyz.com".to_owned(), RouteHealth::Live),
                ("b.xyz.com".to_owned(), RouteHealth::NoDns)
            ]
        );
        assert_eq!(RouteHealth::NoDns.text().english(), "No DNS record");
        overview.tunnel.as_mut().unwrap().connector = None;
        assert_eq!(overview.statuses()[0].1, RouteHealth::Stopped);
    }
}
