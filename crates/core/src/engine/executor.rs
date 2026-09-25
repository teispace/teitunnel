//! The executor: applies a reviewed plan, step by step, and undoes completed steps in
//! reverse order if one fails (ARCHITECTURE §4.4).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cf_api::{DnsRecord, IngressRule, NewAccessApp, NewDnsRecord, TunnelConfig};
use serde::Serialize;
use tokio::time::Instant;

use super::activity::ActivityRecord;
use super::drift::{Drift, diff};
use super::observe::ObserveNeed;
use super::tunnels::TunnelSummary;
use super::views::{Change, InputError, RoutesOverview, overview, to_intent};

/// A snapshot with nothing in it, for changes that don't need one to be parsed.
static EMPTY: Snapshot = Snapshot {
    account_id: String::new(),
    machine_name: String::new(),
    zones: Vec::new(),
    tunnel: None,
    tunnel_names: Vec::new(),
    elsewhere: Vec::new(),
    records: Vec::new(),
    access: None,
    networks: None,
    balance: None,
    site: None,
    held: Vec::new(),
    owner: String::new(),
    now: 0,
    edge: Vec::new(),
    service_tokens: None,
    database: None,
    front: Vec::new(),
};
use super::verify::{Edge, Failure, Verification, check_dns, probe};
use super::{
    cloud::{CloudApi, Connectors},
    local::Local,
    networks::NETWORK_COMMENT,
    observe::{ObserveError, observe},
    planner::{PlanError, plan},
    types::{Intent, Plan, Snapshot, Step, TunnelRef, tunnel_target},
};
use crate::domain::{Hostname, RouteOrigin};

use crate::text::{Text, UserText, english_display, msg};

/// How long a preview may reuse an observation.
const CACHE_TTL: Duration = Duration::from_secs(5);

/// Why a change couldn't be previewed or started.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// The intent can't be planned.
    #[error(transparent)]
    Plan(#[from] PlanError),
    /// Reading the current state failed.
    #[error(transparent)]
    Observe(#[from] ObserveError),
    /// Something changed since the plan was reviewed; here's the new plan.
    Stale(Box<Plan>),
    /// The plan touches records Teitunnel didn't create and wasn't confirmed.
    NeedsConfirmation,
    /// The request itself is invalid.
    #[error(transparent)]
    Input(#[from] InputError),
    /// "Restore mine" when nothing was changed elsewhere.
    NothingToRestore,
    /// The tunnel can't be run on this Mac too (why, for the user).
    Adopt(Text),
}

impl UserText for EngineError {
    fn text(&self) -> Text {
        match self {
            Self::Plan(err) => err.text(),
            Self::Observe(err) => err.text(),
            Self::Input(err) => err.text(),
            Self::Stale(_) => msg::error::engine::stale(),
            Self::NeedsConfirmation => msg::error::engine::needs_confirmation(),
            Self::NothingToRestore => msg::error::engine::nothing_to_restore(),
            Self::Adopt(text) => text.clone(),
        }
    }
}

english_display!(EngineError);

/// How often a transient verification failure is retried.
const VERIFY_RETRY: Duration = Duration::from_secs(2);

/// How long a check right after a change waits out transient failures: Cloudflare takes
/// about half a minute to connect a new record to a tunnel (1016 until then; measured
/// 2026-09-25), so 30 s cut it short.
pub const VERIFY_PATIENCE: Duration = Duration::from_secs(60);

/// Who is asking: the account and this Mac's name for a new tunnel.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// Account id.
    pub account: &'a str,
    /// Name for a new machine tunnel.
    pub machine_name: &'a str,
    /// Which of this Mac's tunnels the change is about (its id); `None`: the default one.
    pub tunnel: Option<&'a str>,
}

/// What the user approved: the plan they reviewed (by fingerprint) and whether they
/// confirmed changes to records Teitunnel doesn't own.
#[derive(Debug, Clone, Copy)]
pub struct Approval<'a> {
    /// Fingerprint of the reviewed plan.
    pub fingerprint: &'a str,
    /// Confirmed replacing foreign records.
    pub confirmed: bool,
}

/// The state of one step while applying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum StepState {
    /// In progress.
    Running,
    /// Finished.
    Done,
    /// Not run by the executor (verification happens afterwards).
    Skipped,
    /// Failed with a message.
    Failed {
        /// What went wrong.
        message: Text,
    },
    /// Being undone after a later step failed.
    Undoing,
    /// Undone.
    Undone,
    /// Couldn't be undone; left in place.
    UndoFailed {
        /// What went wrong.
        message: Text,
    },
    /// Sending a Snapshot's files: how far along.
    Transferring {
        /// Files sent.
        #[cfg_attr(feature = "specta", specta(type = f64))]
        files: u64,
        /// Of this many.
        #[serde(rename = "totalFiles")]
        #[cfg_attr(feature = "specta", specta(type = f64))]
        total_files: u64,
        /// Bytes sent.
        #[cfg_attr(feature = "specta", specta(type = f64))]
        bytes: u64,
        /// Of this many.
        #[serde(rename = "totalBytes")]
        #[cfg_attr(feature = "specta", specta(type = f64))]
        total_bytes: u64,
    },
}

/// A progress update for the step at `step` (index into the plan).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Step index.
    pub step: u32,
    /// Its state.
    pub state: StepState,
}

/// How applying ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Outcome {
    /// Every step succeeded.
    Applied {
        /// This Mac's tunnel afterwards (None once deleted).
        tunnel_id: Option<String>,
        /// Hostnames to verify next.
        verify: Vec<String>,
        /// The connector couldn't be started (the routes are configured, though).
        connector_error: Option<Text>,
    },
    /// A step failed and everything done before it was undone.
    RolledBack {
        /// Index of the failed step.
        failed_step: u32,
        /// Why it failed.
        error: Text,
    },
    /// A step failed and some earlier changes couldn't be undone.
    PartiallyApplied {
        /// Index of the failed step.
        failed_step: u32,
        /// Why it failed.
        error: Text,
        /// What was left in place.
        leftovers: Vec<Text>,
    },
}

impl Outcome {
    fn label(&self) -> &'static str {
        match self {
            Self::Applied { .. } => "applied",
            Self::RolledBack { .. } => "rolledBack",
            Self::PartiallyApplied { .. } => "partiallyApplied",
        }
    }
}

/// How to reverse a completed step.
#[derive(Debug)]
enum Undo {
    DeleteTunnel(String),
    RestoreConfig {
        tunnel: String,
        previous: Vec<IngressRule>,
    },
    DeleteRecord {
        zone: String,
        id: String,
        name: String,
    },
    RestoreRecord {
        zone: String,
        previous: DnsRecord,
        /// The record's id now: `previous.id` if it was changed in place, else the
        /// replacement's (a type change deletes and creates).
        current: String,
        was_owned: bool,
    },
    RecreateRecord {
        zone: String,
        record: DnsRecord,
        was_owned: bool,
    },
    StartConnector(String),
    DeleteLoginMethod(String),
    DeleteAccessApp {
        id: String,
        domain: String,
        /// It let everyone through a path (not a login).
        bypass: bool,
    },
    RestoreAccessApp {
        id: String,
        previous: NewAccessApp,
    },
    RecreateAccessApp(NewAccessApp),
    DeleteNetworkRoute {
        id: String,
        network: String,
    },
    RecreateNetworkRoute(super::networks::ObservedNetworkRoute),
    DeleteLbMonitor(String),
    DeleteLbPool(String),
    RestoreLbPool {
        id: String,
        previous: cf_api::Pool,
    },
    DeleteLoadBalancer {
        zone: String,
        id: String,
        hostname: String,
    },
    RecreateLoadBalancer {
        zone: String,
        balancer: cf_api::LoadBalancer,
    },
    RecreateLbPool(cf_api::Pool),
    RecreateLbMonitor(cf_api::Monitor),
    DeleteSnapshotWorker {
        snapshot: String,
        script: String,
    },
    RedeploySnapshot {
        snapshot: String,
        script: String,
        version: String,
        /// The version the step made live, forgotten when it was new.
        replaced: Option<String>,
    },
    SetWorkersDev {
        script: String,
        enabled: bool,
    },
    DetachSnapshotDomain {
        id: String,
        hostname: String,
    },
    ReattachSnapshotDomain(cf_api::WorkerDomain),
    /// A deleted Worker can't come back.
    RecreateSnapshotWorker(String),
    DeleteEdgeRule {
        zone: String,
        ruleset: String,
        id: String,
        hostnames: String,
    },
    RestoreEdgeRule {
        zone: String,
        ruleset: String,
        id: String,
        previous: cf_api::NewRule,
        hostnames: String,
    },
    RecreateEdgeRule {
        zone: String,
        phase: String,
        ruleset: String,
        rule: cf_api::NewRule,
        position: u32,
        hostnames: String,
    },
    DeleteServiceToken {
        id: String,
        name: String,
    },
    /// A deleted token (or a replaced secret) can't come back.
    RestoreServiceToken(String),
    DeleteDatabase(String),
    DeleteFrontWorker {
        hostname: String,
        script: String,
        config: super::front::FrontConfig,
    },
    /// Put a front Worker back as it was (after a replacement or a deletion).
    RestoreFrontWorker {
        hostname: String,
        zone_id: String,
        script: String,
        config: super::front::FrontConfig,
        database: Option<String>,
    },
    DeleteWorkerRoute {
        zone: String,
        id: String,
        pattern: String,
        hostname: String,
        kind: super::front::FrontKind,
        path: String,
    },
    RecreateWorkerRoute {
        zone: String,
        route: cf_api::WorkerRoute,
        hostname: String,
        kind: super::front::FrontKind,
        path: String,
    },
}

impl Undo {
    fn leftover(&self) -> Text {
        use crate::text::msg::apply::leftover as m;
        match self {
            Self::DeleteTunnel(id) => m::delete_tunnel(id),
            Self::RestoreConfig { .. } => m::restore_config(),
            Self::DeleteRecord { name, .. } => m::delete_record(name),
            Self::RestoreRecord { previous, .. } => {
                m::restore_record(&previous.name, &previous.kind, &previous.content)
            }
            Self::RecreateRecord { record, .. } => {
                m::recreate_record(&record.name, &record.kind, &record.content)
            }
            Self::StartConnector(_) => m::start_connector(),
            Self::DeleteLoginMethod(_) => m::delete_login_method(),
            Self::DeleteAccessApp {
                domain,
                bypass: true,
                ..
            } => m::delete_access_bypass(domain),
            Self::DeleteAccessApp { domain, .. } => m::delete_access_app(domain),
            Self::RestoreAccessApp { previous, .. } => m::restore_access_app(&previous.domain),
            Self::RecreateAccessApp(previous) if super::access::is_bypass(previous) => {
                m::recreate_access_bypass(&previous.domain)
            }
            Self::RecreateAccessApp(previous) => m::recreate_access_app(&previous.domain),
            Self::DeleteNetworkRoute { network, .. } => m::delete_network_route(network),
            Self::RecreateNetworkRoute(route) => m::recreate_network_route(&route.network),
            Self::DeleteLbMonitor(_) => m::delete_lb_monitor(),
            Self::DeleteLbPool(_) => m::delete_lb_pool(),
            Self::RestoreLbPool { .. } => m::restore_lb_pool(),
            Self::DeleteLoadBalancer { hostname, .. } => m::delete_load_balancer(hostname),
            Self::RecreateLoadBalancer { balancer, .. } => {
                m::recreate_load_balancer(&balancer.name)
            }
            Self::RecreateLbPool(pool) => m::recreate_lb_pool(&pool.name),
            Self::RecreateLbMonitor(_) => m::recreate_lb_monitor(),
            Self::DeleteSnapshotWorker { script, .. } => {
                msg::snapshot::leftover::delete_worker(script)
            }
            Self::RedeploySnapshot { script, .. } => msg::snapshot::leftover::redeploy(script),
            Self::SetWorkersDev { script, enabled } => {
                if *enabled {
                    msg::snapshot::leftover::workers_dev_off(script)
                } else {
                    msg::snapshot::leftover::workers_dev_on(script)
                }
            }
            Self::DetachSnapshotDomain { hostname, .. } => {
                msg::snapshot::leftover::detach_domain(hostname)
            }
            Self::ReattachSnapshotDomain(domain) => {
                msg::snapshot::leftover::reattach_domain(&domain.hostname)
            }
            Self::RecreateSnapshotWorker(script) => {
                msg::snapshot::leftover::recreate_worker(script)
            }
            Self::DeleteEdgeRule { hostnames, .. } => {
                msg::protection::leftover::delete_rule(hostnames)
            }
            Self::RestoreEdgeRule { hostnames, .. } => {
                msg::protection::leftover::restore_rule(hostnames)
            }
            Self::RecreateEdgeRule { hostnames, .. } => {
                msg::protection::leftover::recreate_rule(hostnames)
            }
            Self::DeleteServiceToken { name, .. } => msg::protection::leftover::delete_token(name),
            Self::RestoreServiceToken(name) => msg::protection::leftover::restore_token(name),
            Self::DeleteDatabase(id) => msg::front::leftover::delete_database(id),
            Self::DeleteFrontWorker { script, .. } => msg::front::leftover::delete_worker(script),
            Self::RestoreFrontWorker { script, .. } => msg::front::leftover::restore_worker(script),
            Self::DeleteWorkerRoute { pattern, .. } => msg::front::leftover::delete_route(pattern),
            Self::RecreateWorkerRoute { route, .. } => {
                msg::front::leftover::recreate_route(&route.pattern)
            }
        }
    }
}

fn route_id_from(comment: Option<&str>) -> String {
    comment
        .and_then(super::ownership::Ownership::parse)
        .and_then(|o| o.route_id().map(str::to_owned))
        .unwrap_or_default()
}

fn tunnel_cname(hostname: &str, tunnel_id: &str, comment: &str) -> NewDnsRecord {
    NewDnsRecord {
        name: hostname.to_owned(),
        kind: "CNAME".to_owned(),
        content: tunnel_target(tunnel_id),
        proxied: true,
        ttl: 1,
        comment: Some(comment.to_owned()),
    }
}

type Locks = HashMap<String, Arc<tokio::sync::Mutex<()>>>;

/// Plans and applies changes. One per app; cheap to share.
#[derive(Debug)]
pub struct Engine {
    local: Local,
    /// The keychain, for putting back a verifying inbox's signing secret when a plan
    /// that removed its Worker rolls back (never read otherwise).
    secrets: Option<crate::secrets::Secrets>,
    /// Who this is, for the DNS comments it writes (`person@machine`).
    owner: String,
    locks: Mutex<Locks>,
    cache: Mutex<HashMap<String, (Instant, Snapshot)>>,
}

impl Engine {
    /// An engine over the local store.
    pub fn new(local: Local) -> Self {
        Self {
            local,
            secrets: None,
            owner: super::ownership::owner_label(),
            locks: Mutex::default(),
            cache: Mutex::default(),
        }
    }

    /// The same engine writing `owner` (`person@machine`) into the DNS comments it makes
    /// instead of this user and machine.
    #[must_use]
    pub fn with_owner(mut self, owner: &str) -> Self {
        self.owner = super::ownership::sanitize_owner(owner);
        self
    }

    /// The same engine with the keychain, so a rollback restores a verifying inbox with
    /// its signing secret.
    #[must_use]
    pub fn with_secrets(mut self, secrets: crate::secrets::Secrets) -> Self {
        self.secrets = Some(secrets);
        self
    }

    /// Who this engine writes into DNS comments.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Who observes, now.
    pub fn who(&self) -> super::observe::Who<'_> {
        super::observe::Who {
            owner: &self.owner,
            now: crate::domain_shares::now_ms(),
        }
    }

    /// The local store.
    pub fn local(&self) -> &Local {
        &self.local
    }

    fn lock_for(&self, account: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.locks.lock().unwrap_or_else(PoisonError::into_inner);
        Arc::clone(locks.entry(account.to_owned()).or_default())
    }

    fn cache_key(account: &str, tunnel: Option<&str>, intent: &Intent) -> String {
        let scope = intent.hostnames().map_or_else(
            || "*".to_owned(),
            |names| {
                names
                    .iter()
                    .map(|h| h.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            },
        );
        // Everything the observation reads, so two intents share one only when it
        // would be the same.
        let need = ObserveNeed::of(intent);
        format!(
            "{account}\n{}\n{scope}\n{need:?}",
            tunnel.unwrap_or_default()
        )
    }

    /// Drops cached observations for `account` (after writes, or on "Refresh").
    pub fn invalidate(&self, account: &str) {
        let prefix = format!("{account}\n");
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|key, _| !key.starts_with(&prefix));
    }

    async fn snapshot<C: CloudApi>(
        &self,
        api: &C,
        ctx: Context<'_>,
        intent: &Intent,
        cached: bool,
    ) -> Result<Snapshot, EngineError> {
        let key = Self::cache_key(ctx.account, ctx.tunnel, intent);
        if cached {
            let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((at, snapshot)) = cache.get(&key)
                && at.elapsed() < CACHE_TTL
            {
                return Ok(snapshot.clone());
            }
        }
        let hostnames = intent.hostnames();
        let snapshot = observe(
            api,
            &self.local,
            ctx.account,
            ctx.tunnel,
            ctx.machine_name,
            hostnames.as_deref(),
            &ObserveNeed::of(intent),
            self.who(),
        )
        .await?;
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key, (Instant::now(), snapshot.clone()));
        Ok(snapshot)
    }

    /// Turns a change from the UI into an intent.
    ///
    /// # Errors
    /// Invalid input, observation errors, or nothing to restore.
    pub async fn intent_for<C: CloudApi>(
        &self,
        api: &C,
        ctx: Context<'_>,
        change: &Change,
    ) -> Result<Intent, EngineError> {
        match change {
            Change::RestoreConfig => {
                let drift = self
                    .drift(api, ctx.account, ctx.tunnel)
                    .await?
                    .ok_or(EngineError::NothingToRestore)?;
                Ok(Intent::RestoreConfig {
                    ingress: drift.ours,
                })
            }
            Change::UpdateRoute { .. } => {
                // Only the tunnel's routes are needed (to keep the edited route's options).
                let scope = Intent::RestoreConfig {
                    ingress: Vec::new(),
                };
                let snapshot = self.snapshot(api, ctx, &scope, true).await?;
                Ok(to_intent(change, &snapshot)?)
            }
            _ => Ok(to_intent(change, &EMPTY)?),
        }
    }

    /// This Mac's tunnels and routes in an account, with DNS and connector state:
    /// private networks are those of the default tunnel. May reuse observations up to
    /// 5 s old.
    ///
    /// # Errors
    /// Observation errors.
    pub async fn overview<C: CloudApi, K: Connectors>(
        &self,
        api: &C,
        connectors: &K,
        ctx: Context<'_>,
    ) -> Result<RoutesOverview, EngineError> {
        let state = |id: &str| connectors.state(id);
        let base = Context {
            tunnel: None,
            ..ctx
        };
        let snapshot = self
            .snapshot(api, base, &Intent::RemoveTunnel, true)
            .await?;
        let mut merged = overview(&snapshot, true, state);
        let others = self
            .local
            .tunnels(ctx.account)
            .await
            .map_err(ObserveError::from)?;
        for tunnel in others.iter().filter(|t| !t.is_default) {
            let ctx = Context {
                tunnel: Some(&tunnel.tunnel_id),
                ..ctx
            };
            // Every routed hostname's DNS and login, like the default tunnel's.
            let snapshot = self.snapshot(api, ctx, &Intent::RemoveTunnel, true).await?;
            let mut part = overview(&snapshot, false, state);
            // A tunnel deleted elsewhere still shows (by its local name) until removed.
            if part.tunnels.is_empty() {
                part.tunnels.push(super::views::TunnelView {
                    id: tunnel.tunnel_id.clone(),
                    name: tunnel.name.clone(),
                    connector: None,
                    is_default: false,
                });
            }
            merged.tunnels.append(&mut part.tunnels);
            merged.routes.append(&mut part.routes);
        }
        merged.tunnels[usize::from(merged.tunnel.is_some())..]
            .sort_by_key(|t| t.name.to_lowercase());
        merged
            .routes
            .sort_by(|a, b| (&a.zone, &a.hostname, &a.path).cmp(&(&b.zone, &b.hostname, &b.path)));
        let shares = self
            .local
            .shares(Some(ctx.account))
            .await
            .map_err(ObserveError::from)?;
        let balanced = self
            .local
            .balanced(ctx.account)
            .await
            .map_err(ObserveError::from)?;
        let paused: std::collections::HashSet<String> =
            crate::pause::list(self.local.store(), Some(ctx.account))
                .await
                .map_err(ObserveError::from)?
                .into_iter()
                .map(|p| p.hostname.to_ascii_lowercase())
                .collect();
        for route in &mut merged.routes {
            route.balanced = balanced.contains(&route.hostname.to_ascii_lowercase());
            route.paused = paused.contains(&route.hostname.to_ascii_lowercase());
            route.temporary = route.path.is_none()
                && shares
                    .iter()
                    .any(|s| s.hostname.eq_ignore_ascii_case(&route.hostname));
        }
        Ok(merged)
    }

    /// What an export of this Mac's tunnel is made from: its routes and the proxied
    /// CNAMEs that point at it. `None` if this Mac has no tunnel in the account.
    ///
    /// # Errors
    /// Observation errors.
    pub async fn export_input<C: CloudApi>(
        &self,
        api: &C,
        ctx: Context<'_>,
        cloudflared_version: Option<String>,
    ) -> Result<Option<crate::export::ExportInput>, EngineError> {
        let snapshot = self.snapshot(api, ctx, &Intent::RemoveTunnel, true).await?;
        let Some(tunnel) = snapshot.tunnel else {
            return Ok(None);
        };
        let target = super::types::tunnel_target(&tunnel.id);
        let routed: std::collections::HashSet<&str> = tunnel
            .ingress
            .iter()
            .filter_map(|r| r.hostname.as_deref())
            .collect();
        let records = snapshot
            .records
            .iter()
            .filter(|r| {
                r.record.kind == "CNAME"
                    && r.record.content.eq_ignore_ascii_case(&target)
                    && routed.contains(r.record.name.as_str())
            })
            .map(|r| crate::export::ExportRecord {
                zone_id: r.zone_id.clone(),
                record_id: r.record.id.clone(),
                hostname: r.record.name.clone(),
                comment: r.record.comment.clone(),
                ttl: r.record.ttl,
                proxied: r.record.proxied,
            })
            .collect();
        Ok(Some(crate::export::ExportInput {
            account_id: snapshot.account_id,
            tunnel_id: tunnel.id,
            tunnel_name: tunnel.name,
            ingress: tunnel.ingress,
            records,
            cloudflared_version,
        }))
    }

    /// Every tunnel in the account, this Mac's first.
    ///
    /// # Errors
    /// API or database errors.
    pub async fn tunnels<C: CloudApi, K: Connectors>(
        &self,
        api: &C,
        connectors: &K,
        account: &str,
    ) -> Result<Vec<TunnelSummary>, EngineError> {
        Ok(super::tunnels::list(api, connectors, &self.local, account).await?)
    }

    /// Runs an existing tunnel of the account on this Mac too: it becomes one of this
    /// Mac's tunnels (not the default), and its routes stay as they are in Cloudflare.
    /// Nothing changes in Cloudflare. If another machine runs it, both serve it: requests
    /// are split between them (D-058), so only adopt a tunnel whose services this Mac has.
    ///
    /// # Errors
    /// [`EngineError::Adopt`] if the tunnel doesn't exist, is already this Mac's, or is
    /// configured locally (its routes live in a config file Teitunnel can't manage).
    pub async fn adopt<C: CloudApi>(
        &self,
        api: &C,
        account: &str,
        tunnel_id: &str,
    ) -> Result<(), EngineError> {
        let ours = self
            .local
            .tunnels(account)
            .await
            .map_err(ObserveError::from)?;
        if ours.iter().any(|t| t.tunnel_id == tunnel_id) {
            return Err(EngineError::Adopt(msg::error::adopt::already_here()));
        }
        let tunnel = api
            .tunnel(account, tunnel_id)
            .await
            .map_err(ObserveError::from)?
            .ok_or_else(|| EngineError::Adopt(msg::error::adopt::gone()))?;
        if !tunnel.remote_config {
            return Err(EngineError::Adopt(msg::error::adopt::locally_configured(
                &tunnel.name,
            )));
        }
        self.local
            .add_tunnel(account, &tunnel.id, &tunnel.name)
            .await
            .map_err(ObserveError::from)?;
        self.invalidate(account);
        Ok(())
    }

    /// What planning `intent` would be based on (for views of the current state). May
    /// reuse an observation up to 5 s old.
    ///
    /// # Errors
    /// Observation errors.
    pub async fn observation<C: CloudApi>(
        &self,
        api: &C,
        ctx: Context<'_>,
        intent: &Intent,
    ) -> Result<Snapshot, EngineError> {
        self.snapshot(api, ctx, intent, true).await
    }

    /// Plans `intent` for review. May reuse an observation up to 5 s old.
    ///
    /// # Errors
    /// Observation or planning errors.
    pub async fn preview<C: CloudApi>(
        &self,
        api: &C,
        ctx: Context<'_>,
        intent: &Intent,
    ) -> Result<Plan, EngineError> {
        let snapshot = self.snapshot(api, ctx, intent, true).await?;
        Ok(plan(intent, &snapshot)?)
    }

    /// Whether this Mac's tunnel config was edited outside Teitunnel since it last wrote
    /// it. Edits that don't change any route (e.g. the catch-all) are adopted silently.
    ///
    /// # Errors
    /// API or database errors.
    pub async fn drift<C: CloudApi>(
        &self,
        api: &C,
        account: &str,
        tunnel: Option<&str>,
    ) -> Result<Option<Drift>, EngineError> {
        let Some(tunnel) = self
            .local
            .tunnel(account, tunnel)
            .await
            .map_err(ObserveError::from)?
        else {
            return Ok(None);
        };
        let Some(applied) = tunnel.last_applied_version else {
            return Ok(None);
        };
        let current = match api.tunnel_config(account, &tunnel.tunnel_id).await {
            Ok(current) => current,
            Err(err) if err.status() == Some(404) => return Ok(None),
            Err(err) => return Err(ObserveError::from(err).into()),
        };
        if current.version <= applied {
            return Ok(None);
        }
        let ours = self
            .local
            .applied_ingress(&tunnel.tunnel_id)
            .await
            .map_err(ObserveError::from)?
            .unwrap_or_default();
        let theirs = current.config.map(|c| c.ingress).unwrap_or_default();
        let changes = diff(&ours, &theirs);
        if changes.is_empty() {
            self.local
                .set_applied(&tunnel.tunnel_id, current.version, &theirs)
                .await
                .map_err(ObserveError::from)?;
            return Ok(None);
        }
        Ok(Some(Drift {
            tunnel_id: tunnel.tunnel_id,
            applied_version: applied,
            current_version: current.version,
            changes,
            ours,
            theirs,
        }))
    }

    /// The first outside edit found on any of this Mac's tunnels in `account`.
    ///
    /// # Errors
    /// API or database errors.
    pub async fn any_drift<C: CloudApi>(
        &self,
        api: &C,
        account: &str,
    ) -> Result<Option<Drift>, EngineError> {
        let tunnels = self
            .local
            .tunnels(account)
            .await
            .map_err(ObserveError::from)?;
        for tunnel in tunnels {
            if let Some(drift) = self.drift(api, account, Some(&tunnel.tunnel_id)).await? {
                return Ok(Some(drift));
            }
        }
        Ok(None)
    }

    /// Adopts an outside edit as the new baseline ("Keep theirs").
    ///
    /// # Errors
    /// Database errors.
    pub async fn keep_theirs(&self, account: &str, drift: &Drift) -> Result<(), EngineError> {
        let lock = self.lock_for(account);
        let _guard = lock.lock().await;
        self.local
            .set_applied(&drift.tunnel_id, drift.current_version, &drift.theirs)
            .await
            .map_err(ObserveError::from)?;
        self.invalidate(account);
        Ok(())
    }

    /// Checks that `hostname` works end to end. Transient failures (a connector still
    /// connecting, propagation) are retried for up to `patience`.
    ///
    /// # Errors
    /// Observation errors.
    pub async fn verify<C: CloudApi>(
        &self,
        api: &C,
        ctx: Context<'_>,
        hostname: &Hostname,
        edge: Edge,
        patience: Duration,
    ) -> Result<Verification, EngineError> {
        let carrying = match ctx.tunnel {
            Some(id) => Some(id.to_owned()),
            None => self
                .local
                .tunnel_routing(ctx.account, hostname.as_str())
                .await
                .map_err(ObserveError::from)?,
        };
        let snapshot = observe(
            api,
            &self.local,
            ctx.account,
            carrying.as_deref(),
            ctx.machine_name,
            Some(&[hostname]),
            &ObserveNeed::none(),
            self.who(),
        )
        .await?;
        if let Some(failure) = check_dns(&snapshot, hostname.as_str()) {
            return Ok(Verification::new(hostname.to_string(), None, Some(failure)));
        }
        let route = snapshot
            .routes()
            .into_iter()
            .find(|r| r.hostname.as_deref() == Some(hostname.as_str()));
        let origin = route.and_then(|r| RouteOrigin::parse(&r.service).ok());
        let sends_host = route.is_some_and(|r| r.origin_request.contains_key("httpHostHeader"));
        let deadline = Instant::now() + patience;
        loop {
            let mut result = probe(edge, hostname, origin.as_ref()).await;
            // Sending the Host header again is no fix when the route already does.
            if sends_host && let Some(Failure::HostRejected { rejection }) = &mut result.failure {
                rejection.host_header = None;
            }
            let transient = result.failure.as_ref().is_some_and(Failure::is_transient);
            if !transient || Instant::now() + VERIFY_RETRY > deadline {
                return Ok(result);
            }
            tokio::time::sleep(VERIFY_RETRY).await;
        }
    }

    /// Applies `intent`, which the user reviewed and approved.
    /// Re-observes first: if anything changed, returns [`EngineError::Stale`] with the new
    /// plan instead of applying. Changes to one account are serialised.
    ///
    /// # Errors
    /// Errors before anything was changed. Failures while applying are reported in the
    /// [`Outcome`].
    pub async fn apply<C, K, P>(
        &self,
        api: &C,
        connectors: &K,
        ctx: Context<'_>,
        intent: &Intent,
        approval: Approval<'_>,
        progress: P,
    ) -> Result<Outcome, EngineError>
    where
        C: CloudApi,
        K: Connectors,
        P: FnMut(Progress) + Send,
    {
        self.apply_issuing(api, connectors, ctx, intent, approval, progress)
            .await
            .map(|(outcome, _)| outcome)
    }

    /// [`Self::apply`], also returning the credentials of service tokens the change
    /// created or rotated (only when it applied: a rolled-back token is deleted again).
    /// This is the only time their secrets exist outside Cloudflare.
    ///
    /// # Errors
    /// See [`Self::apply`].
    pub async fn apply_issuing<C, K, P>(
        &self,
        api: &C,
        connectors: &K,
        ctx: Context<'_>,
        intent: &Intent,
        approval: Approval<'_>,
        mut progress: P,
    ) -> Result<(Outcome, Vec<super::edge::IssuedToken>), EngineError>
    where
        C: CloudApi,
        K: Connectors,
        P: FnMut(Progress) + Send,
    {
        let lock = self.lock_for(ctx.account);
        let _guard = lock.lock().await;
        let snapshot = self.snapshot(api, ctx, intent, false).await?;
        let plan = plan(intent, &snapshot)?;
        if plan.fingerprint != approval.fingerprint {
            return Err(EngineError::Stale(Box::new(plan)));
        }
        if plan.requires_confirmation && !approval.confirmed {
            return Err(EngineError::NeedsConfirmation);
        }

        let tunnel_name = snapshot
            .tunnel
            .as_ref()
            .map_or(ctx.machine_name, |t| t.name.as_str())
            .to_owned();
        let mut run = Run {
            api,
            connectors,
            local: &self.local,
            secrets: self.secrets.as_ref(),
            snapshot: &snapshot,
            account: ctx.account,
            slot: match (intent, ctx.tunnel) {
                (Intent::CreateTunnel { .. }, _) => Slot::Additional,
                (_, Some(id)) => Slot::Replaces(id),
                (_, None) => Slot::Default,
            },
            created: None,
            created_monitor: None,
            created_pool: None,
            renamed: std::sync::Mutex::default(),
            done: Vec::new(),
            covered: Vec::new(),
            uploaded: None,
            created_token: None,
            created_rulesets: HashMap::new(),
            issued: Vec::new(),
            created_database: None,
        };
        let serve = matches!(
            intent,
            Intent::AddRoute { .. } | Intent::UpdateRoute { .. } | Intent::AddNetwork { .. }
        );
        // Keep each step's last state for the activity log.
        let mut states: Vec<Option<StepState>> = vec![None; plan.steps.len()];
        let mut record_progress = |p: Progress| {
            if let Some(slot) = usize::try_from(p.step).ok().and_then(|i| states.get_mut(i)) {
                *slot = Some(p.state.clone());
            }
            progress(p);
        };
        let outcome = run.execute(&plan, serve, &mut record_progress).await;
        self.invalidate(ctx.account);
        let mut record = ActivityRecord::new(intent, &plan, ctx.account, &states);
        match &outcome {
            Outcome::Applied {
                connector_error, ..
            } => record.connector_error.clone_from(connector_error),
            Outcome::RolledBack { error, .. } => record.error = Some(error.clone()),
            Outcome::PartiallyApplied {
                error, leftovers, ..
            } => {
                record.error = Some(error.clone());
                record.leftovers.clone_from(leftovers);
            }
        }
        // Plain English lines too: search, and apps from before `record` had them.
        let mut detail: Vec<String> = plan
            .steps
            .iter()
            .filter(|s| s.is_mutation())
            .map(|s| s.describe(&tunnel_name).english())
            .collect();
        match &outcome {
            Outcome::Applied {
                connector_error: Some(error),
                ..
            } => detail.push(format!("Connector: {}", error.english())),
            Outcome::RolledBack { error, .. } => {
                detail.push(format!("Failed: {}", error.english()));
            }
            Outcome::PartiallyApplied {
                error, leftovers, ..
            } => {
                detail.push(format!("Failed: {}", error.english()));
                detail.extend(
                    leftovers
                        .iter()
                        .map(|l| format!("Left over: {}", l.english())),
                );
            }
            Outcome::Applied { .. } => {}
        }
        // A hostname with no route left keeps no pause or schedule (they'd come back if
        // the name were used again).
        if matches!(outcome, Outcome::Applied { .. })
            && matches!(intent, Intent::RemoveRoute { .. } | Intent::RemoveTunnel)
        {
            for hostname in &record.hostnames {
                if let Ok(None) = self.local.tunnel_routing(ctx.account, hostname).await {
                    let store = self.local.store();
                    warn_local(crate::pause::forget(store, ctx.account, hostname).await);
                    warn_local(crate::schedule::set(store, ctx.account, hostname, None).await);
                }
            }
        }
        // The app log says what changed too, so a report ("the record wasn't removed")
        // can be traced without the database.
        tracing::info!(
            account = ctx.account,
            outcome = outcome.label(),
            steps = %detail.join("; "),
            "{}",
            intent.summary().english()
        );
        if let Err(err) = self
            .local
            .log(
                ctx.account,
                &intent.summary().english(),
                outcome.label(),
                &detail,
                Some(&record),
            )
            .await
        {
            tracing::warn!(%err, "couldn't write the activity log");
        }
        let issued = if matches!(outcome, Outcome::Applied { .. }) {
            std::mem::take(&mut run.issued)
        } else {
            Vec::new()
        };
        Ok((outcome, issued))
    }
}

struct Run<'a, C, K> {
    api: &'a C,
    connectors: &'a K,
    local: &'a Local,
    secrets: Option<&'a crate::secrets::Secrets>,
    snapshot: &'a Snapshot,
    account: &'a str,
    /// How a tunnel this run creates is remembered.
    slot: Slot<'a>,
    created: Option<String>,
    /// The monitor and pool this run created (for steps referring to them).
    created_monitor: Option<String>,
    created_pool: Option<String>,
    /// Load-balancing objects recreated while rolling back get new ids: old → new, so
    /// what refers to them is recreated pointing at the new ones.
    renamed: std::sync::Mutex<std::collections::HashMap<String, String>>,
    done: Vec<(u32, Undo)>,
    /// Completed steps with no undo of their own: another step's undo reverses them
    /// (e.g. a new tunnel's config goes with the tunnel).
    covered: Vec<u32>,
    /// A Snapshot's files, once uploaded: the completion token and what was sent.
    uploaded: Option<(String, super::sites::SiteContent)>,
    /// The service token this run created (for steps referring to it).
    created_token: Option<String>,
    /// Entry point rulesets this run created, by zone and phase.
    created_rulesets: HashMap<(String, String), String>,
    /// Credentials of tokens created or rotated, handed to the caller once.
    issued: Vec<super::edge::IssuedToken>,
    /// The D1 database this run created (for Workers binding to it).
    created_database: Option<String>,
}

/// What a tunnel created while applying is to this Mac.
#[derive(Debug, Clone, Copy)]
enum Slot<'a> {
    /// The default (machine) tunnel.
    Default,
    /// An additional tunnel (`Intent::CreateTunnel`).
    Additional,
    /// A replacement for this tunnel of this Mac's, which was deleted elsewhere.
    Replaces(&'a str),
}

fn warn_local<T>(result: Result<T, crate::store::StoreError>) {
    if let Err(err) = result {
        tracing::warn!(%err, "couldn't update the local ownership index");
    }
}

impl<C: CloudApi, K: Connectors> Run<'_, C, K> {
    fn resolve(&self, tunnel: &TunnelRef) -> Result<String, Text> {
        match tunnel {
            TunnelRef::Existing(id) => Ok(id.clone()),
            TunnelRef::Created => self
                .created
                .clone()
                .ok_or_else(msg::apply::tunnel_not_created),
        }
    }

    fn resolve_lb(
        reference: &super::types::LbRef,
        created: Option<&String>,
    ) -> Result<String, Text> {
        match reference {
            super::types::LbRef::Existing(id) => Ok(id.clone()),
            super::types::LbRef::Created => created.cloned().ok_or_else(msg::apply::lb_not_created),
        }
    }

    /// The pool for `hostname` sending traffic to `endpoints` (tunnels resolved).
    fn pool_for(
        &self,
        hostname: &str,
        monitor: &super::types::LbRef,
        endpoints: &[super::types::PoolEndpoint],
    ) -> Result<cf_api::Pool, Text> {
        use super::balance;
        let origins = endpoints
            .iter()
            .map(|e| {
                Ok(balance::origin_for(
                    hostname,
                    &self.resolve(&e.tunnel)?,
                    &e.name,
                ))
            })
            .collect::<Result<Vec<_>, Text>>()?;
        Ok(cf_api::Pool {
            id: String::new(),
            name: balance::pool_name(hostname),
            description: balance::marker(hostname),
            enabled: true,
            monitor: Some(Self::resolve_lb(monitor, self.created_monitor.as_ref())?),
            origins,
        })
    }

    fn resolve_database(&self, database: &super::front::DatabaseRef) -> Result<String, Text> {
        match database {
            super::front::DatabaseRef::Existing(id) => Ok(id.clone()),
            super::front::DatabaseRef::Created => self
                .created_database
                .clone()
                .ok_or_else(msg::front::error::no_database),
        }
    }

    fn site_database(&self, settings: &super::sites::SiteSettings) -> Result<Option<String>, Text> {
        settings
            .comments
            .as_ref()
            .map(|c| self.resolve_database(&c.database))
            .transpose()
    }

    /// Uploads a front Worker with `config` and records it in the local index.
    async fn put_front(
        &self,
        hostname: &str,
        zone_id: &str,
        script: &str,
        config: &super::front::FrontConfig,
        database: Option<&str>,
        secret: Option<&crate::Secret<String>>,
    ) -> Result<(), Text> {
        let metadata = super::front::metadata(script, config, database, secret);
        self.api
            .put_worker_script(
                self.account,
                script,
                &metadata,
                &super::front::modules(config.kind()),
            )
            .await
            .map_err(|e| e.text())?;
        warn_local(
            self.local
                .save_front(self.account, hostname, zone_id, script, config)
                .await,
        );
        Ok(())
    }

    fn renamed(&self, id: &str) -> String {
        self.renamed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned())
    }

    /// The comment for a route's record: the route and this owner, keeping the lease the
    /// record held for this owner.
    fn comment(&self, route_id: &str, previous: Option<&str>) -> String {
        super::ownership::route_comment(route_id, &self.snapshot.owner, previous, self.snapshot.now)
    }

    fn was_owned(&self, record_id: &str) -> bool {
        self.snapshot
            .records
            .iter()
            .any(|r| r.record.id == record_id && r.owned)
    }

    /// Runs the steps; with `serve`, also makes sure this Mac's connector is running.
    async fn execute(
        &mut self,
        plan: &Plan,
        serve: bool,
        progress: &mut impl FnMut(Progress),
    ) -> Outcome {
        let mut verify = Vec::new();
        let mut index = 0u32;
        let mut deleted_tunnel = false;
        for step in &plan.steps {
            if let Step::Verify { hostname } = step {
                verify.push(hostname.clone());
                progress(Progress {
                    step: index,
                    state: StepState::Skipped,
                });
                index += 1;
                continue;
            }
            deleted_tunnel |= matches!(step, Step::DeleteTunnel { .. });
            progress(Progress {
                step: index,
                state: StepState::Running,
            });
            let result = match step {
                Step::UploadSnapshotFiles {
                    script, content, ..
                } => {
                    let step_index = index;
                    super::sites::upload(self.api, self.account, script, content, |t| {
                        progress(Progress {
                            step: step_index,
                            state: StepState::Transferring {
                                files: t.files,
                                total_files: t.total_files,
                                bytes: t.bytes,
                                total_bytes: t.total_bytes,
                            },
                        });
                    })
                    .await
                    .map(|jwt| {
                        self.uploaded = Some((jwt, content.clone()));
                        None
                    })
                }
                _ => self.step(step).await,
            };
            match result {
                Ok(undo) => {
                    match undo {
                        Some(undo) => self.done.push((index, undo)),
                        None => self.covered.push(index),
                    }
                    progress(Progress {
                        step: index,
                        state: StepState::Done,
                    });
                }
                Err(message) => {
                    progress(Progress {
                        step: index,
                        state: StepState::Failed {
                            message: message.clone(),
                        },
                    });
                    return self.roll_back(index, message, progress).await;
                }
            }
            index += 1;
        }

        let tunnel_id = if deleted_tunnel {
            None
        } else {
            self.created
                .clone()
                .or_else(|| self.snapshot.tunnel.as_ref().map(|t| t.id.clone()))
        };
        let connector_error = match (&tunnel_id, serve) {
            (Some(id), true) => self.ensure_connector(id).await.err(),
            _ => None,
        };
        Outcome::Applied {
            tunnel_id,
            verify,
            connector_error,
        }
    }

    async fn step(&mut self, step: &Step) -> Result<Option<Undo>, Text> {
        let (api, account) = (self.api, self.account);
        match step {
            Step::CreateTunnel { name } => {
                let tunnel = api
                    .create_tunnel(account, name)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(match self.slot {
                    Slot::Default => {
                        self.local
                            .set_machine_tunnel(account, &tunnel.id, name)
                            .await
                    }
                    Slot::Additional => self.local.add_tunnel(account, &tunnel.id, name).await,
                    Slot::Replaces(old) => self.local.replace_tunnel(old, &tunnel.id).await,
                });
                self.created = Some(tunnel.id.clone());
                Ok(Some(Undo::DeleteTunnel(tunnel.id)))
            }
            Step::PutConfig {
                tunnel,
                ingress,
                expected_version,
                previous,
            } => {
                let id = self.resolve(tunnel)?;
                self.put_ingress(&id, ingress, *expected_version).await?;
                Ok((!previous.is_empty()).then(|| Undo::RestoreConfig {
                    tunnel: id,
                    previous: previous.clone(),
                }))
            }
            Step::CreateRecord {
                zone_id,
                hostname,
                tunnel,
                route_id,
            } => {
                let target = self.resolve(tunnel)?;
                let record = api
                    .create_record(
                        zone_id,
                        &tunnel_cname(hostname, &target, &self.comment(route_id, None)),
                    )
                    .await
                    .map_err(|e| e.text())?;
                warn_local(
                    self.local
                        .own_record(account, zone_id, &record.id, hostname, route_id)
                        .await,
                );
                Ok(Some(Undo::DeleteRecord {
                    zone: zone_id.clone(),
                    id: record.id,
                    name: hostname.clone(),
                }))
            }
            Step::UpdateRecord {
                zone_id,
                record_id,
                hostname,
                tunnel,
                route_id,
                previous,
            } => {
                let target = self.resolve(tunnel)?;
                let cname = tunnel_cname(
                    hostname,
                    &target,
                    &self.comment(route_id, previous.comment.as_deref()),
                );
                let was_owned = self.was_owned(record_id);
                let current = if previous.kind == cname.kind {
                    api.update_record(zone_id, record_id, &cname)
                        .await
                        .map_err(|e| e.text())?
                } else {
                    let replaced = api
                        .replace_record(zone_id, record_id, &cname)
                        .await
                        .map_err(|e| e.text())?;
                    warn_local(self.local.disown_record(record_id).await);
                    replaced
                };
                warn_local(
                    self.local
                        .own_record(account, zone_id, &current.id, hostname, route_id)
                        .await,
                );
                Ok(Some(Undo::RestoreRecord {
                    zone: zone_id.clone(),
                    previous: previous.clone(),
                    current: current.id,
                    was_owned,
                }))
            }
            Step::DeleteRecord { zone_id, record } => {
                api.delete_record(zone_id, &record.id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(self.local.disown_record(&record.id).await);
                Ok(Some(Undo::RecreateRecord {
                    zone: zone_id.clone(),
                    record: record.clone(),
                    was_owned: self.was_owned(&record.id),
                }))
            }
            Step::CreateReservation {
                zone_id,
                hostname,
                until,
            } => {
                use super::ownership::{LEASE_ADDRESS, LEASE_KIND, Ownership};
                let placeholder = NewDnsRecord {
                    name: hostname.clone(),
                    kind: LEASE_KIND.to_owned(),
                    content: LEASE_ADDRESS.to_owned(),
                    proxied: true,
                    ttl: 1,
                    comment: Some(Ownership::lease(&self.snapshot.owner, *until).render()),
                };
                let record = api
                    .create_record(zone_id, &placeholder)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::DeleteRecord {
                    zone: zone_id.clone(),
                    id: record.id,
                    name: hostname.clone(),
                }))
            }
            Step::SetLease {
                zone_id,
                record,
                lease,
                until,
            } => {
                use super::ownership::{Marker, Ownership};
                let mut ownership = record
                    .comment
                    .as_deref()
                    .and_then(Ownership::parse)
                    .unwrap_or(Ownership {
                        marker: Marker::Lease,
                        owner: None,
                        lease: true,
                        until: None,
                    });
                if ownership.owner.is_none() {
                    ownership.owner = Some(self.snapshot.owner.clone());
                }
                ownership.lease = *lease || ownership.marker == Marker::Lease;
                ownership.until = if *lease { *until } else { None };
                let changed = NewDnsRecord {
                    comment: Some(ownership.render()),
                    ..record.to_new()
                };
                api.update_record(zone_id, &record.id, &changed)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::RestoreRecord {
                    zone: zone_id.clone(),
                    previous: record.clone(),
                    current: record.id.clone(),
                    was_owned: self.was_owned(&record.id),
                }))
            }
            Step::StopConnector { tunnel_id } => {
                self.connectors.stop(tunnel_id).await?;
                Ok(Some(Undo::StartConnector(tunnel_id.clone())))
            }
            Step::DeleteTunnel { tunnel_id } => {
                api.delete_tunnel(account, tunnel_id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(self.local.forget_tunnel(tunnel_id).await);
                self.connectors.deleted(tunnel_id).await;
                Ok(None)
            }
            Step::AddLoginMethod => {
                let id = api
                    .create_one_time_pin(account)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::DeleteLoginMethod(id)))
            }
            Step::CreateAccessApp { app } => {
                let created = api
                    .create_access_app(account, app)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(
                    self.local
                        .own_access_app(account, &created.id, &app.domain)
                        .await,
                );
                Ok(Some(Undo::DeleteAccessApp {
                    id: created.id,
                    domain: app.domain.clone(),
                    bypass: super::access::is_bypass(app),
                }))
            }
            Step::UpdateAccessApp { id, app, previous } => {
                api.update_access_app(account, id, app)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(self.local.own_access_app(account, id, &app.domain).await);
                Ok(Some(Undo::RestoreAccessApp {
                    id: id.clone(),
                    previous: previous.clone(),
                }))
            }
            Step::DeleteAccessApp { id, previous } => {
                api.delete_access_app(account, id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(self.local.disown_access_app(id).await);
                Ok(Some(Undo::RecreateAccessApp(previous.clone())))
            }
            Step::CreateNetworkRoute { network, tunnel } => {
                let target = self.resolve(tunnel)?;
                let network = network.to_string();
                let route = api
                    .create_network_route(account, &network, &target, NETWORK_COMMENT, None)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::DeleteNetworkRoute {
                    id: route.id,
                    network,
                }))
            }
            Step::DeleteNetworkRoute { route } => {
                api.delete_network_route(account, &route.id)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::RecreateNetworkRoute(route.clone())))
            }
            Step::CreateLbMonitor { hostname } => {
                let monitor = api
                    .create_lb_monitor(account, &super::balance::monitor_for(hostname))
                    .await
                    .map_err(|e| e.text())?;
                self.created_monitor = Some(monitor.id.clone());
                Ok(Some(Undo::DeleteLbMonitor(monitor.id)))
            }
            Step::CreateLbPool {
                hostname,
                monitor,
                endpoints,
            } => {
                let pool = self.pool_for(hostname, monitor, endpoints)?;
                let created = api
                    .create_lb_pool(account, &pool)
                    .await
                    .map_err(|e| e.text())?;
                self.created_pool = Some(created.id.clone());
                Ok(Some(Undo::DeleteLbPool(created.id)))
            }
            Step::UpdateLbPool {
                hostname,
                id,
                monitor,
                endpoints,
                previous,
            } => {
                let pool = self.pool_for(hostname, monitor, endpoints)?;
                api.update_lb_pool(account, id, &pool)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::RestoreLbPool {
                    id: id.clone(),
                    previous: previous.clone(),
                }))
            }
            Step::CreateLoadBalancer {
                zone_id,
                hostname,
                pool,
            } => {
                let pool = Self::resolve_lb(pool, self.created_pool.as_ref())?;
                let balancer = api
                    .create_load_balancer(
                        zone_id,
                        &cf_api::LoadBalancer {
                            id: String::new(),
                            name: hostname.clone(),
                            description: super::balance::marker(hostname),
                            default_pools: vec![pool.clone()],
                            fallback_pool: pool,
                            proxied: true,
                        },
                    )
                    .await
                    .map_err(|e| e.text())?;
                warn_local(self.local.set_balanced(account, hostname, true).await);
                Ok(Some(Undo::DeleteLoadBalancer {
                    zone: zone_id.clone(),
                    id: balancer.id,
                    hostname: hostname.clone(),
                }))
            }
            Step::DeleteLoadBalancer { zone_id, balancer } => {
                api.delete_load_balancer(zone_id, &balancer.id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(
                    self.local
                        .set_balanced(account, &balancer.name, false)
                        .await,
                );
                Ok(Some(Undo::RecreateLoadBalancer {
                    zone: zone_id.clone(),
                    balancer: balancer.clone(),
                }))
            }
            Step::DeleteLbPool { pool } => {
                api.delete_lb_pool(account, &pool.id)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::RecreateLbPool(pool.clone())))
            }
            Step::DeleteLbMonitor { monitor } => {
                api.delete_lb_monitor(account, &monitor.id)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::RecreateLbMonitor(monitor.clone())))
            }
            Step::Verify { .. } | Step::UploadSnapshotFiles { .. } => Ok(None),
            Step::CreateSnapshotWorker {
                snapshot,
                script,
                settings,
            } => {
                let (jwt, content) = self
                    .uploaded
                    .clone()
                    .ok_or_else(msg::snapshot::error::not_uploaded)?;
                let database = self.site_database(settings)?;
                let metadata = super::sites::metadata(
                    settings,
                    &content,
                    &jwt,
                    "Teitunnel Snapshot",
                    script,
                    database.as_deref(),
                );
                api.put_worker_script(account, script, &metadata, &super::sites::modules())
                    .await
                    .map_err(|e| e.text())?;
                let version = api
                    .worker_deployments(account, script)
                    .await
                    .ok()
                    .flatten()
                    .and_then(|d| d.first().and_then(|d| d.main_version().map(str::to_owned)));
                if let Some(version) = &version {
                    warn_local(
                        self.local
                            .record_site_version(
                                snapshot,
                                version,
                                &content,
                                settings.spa,
                                settings.password != super::sites::Password::Off,
                            )
                            .await,
                    );
                }
                Ok(Some(Undo::DeleteSnapshotWorker {
                    snapshot: snapshot.clone(),
                    script: script.clone(),
                }))
            }
            Step::PublishSnapshotVersion {
                snapshot,
                script,
                settings,
                previous,
            } => {
                let (jwt, content) = self
                    .uploaded
                    .clone()
                    .ok_or_else(msg::snapshot::error::not_uploaded)?;
                let database = self.site_database(settings)?;
                let metadata = super::sites::metadata(
                    settings,
                    &content,
                    &jwt,
                    "Teitunnel Snapshot",
                    script,
                    database.as_deref(),
                );
                let version = api
                    .upload_worker_version(account, script, &metadata, &super::sites::modules())
                    .await
                    .map_err(|e| e.text())?;
                api.deploy_worker_version(account, script, &version.id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(
                    self.local
                        .record_site_version(
                            snapshot,
                            &version.id,
                            &content,
                            settings.spa,
                            settings.password != super::sites::Password::Off,
                        )
                        .await
                        .map(|_| ()),
                );
                Ok(previous.as_ref().map(|previous| Undo::RedeploySnapshot {
                    snapshot: snapshot.clone(),
                    script: script.clone(),
                    version: previous.clone(),
                    replaced: Some(version.id),
                }))
            }
            Step::RollBackSnapshot {
                snapshot,
                script,
                version_id,
                previous,
                ..
            } => {
                api.deploy_worker_version(account, script, version_id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(self.local.set_site_live(snapshot, version_id).await);
                Ok(previous.as_ref().map(|previous| Undo::RedeploySnapshot {
                    snapshot: snapshot.clone(),
                    script: script.clone(),
                    version: previous.clone(),
                    replaced: None,
                }))
            }
            Step::EnableWorkersDev { script, .. } | Step::DisableWorkersDev { script, .. } => {
                let enabled = matches!(step, Step::EnableWorkersDev { .. });
                api.set_worker_on_workers_dev(account, script, enabled)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::SetWorkersDev {
                    script: script.clone(),
                    enabled: !enabled,
                }))
            }
            Step::AttachSnapshotDomain {
                zone_id,
                hostname,
                script,
            } => {
                let domain = api
                    .attach_worker_domain(account, hostname, zone_id, script)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::DetachSnapshotDomain {
                    id: domain.id,
                    hostname: hostname.clone(),
                }))
            }
            Step::DetachSnapshotDomain { domain } => {
                api.detach_worker_domain(account, &domain.id)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::ReattachSnapshotDomain(domain.clone())))
            }
            Step::DeleteSnapshotWorker { script, .. } => {
                api.delete_worker_script(account, script)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::RecreateSnapshotWorker(script.clone())))
            }
            Step::CreateEdgeRule {
                zone_id,
                phase,
                ruleset_id,
                kind,
                hostnames,
                rule,
            } => {
                let key = (zone_id.clone(), phase.clone());
                let ruleset = ruleset_id
                    .clone()
                    .or_else(|| self.created_rulesets.get(&key).cloned());
                let (ruleset, created) = api
                    .create_rule(zone_id, phase, ruleset.as_deref(), rule, None)
                    .await
                    .map_err(|e| e.text())?;
                self.created_rulesets.insert(key, ruleset.clone());
                warn_local(
                    self.local
                        .own_edge_rule(
                            account,
                            super::local_edge::EdgeRuleRow {
                                rule_id: created.id.clone(),
                                zone_id: zone_id.clone(),
                                phase: phase.clone(),
                                hostname: (*kind != super::edge::RuleKind::RateLimit)
                                    .then(|| hostnames.join(", ")),
                                kind: serde_json::to_value(kind)
                                    .ok()
                                    .and_then(|v| v.as_str().map(str::to_owned))
                                    .unwrap_or_default(),
                            },
                        )
                        .await,
                );
                Ok(Some(Undo::DeleteEdgeRule {
                    zone: zone_id.clone(),
                    ruleset,
                    id: created.id,
                    hostnames: hostnames.join(", "),
                }))
            }
            Step::UpdateEdgeRule {
                zone_id,
                ruleset_id,
                rule_id,
                hostnames,
                rule,
                previous,
                ..
            } => {
                api.update_rule(zone_id, ruleset_id, rule_id, rule)
                    .await
                    .map_err(|e| e.text())?;
                Ok(Some(Undo::RestoreEdgeRule {
                    zone: zone_id.clone(),
                    ruleset: ruleset_id.clone(),
                    id: rule_id.clone(),
                    previous: previous.clone(),
                    hostnames: hostnames.join(", "),
                }))
            }
            Step::DeleteEdgeRule {
                zone_id,
                phase,
                ruleset_id,
                rule_id,
                hostnames,
                previous,
                position,
                ..
            } => {
                api.delete_rule(zone_id, ruleset_id, rule_id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(self.local.disown_edge_rule(rule_id).await);
                Ok(Some(Undo::RecreateEdgeRule {
                    zone: zone_id.clone(),
                    phase: phase.clone(),
                    ruleset: ruleset_id.clone(),
                    rule: previous.clone(),
                    position: *position,
                    hostnames: hostnames.join(", "),
                }))
            }
            Step::CreateServiceToken { hostname, name } => {
                let issued = api
                    .create_service_token(account, name, super::edge::SERVICE_TOKEN_DURATION)
                    .await
                    .map_err(|e| e.text())?;
                let token = super::edge::IssuedToken::from(issued);
                warn_local(
                    self.local
                        .own_service_token(
                            account,
                            super::local_edge::ServiceTokenRow {
                                token_id: token.token_id.clone(),
                                hostname: hostname.clone(),
                                name: token.name.clone(),
                                client_id: token.client_id.clone(),
                                expires_at: token.expires_at.clone(),
                                created_at: 0,
                            },
                        )
                        .await,
                );
                self.created_token = Some(token.token_id.clone());
                let undo = Undo::DeleteServiceToken {
                    id: token.token_id.clone(),
                    name: token.name.clone(),
                };
                self.issued.push(token);
                Ok(Some(undo))
            }
            Step::AllowServiceToken { domain, app, token } => {
                let token = match token {
                    super::types::TokenRef::Existing(id) => id.clone(),
                    super::types::TokenRef::Created => self
                        .created_token
                        .clone()
                        .ok_or_else(msg::protection::error::token_not_created)?,
                };
                match app {
                    Some((id, previous)) => {
                        let wanted = super::access::with_service_token(previous, &token);
                        api.update_access_app(account, id, &wanted)
                            .await
                            .map_err(|e| e.text())?;
                        Ok(Some(Undo::RestoreAccessApp {
                            id: id.clone(),
                            previous: previous.clone(),
                        }))
                    }
                    None => {
                        let wanted = super::access::with_service_token(
                            &super::access::machine_only_definition(domain),
                            &token,
                        );
                        let created = api
                            .create_access_app(account, &wanted)
                            .await
                            .map_err(|e| e.text())?;
                        warn_local(
                            self.local
                                .own_access_app(account, &created.id, domain)
                                .await,
                        );
                        Ok(Some(Undo::DeleteAccessApp {
                            id: created.id,
                            domain: domain.clone(),
                            bypass: false,
                        }))
                    }
                }
            }
            Step::DeleteServiceToken { token } => {
                api.delete_service_token(account, &token.id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(self.local.disown_service_token(&token.id).await);
                Ok(Some(Undo::RestoreServiceToken(token.name.clone())))
            }
            Step::CreateDatabase { name } => {
                let database = api
                    .create_d1_database(account, name)
                    .await
                    .map_err(|e| e.text())?;
                self.created_database = Some(database.uuid.clone());
                // The tables are made at once, so the first comment or webhook doesn't
                // wait for them (the Workers create them too if they're missing). If
                // that fails, the new database goes again.
                if let Err(err) = super::front::create_tables(api, account, &database.uuid).await {
                    let _ = api.delete_d1_database(account, &database.uuid).await;
                    self.created_database = None;
                    return Err(err.text());
                }
                warn_local(
                    self.local
                        .set_cloud_database(account, Some((&database.uuid, name)))
                        .await,
                );
                Ok(Some(Undo::DeleteDatabase(database.uuid)))
            }
            Step::PutFrontWorker {
                hostname,
                zone_id,
                script,
                config,
                previous,
                database,
                secret,
            } => {
                let database = database
                    .as_ref()
                    .map(|d| self.resolve_database(d))
                    .transpose()?;
                self.put_front(
                    hostname,
                    zone_id,
                    script,
                    config,
                    database.as_deref(),
                    secret.as_ref(),
                )
                .await?;
                Ok(Some(match previous {
                    Some(previous) => Undo::RestoreFrontWorker {
                        hostname: hostname.clone(),
                        zone_id: zone_id.clone(),
                        script: script.clone(),
                        config: previous.clone(),
                        database,
                    },
                    None => Undo::DeleteFrontWorker {
                        hostname: hostname.clone(),
                        script: script.clone(),
                        config: config.clone(),
                    },
                }))
            }
            Step::CreateWorkerRoute {
                hostname,
                zone_id,
                pattern,
                script,
                kind,
                path,
            } => {
                let route = api
                    .create_worker_route(zone_id, pattern, script)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(
                    self.local
                        .set_front_route(account, hostname, *kind, path, Some(&route.id))
                        .await,
                );
                Ok(Some(Undo::DeleteWorkerRoute {
                    zone: zone_id.clone(),
                    id: route.id,
                    pattern: pattern.clone(),
                    hostname: hostname.clone(),
                    kind: *kind,
                    path: path.clone(),
                }))
            }
            Step::DeleteWorkerRoute {
                hostname,
                zone_id,
                route,
                kind,
                path,
            } => {
                api.delete_worker_route(zone_id, &route.id)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(
                    self.local
                        .set_front_route(account, hostname, *kind, path, None)
                        .await,
                );
                Ok(Some(Undo::RecreateWorkerRoute {
                    zone: zone_id.clone(),
                    route: route.clone(),
                    hostname: hostname.clone(),
                    kind: *kind,
                    path: path.clone(),
                }))
            }
            Step::DeleteFrontWorker {
                hostname,
                zone_id,
                script,
                previous,
                database,
            } => {
                api.delete_worker_script(account, script)
                    .await
                    .map_err(|e| e.text())?;
                warn_local(
                    self.local
                        .forget_front(account, hostname, previous.kind(), previous.path())
                        .await,
                );
                Ok(Some(Undo::RestoreFrontWorker {
                    hostname: hostname.clone(),
                    zone_id: zone_id.clone(),
                    script: script.clone(),
                    config: previous.clone(),
                    database: database.clone(),
                }))
            }
            Step::RotateServiceToken { token } => {
                let issued = api
                    .rotate_service_token(account, &token.id)
                    .await
                    .map_err(|e| e.text())?;
                self.issued.push(super::edge::IssuedToken::from(issued));
                Ok(Some(Undo::RestoreServiceToken(token.name.clone())))
            }
        }
    }

    /// Starts the connector if it isn't running, with a fresh run token.
    async fn ensure_connector(&self, tunnel_id: &str) -> Result<(), Text> {
        if self.connectors.is_running(tunnel_id) {
            return Ok(());
        }
        let token = self
            .api
            .tunnel_token(self.account, tunnel_id)
            .await
            .map_err(|e| e.text())?;
        self.connectors.start(self.account, tunnel_id, token).await
    }

    /// Writes `ingress` into the tunnel's configuration, keeping every other setting.
    async fn put_ingress(
        &self,
        tunnel: &str,
        ingress: &[IngressRule],
        expected_version: Option<u64>,
    ) -> Result<(), Text> {
        let current = self
            .api
            .tunnel_config(self.account, tunnel)
            .await
            .map_err(|e| e.text())?;
        if let Some(expected) = expected_version
            && current.version != expected
        {
            return Err(msg::apply::config_changed());
        }
        let mut config = current.config.unwrap_or_else(|| TunnelConfig {
            ingress: Vec::new(),
            origin_request: serde_json::Map::new(),
            extra: serde_json::Map::new(),
        });
        config.ingress = ingress.to_vec();
        let written = self
            .api
            .put_tunnel_config(self.account, tunnel, &config)
            .await
            .map_err(|e| e.text())?;
        warn_local(
            self.local
                .set_applied(tunnel, written.version, ingress)
                .await,
        );
        Ok(())
    }

    async fn undo(&self, undo: &Undo) -> Result<(), Text> {
        let (api, account) = (self.api, self.account);
        let err = |e: cf_api::Error| e.text();
        match undo {
            Undo::DeleteTunnel(id) => {
                api.delete_tunnel(account, id).await.map_err(err)?;
                warn_local(self.local.forget_tunnel(id).await);
                self.connectors.deleted(id).await;
            }
            Undo::RestoreConfig { tunnel, previous } => {
                self.put_ingress(tunnel, previous, None).await?;
            }
            Undo::DeleteRecord { zone, id, .. } => {
                api.delete_record(zone, id).await.map_err(err)?;
                warn_local(self.local.disown_record(id).await);
            }
            Undo::RestoreRecord {
                zone,
                previous,
                current,
                was_owned,
            } => {
                let restored = if *current == previous.id {
                    api.update_record(zone, &previous.id, &previous.to_new())
                        .await
                        .map_err(err)?
                } else {
                    api.replace_record(zone, current, &previous.to_new())
                        .await
                        .map_err(err)?
                };
                warn_local(self.local.disown_record(current).await);
                if *was_owned {
                    let route = route_id_from(previous.comment.as_deref());
                    warn_local(
                        self.local
                            .own_record(account, zone, &restored.id, &previous.name, &route)
                            .await,
                    );
                }
            }
            Undo::RecreateRecord {
                zone,
                record,
                was_owned,
            } => {
                let created = api
                    .create_record(zone, &record.to_new())
                    .await
                    .map_err(err)?;
                if *was_owned {
                    let route = route_id_from(record.comment.as_deref());
                    warn_local(
                        self.local
                            .own_record(account, zone, &created.id, &record.name, &route)
                            .await,
                    );
                }
            }
            Undo::StartConnector(id) => self.ensure_connector(id).await?,
            Undo::DeleteLoginMethod(id) => {
                api.delete_login_method(account, id).await.map_err(err)?;
            }
            Undo::DeleteAccessApp { id, .. } => {
                api.delete_access_app(account, id).await.map_err(err)?;
                warn_local(self.local.disown_access_app(id).await);
            }
            Undo::RestoreAccessApp { id, previous } => {
                api.update_access_app(account, id, previous)
                    .await
                    .map_err(err)?;
                warn_local(
                    self.local
                        .own_access_app(account, id, &previous.domain)
                        .await,
                );
            }
            Undo::RecreateAccessApp(previous) => {
                let created = api
                    .create_access_app(account, previous)
                    .await
                    .map_err(err)?;
                warn_local(
                    self.local
                        .own_access_app(account, &created.id, &previous.domain)
                        .await,
                );
            }
            Undo::DeleteNetworkRoute { id, .. } => {
                api.delete_network_route(account, id).await.map_err(err)?;
            }
            Undo::RecreateNetworkRoute(route) => {
                api.create_network_route(
                    account,
                    &route.network,
                    &route.tunnel_id,
                    &route.comment,
                    route.virtual_network_id.as_deref(),
                )
                .await
                .map_err(err)?;
            }
            Undo::DeleteLbMonitor(id) => api.delete_lb_monitor(account, id).await.map_err(err)?,
            Undo::DeleteLbPool(id) => api.delete_lb_pool(account, id).await.map_err(err)?,
            Undo::RestoreLbPool { id, previous } => {
                api.update_lb_pool(account, id, previous)
                    .await
                    .map_err(err)?;
            }
            Undo::DeleteLoadBalancer { zone, id, hostname } => {
                api.delete_load_balancer(zone, id).await.map_err(err)?;
                warn_local(self.local.set_balanced(account, hostname, false).await);
            }
            // Rolled back in reverse: the monitor comes back first, then the pool (with
            // the monitor's new id), then the load balancer (with the pool's).
            Undo::RecreateLbMonitor(monitor) => {
                let created = api.create_lb_monitor(account, monitor).await.map_err(err)?;
                self.renamed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(monitor.id.clone(), created.id);
            }
            Undo::RecreateLbPool(pool) => {
                let mut pool = pool.clone();
                pool.monitor = pool.monitor.as_deref().map(|id| self.renamed(id));
                let old = std::mem::take(&mut pool.id);
                let created = api.create_lb_pool(account, &pool).await.map_err(err)?;
                self.renamed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(old, created.id);
            }
            Undo::DeleteSnapshotWorker { snapshot, script } => {
                api.delete_worker_script(account, script)
                    .await
                    .map_err(err)?;
                // The Worker and every version it had are gone.
                if let Ok(versions) = self.local.site_versions(snapshot).await {
                    for version in versions {
                        warn_local(
                            self.local
                                .drop_site_version(snapshot, &version.version_id)
                                .await,
                        );
                    }
                }
            }
            Undo::RedeploySnapshot {
                snapshot,
                script,
                version,
                replaced,
            } => {
                api.deploy_worker_version(account, script, version)
                    .await
                    .map_err(err)?;
                if let Some(replaced) = replaced {
                    warn_local(self.local.drop_site_version(snapshot, replaced).await);
                }
                warn_local(self.local.set_site_live(snapshot, version).await);
            }
            Undo::SetWorkersDev { script, enabled } => {
                api.set_worker_on_workers_dev(account, script, *enabled)
                    .await
                    .map_err(err)?;
            }
            Undo::DetachSnapshotDomain { id, .. } => {
                api.detach_worker_domain(account, id).await.map_err(err)?;
            }
            Undo::ReattachSnapshotDomain(domain) => {
                api.attach_worker_domain(
                    account,
                    &domain.hostname,
                    &domain.zone_id,
                    &domain.service,
                )
                .await
                .map_err(err)?;
            }
            Undo::RecreateSnapshotWorker(script) => {
                return Err(msg::snapshot::leftover::recreate_worker(script));
            }
            Undo::DeleteEdgeRule {
                zone, ruleset, id, ..
            } => {
                api.delete_rule(zone, ruleset, id).await.map_err(err)?;
                warn_local(self.local.disown_edge_rule(id).await);
            }
            Undo::RestoreEdgeRule {
                zone,
                ruleset,
                id,
                previous,
                ..
            } => {
                api.update_rule(zone, ruleset, id, previous)
                    .await
                    .map_err(err)?;
            }
            Undo::RecreateEdgeRule {
                zone,
                phase,
                ruleset,
                rule,
                position,
                ..
            } => {
                let (_, created) = api
                    .create_rule(zone, phase, Some(ruleset), rule, Some(*position))
                    .await
                    .map_err(err)?;
                warn_local(
                    self.local
                        .own_edge_rule(
                            account,
                            super::local_edge::EdgeRuleRow {
                                rule_id: created.id,
                                zone_id: zone.clone(),
                                phase: phase.clone(),
                                hostname: None,
                                kind: String::new(),
                            },
                        )
                        .await,
                );
            }
            Undo::DeleteServiceToken { id, .. } => {
                api.delete_service_token(account, id).await.map_err(err)?;
                warn_local(self.local.disown_service_token(id).await);
            }
            Undo::RestoreServiceToken(name) => {
                return Err(msg::protection::leftover::restore_token(name));
            }
            Undo::DeleteDatabase(id) => {
                api.delete_d1_database(account, id).await.map_err(err)?;
                warn_local(self.local.set_cloud_database(account, None).await);
            }
            Undo::DeleteFrontWorker {
                hostname,
                script,
                config,
            } => {
                api.delete_worker_script(account, script)
                    .await
                    .map_err(err)?;
                warn_local(
                    self.local
                        .forget_front(account, hostname, config.kind(), config.path())
                        .await,
                );
            }
            Undo::RestoreFrontWorker {
                hostname,
                zone_id,
                script,
                config,
                database,
            } => {
                // A verifying inbox gets its signing secret back: a Worker that was
                // deleted has none, and one whose verification changed has the new one.
                let secret = match (config, self.secrets) {
                    (super::front::FrontConfig::Inbox { settings, .. }, Some(secrets)) => {
                        match settings.verify {
                            Some(verify) => {
                                super::front::saved_inbox_secret(secrets, hostname, verify).await
                            }
                            None => None,
                        }
                    }
                    _ => None,
                };
                self.put_front(
                    hostname,
                    zone_id,
                    script,
                    config,
                    database.as_deref(),
                    secret.as_ref(),
                )
                .await?;
            }
            Undo::DeleteWorkerRoute {
                zone,
                id,
                hostname,
                kind,
                path,
                ..
            } => {
                api.delete_worker_route(zone, id).await.map_err(err)?;
                warn_local(
                    self.local
                        .set_front_route(account, hostname, *kind, path, None)
                        .await,
                );
            }
            Undo::RecreateWorkerRoute {
                zone,
                route,
                hostname,
                kind,
                path,
            } => {
                let script = route.script.clone().unwrap_or_default();
                let created = api
                    .create_worker_route(zone, &route.pattern, &script)
                    .await
                    .map_err(err)?;
                warn_local(
                    self.local
                        .set_front_route(account, hostname, *kind, path, Some(&created.id))
                        .await,
                );
            }
            Undo::RecreateLoadBalancer { zone, balancer } => {
                let mut balancer = balancer.clone();
                balancer.id = String::new();
                balancer.default_pools = balancer
                    .default_pools
                    .iter()
                    .map(|id| self.renamed(id))
                    .collect();
                balancer.fallback_pool = self.renamed(&balancer.fallback_pool);
                api.create_load_balancer(zone, &balancer)
                    .await
                    .map_err(err)?;
                warn_local(self.local.set_balanced(account, &balancer.name, true).await);
            }
        }
        Ok(())
    }

    async fn roll_back(
        &mut self,
        failed: u32,
        error: Text,
        progress: &mut impl FnMut(Progress),
    ) -> Outcome {
        let mut leftovers = Vec::new();
        let done = std::mem::take(&mut self.done);
        for (step, undo) in done.iter().rev() {
            let step = *step;
            progress(Progress {
                step,
                state: StepState::Undoing,
            });
            match self.undo(undo).await {
                Ok(()) => progress(Progress {
                    step,
                    state: StepState::Undone,
                }),
                Err(message) => {
                    leftovers.push(undo.leftover());
                    progress(Progress {
                        step,
                        state: StepState::UndoFailed { message },
                    });
                }
            }
        }
        leftovers.reverse();
        if leftovers.is_empty() {
            for step in std::mem::take(&mut self.covered) {
                progress(Progress {
                    step,
                    state: StepState::Undone,
                });
            }
        }
        if leftovers.is_empty() {
            Outcome::RolledBack {
                failed_step: failed,
                error,
            }
        } else {
            Outcome::PartiallyApplied {
                failed_step: failed,
                error,
                leftovers,
            }
        }
    }
}
