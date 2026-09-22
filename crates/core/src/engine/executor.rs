//! The executor: applies a reviewed plan, step by step, and undoes completed steps in
//! reverse order if one fails (ARCHITECTURE §4.4).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cf_api::{DnsRecord, IngressRule, NewDnsRecord, TunnelConfig};
use serde::Serialize;
use tokio::time::Instant;

use super::drift::{Drift, diff};
use super::views::{Change, InputError, RoutesOverview, overview, to_intent};

/// A snapshot with nothing in it, for changes that don't need one to be parsed.
static EMPTY: Snapshot = Snapshot {
    account_id: String::new(),
    machine_name: String::new(),
    zones: Vec::new(),
    tunnel: None,
    tunnel_names: Vec::new(),
    records: Vec::new(),
};
use super::verify::{Edge, Failure, Verification, check_dns, probe};
use super::{
    cloud::{CloudApi, Connectors},
    local::Local,
    observe::{ObserveError, observe},
    planner::{PlanError, plan},
    types::{Intent, Plan, Snapshot, Step, TunnelRef, ownership_comment, tunnel_target},
};
use crate::domain::{Hostname, RouteOrigin};

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
    #[error("Something changed in Cloudflare since you reviewed this change. Review it again.")]
    Stale(Box<Plan>),
    /// The plan touches records Teitunnel didn't create and wasn't confirmed.
    #[error("This change replaces DNS records Teitunnel didn't create. Confirm it first.")]
    NeedsConfirmation,
    /// The request itself is invalid.
    #[error(transparent)]
    Input(#[from] InputError),
    /// "Restore mine" when nothing was changed elsewhere.
    #[error("Nothing to restore: the routes are as Teitunnel left them.")]
    NothingToRestore,
}

/// How often a transient verification failure is retried.
const VERIFY_RETRY: Duration = Duration::from_secs(2);

/// Who is asking: the account and this Mac's name for a new tunnel.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    /// Account id.
    pub account: &'a str,
    /// Name for a new machine tunnel.
    pub machine_name: &'a str,
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
        message: String,
    },
    /// Being undone after a later step failed.
    Undoing,
    /// Undone.
    Undone,
    /// Couldn't be undone; left in place.
    UndoFailed {
        /// What went wrong.
        message: String,
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
        connector_error: Option<String>,
    },
    /// A step failed and everything done before it was undone.
    RolledBack {
        /// Index of the failed step.
        failed_step: u32,
        /// Why it failed.
        error: String,
    },
    /// A step failed and some earlier changes couldn't be undone.
    PartiallyApplied {
        /// Index of the failed step.
        failed_step: u32,
        /// Why it failed.
        error: String,
        /// What was left in place.
        leftovers: Vec<String>,
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
}

impl Undo {
    fn leftover(&self) -> String {
        match self {
            Self::DeleteTunnel(id) => format!("Tunnel {id} was created and is still there"),
            Self::RestoreConfig { .. } => {
                "The tunnel's routes weren't restored to what they were".to_owned()
            }
            Self::DeleteRecord { name, .. } => {
                format!("DNS record {name} was created and is still there")
            }
            Self::RestoreRecord { previous, .. } => format!(
                "DNS record {} wasn't restored to {} {}",
                previous.name, previous.kind, previous.content
            ),
            Self::RecreateRecord { record, .. } => format!(
                "DNS record {} ({} {}) was deleted",
                record.name, record.kind, record.content
            ),
            Self::StartConnector(_) => "This Mac's connector is stopped".to_owned(),
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

    fn cache_key(account: &str, intent: &Intent) -> String {
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
        format!("{account}\n{scope}")
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
        let key = Self::cache_key(ctx.account, intent);
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
            ctx.machine_name,
            hostnames.as_deref(),
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
                    .drift(api, ctx.account)
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

    /// This Mac's tunnel and routes in an account, with DNS and connector state. May
    /// reuse an observation up to 5 s old.
    ///
    /// # Errors
    /// Observation errors.
    pub async fn overview<C: CloudApi, K: Connectors>(
        &self,
        api: &C,
        connectors: &K,
        ctx: Context<'_>,
    ) -> Result<RoutesOverview, EngineError> {
        let snapshot = self.snapshot(api, ctx, &Intent::RemoveTunnel, true).await?;
        Ok(overview(&snapshot, |id| connectors.state(id)))
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
    ) -> Result<Option<Drift>, EngineError> {
        let Some(tunnel) = self
            .local
            .machine_tunnel(account)
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
            .applied_ingress(account)
            .await
            .map_err(ObserveError::from)?
            .unwrap_or_default();
        let theirs = current.config.map(|c| c.ingress).unwrap_or_default();
        let changes = diff(&ours, &theirs);
        if changes.is_empty() {
            self.local
                .set_applied(account, current.version, &theirs)
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

    /// Adopts an outside edit as the new baseline ("Keep theirs").
    ///
    /// # Errors
    /// Database errors.
    pub async fn keep_theirs(&self, account: &str, drift: &Drift) -> Result<(), EngineError> {
        let lock = self.lock_for(account);
        let _guard = lock.lock().await;
        self.local
            .set_applied(account, drift.current_version, &drift.theirs)
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
        let snapshot = observe(
            api,
            &self.local,
            ctx.account,
            ctx.machine_name,
            Some(&[hostname]),
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
            created: None,
            done: Vec::new(),
        };
        let serve = matches!(intent, Intent::AddRoute { .. } | Intent::UpdateRoute { .. });
        let outcome = run.execute(&plan, serve, &mut progress).await;
        self.invalidate(ctx.account);

        let mut detail: Vec<String> = plan
            .steps
            .iter()
            .filter(|s| s.is_mutation())
            .map(|s| s.describe(&tunnel_name))
            .collect();
        match &outcome {
            Outcome::Applied {
                connector_error: Some(error),
                ..
            } => detail.push(format!("Connector: {error}")),
            Outcome::RolledBack { error, .. } => detail.push(format!("Failed: {error}")),
            Outcome::PartiallyApplied {
                error, leftovers, ..
            } => {
                detail.push(format!("Failed: {error}"));
                detail.extend(leftovers.iter().map(|l| format!("Left over: {l}")));
            }
            Outcome::Applied { .. } => {}
        }
        if let Err(err) = self
            .local
            .log(ctx.account, &intent.summary(), outcome.label(), &detail)
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
    created: Option<String>,
    done: Vec<(u32, Undo)>,
}

fn warn_local<T>(result: Result<T, crate::store::StoreError>) {
    if let Err(err) = result {
        tracing::warn!(%err, "couldn't update the local ownership index");
    }
}

impl<C: CloudApi, K: Connectors> Run<'_, C, K> {
    fn resolve(&self, tunnel: &TunnelRef) -> Result<String, String> {
        match tunnel {
            TunnelRef::Existing(id) => Ok(id.clone()),
            TunnelRef::Created => self
                .created
                .clone()
                .ok_or_else(|| "The tunnel wasn't created".to_owned()),
        }
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
                    self.done.extend(undo.map(|u| (index, u)));
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

    async fn step(&mut self, step: &Step) -> Result<Option<Undo>, String> {
        let (api, account) = (self.api, self.account);
        match step {
            Step::CreateTunnel { name } => {
                let tunnel = api
                    .create_tunnel(account, name)
                    .await
                    .map_err(|e| e.to_string())?;
                warn_local(
                    self.local
                        .set_machine_tunnel(account, &tunnel.id, name)
                        .await,
                );
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
                    .map_err(|e| e.to_string())?;
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
                .map_err(|e| e.to_string())?;
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
                    .map_err(|e| e.to_string())?;
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
                    .map_err(|e| e.to_string())?;
                warn_local(self.local.forget_machine_tunnel(account).await);
                self.connectors.deleted(tunnel_id).await;
                Ok(None)
            }
            Step::Verify { .. } => Ok(None),
        }
    }

    /// Starts the connector if it isn't running, with a fresh run token.
    async fn ensure_connector(&self, tunnel_id: &str) -> Result<(), String> {
        if self.connectors.is_running(tunnel_id) {
            return Ok(());
        }
        let token = self
            .api
            .tunnel_token(self.account, tunnel_id)
            .await
            .map_err(|e| e.to_string())?;
        self.connectors.start(self.account, tunnel_id, token).await
    }

    /// Writes `ingress` into the tunnel's configuration, keeping every other setting.
    async fn put_ingress(
        &self,
        tunnel: &str,
        ingress: &[IngressRule],
        expected_version: Option<u64>,
    ) -> Result<(), String> {
        let current = self
            .api
            .tunnel_config(self.account, tunnel)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(expected) = expected_version
            && current.version != expected
        {
            return Err(
                "The tunnel's configuration was changed elsewhere while applying".to_owned(),
            );
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
            .map_err(|e| e.to_string())?;
        warn_local(
            self.local
                .set_applied(self.account, written.version, ingress)
                .await,
        );
        Ok(())
    }

    async fn undo(&self, undo: &Undo) -> Result<(), String> {
        let (api, account) = (self.api, self.account);
        let err = |e: cf_api::Error| e.to_string();
        match undo {
            Undo::DeleteTunnel(id) => {
                api.delete_tunnel(account, id).await.map_err(err)?;
                warn_local(self.local.forget_machine_tunnel(account).await);
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
        }
        Ok(())
    }

    async fn roll_back(
        &mut self,
        failed: u32,
        error: String,
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
