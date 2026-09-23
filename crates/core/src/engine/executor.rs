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
};
use super::verify::{Edge, Failure, Verification, check_dns, probe};
use super::{
    cloud::{CloudApi, Connectors},
    local::Local,
    networks::NETWORK_COMMENT,
    observe::{ObserveError, observe},
    planner::{PlanError, plan},
    types::{Intent, Plan, Snapshot, Step, TunnelRef, ownership_comment, tunnel_target},
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
        }
    }
}

english_display!(EngineError);

/// How often a transient verification failure is retried.
const VERIFY_RETRY: Duration = Duration::from_secs(2);

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
            Self::DeleteAccessApp { domain, .. } => m::delete_access_app(domain),
            Self::RestoreAccessApp { previous, .. } => m::restore_access_app(&previous.domain),
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
        }
    }
}

fn route_id_from(comment: Option<&str>) -> &str {
    comment
        .and_then(|c| c.strip_prefix("teitunnel:route="))
        .unwrap_or_default()
}

fn tunnel_cname(hostname: &str, tunnel_id: &str, route_id: &str) -> NewDnsRecord {
    NewDnsRecord {
        name: hostname.to_owned(),
        kind: "CNAME".to_owned(),
        content: tunnel_target(tunnel_id),
        proxied: true,
        ttl: 1,
        comment: Some(ownership_comment(route_id)),
    }
}

type Locks = HashMap<String, Arc<tokio::sync::Mutex<()>>>;

/// Plans and applies changes. One per app; cheap to share.
#[derive(Debug)]
pub struct Engine {
    local: Local,
    locks: Mutex<Locks>,
    cache: Mutex<HashMap<String, (Instant, Snapshot)>>,
}

impl Engine {
    /// An engine over the local store.
    pub fn new(local: Local) -> Self {
        Self {
            local,
            locks: Mutex::default(),
            cache: Mutex::default(),
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
        let need = ObserveNeed::of(intent);
        format!(
            "{account}\n{}\n{scope}\n{}{}{}\n{:?}{}",
            tunnel.unwrap_or_default(),
            u8::from(need.access.setup),
            u8::from(need.access.owned),
            need.access.domains.join(","),
            need.networks,
            u8::from(need.tunnel_names),
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
        for route in &mut merged.routes {
            route.balanced = balanced.contains(&route.hostname.to_ascii_lowercase());
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
        )
        .await?;
        if let Some(failure) = check_dns(&snapshot, hostname.as_str()) {
            return Ok(Verification::new(hostname.to_string(), None, Some(failure)));
        }
        let origin = snapshot
            .routes()
            .into_iter()
            .find(|r| r.hostname.as_deref() == Some(hostname.as_str()))
            .and_then(|r| RouteOrigin::parse(&r.service).ok());
        let deadline = Instant::now() + patience;
        loop {
            let result = probe(edge, hostname, origin.as_ref()).await;
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
        mut progress: P,
    ) -> Result<Outcome, EngineError>
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
        Ok(outcome)
    }
}

struct Run<'a, C, K> {
    api: &'a C,
    connectors: &'a K,
    local: &'a Local,
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

    fn renamed(&self, id: &str) -> String {
        self.renamed
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
            .unwrap_or_else(|| id.to_owned())
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
            match self.step(step).await {
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
                    .create_record(zone_id, &tunnel_cname(hostname, &target, route_id))
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
                api.update_record(
                    zone_id,
                    record_id,
                    &tunnel_cname(hostname, &target, route_id),
                )
                .await
                .map_err(|e| e.text())?;
                warn_local(
                    self.local
                        .own_record(account, zone_id, record_id, hostname, route_id)
                        .await,
                );
                Ok(Some(Undo::RestoreRecord {
                    zone: zone_id.clone(),
                    previous: previous.clone(),
                    was_owned: self.was_owned(record_id),
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
            Step::Verify { .. } => Ok(None),
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
                was_owned,
            } => {
                api.update_record(zone, &previous.id, &previous.to_new())
                    .await
                    .map_err(err)?;
                if !was_owned {
                    warn_local(self.local.disown_record(&previous.id).await);
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
                            .own_record(account, zone, &created.id, &record.name, route)
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
