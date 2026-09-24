//! [`Backend`] over `teitunnel-core`: the accounts, engine, shares, discovery, doctor
//! and logs a host (the CLI's `teitunnel mcp` and `teitunnel serve`, later the app)
//! already has. What differs between hosts is how connector state is known
//! ([`ConnectorSource`]): the CLI probes the app's connectors, `serve` runs its own.
//!
//! Shares this server starts belong to it: Quick Shares run in this process (recorded
//! for the app next to the CLI's, so the app lists them and can stop them), and shares
//! on a domain are recorded with this process as their owner, so the app removes them
//! if the server dies without doing so.

use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use teitunnel_core::{
    accounts::{Account, Accounts},
    binary::BinaryManager,
    cli_shares::{self, CliShare},
    doctor::{self, Fix, Issue},
    domain::{Hostname, OriginUrl},
    domain_shares::{self, APP_OWNER, ShareRequest},
    engine::{
        ActivityEntry, Actor, Approval, Change, Connectors, Context, Edge, Engine, EngineError,
        Outcome, PlanView, RoutesOverview, TunnelSummary, Verification, with_actor,
    },
    export::{ExportFile, ExportFormat, render},
    import::LocalSetup,
    machine::MachineTunnels,
    quick_share::{HostHeaderChoice, QuickShare, QuickShares, ShareStatus},
    remote_logs::RemoteLogs,
    runtime,
    store::Store,
};
use tokio::sync::broadcast;

use crate::backend::{
    ApplyApproval, Backend, BackendError, BackendResult, BoxFuture, ChangeEvent,
    DomainShareRequest, LocalService, LogBatch, LogLine, ProgressSink, RemoteLogBatch, ShareInfo,
    ShareKind, Target,
};

/// How long a Quick Share may take to get its URL.
const URL_TIMEOUT: Duration = Duration::from_secs(45);

/// Knows the state of this machine's connectors.
pub trait ConnectorSource: Send + Sync + 'static {
    /// The connectors' state.
    type Connectors: Connectors + Send + Sync + 'static;

    /// The connectors of `account` (every account's when `None`).
    fn connectors<'a>(&'a self, account: Option<&'a str>) -> BoxFuture<'a, Self::Connectors>;

    /// Whether this host runs connectors itself (`serve`), so it can start them. When it
    /// doesn't (a terminal-hosted server), the app or an Always-on service runs them.
    fn runs_connectors(&self) -> bool;
}

impl ConnectorSource for MachineTunnels {
    type Connectors = Self;

    fn connectors<'a>(&'a self, _account: Option<&'a str>) -> BoxFuture<'a, Self> {
        let machine = self.clone();
        Box::pin(async move { machine })
    }

    fn runs_connectors(&self) -> bool {
        true
    }
}

/// What a host already has.
#[derive(Debug)]
pub struct CoreParts {
    /// Connected accounts.
    pub accounts: Accounts,
    /// The engine.
    pub engine: Arc<Engine>,
    /// The database.
    pub store: Store,
    /// The cloudflared binary.
    pub binary: BinaryManager,
    /// This machine's name.
    pub machine_name: String,
    /// This machine's tunnels (logs of Always-on connectors and of connectors this
    /// process runs; starting connectors when the host runs them).
    pub machine: MachineTunnels,
    /// Quick Shares run by this process.
    pub quick_shares: QuickShares,
    /// Where terminal processes record their shares (`<data>/run-cli`).
    pub runs: PathBuf,
    /// Where route checks go.
    pub edge: Edge,
    /// Log streams of other machines' connectors.
    pub remote_logs: RemoteLogs,
    /// Applies pauses to this process's taps (its shares on your domain).
    pub pauses: Arc<teitunnel_core::pause::Enforcer>,
}

/// The backend over the core.
pub struct CoreBackend<S: ConnectorSource> {
    parts: CoreParts,
    source: S,
    owner: String,
    changes: broadcast::Sender<ChangeEvent>,
    own_domain: Mutex<HashSet<(String, String)>>,
}

impl<S: ConnectorSource> std::fmt::Debug for CoreBackend<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreBackend")
            .field("owner", &self.owner)
            .finish_non_exhaustive()
    }
}

fn msg(err: impl std::fmt::Display) -> BackendError {
    BackendError::Message(err.to_string())
}

fn engine_error(err: EngineError, account: &str) -> BackendError {
    match err {
        EngineError::Stale(plan) => BackendError::Stale(Box::new(plan.view(account))),
        EngineError::NeedsConfirmation => BackendError::NeedsConfirmation,
        other => BackendError::Message(other.to_string()),
    }
}

fn status_text(status: &ShareStatus) -> String {
    match status {
        ShareStatus::Starting => "starting".into(),
        ShareStatus::Live => "live".into(),
        ShareStatus::Reconnecting => "reconnecting".into(),
        ShareStatus::Failed { message } => format!("failed: {}", message.english()),
    }
}

impl<S: ConnectorSource> CoreBackend<S> {
    /// A backend over `parts`, with connector state from `source`. Starts watching its
    /// Quick Shares (call from a Tokio runtime).
    pub fn new(parts: CoreParts, source: S) -> Arc<Self> {
        let (changes, _) = broadcast::channel(64);
        let backend = Arc::new(Self {
            owner: runtime::this_process(),
            parts,
            source,
            changes,
            own_domain: Mutex::default(),
        });
        tokio::spawn(backend.parts.quick_shares.clone().watch_runtime());
        // Forget the record of a Quick Share that ended by itself (`expiresIn`).
        let weak = Arc::downgrade(&backend);
        let mut updates = backend.parts.quick_shares.subscribe();
        tokio::spawn(async move {
            while let Ok(id) = updates.recv().await {
                let Some(backend) = weak.upgrade() else { break };
                if !backend.parts.quick_shares.list().iter().any(|s| s.id == id) {
                    cli_shares::forget_as(&backend.owner_dir(), &id);
                    let _ = backend.changes.send(ChangeEvent::Shares);
                }
            }
        });
        backend
    }

    fn owner_dir(&self) -> PathBuf {
        self.parts.runs.join(&self.owner)
    }

    fn engine(&self) -> &Engine {
        &self.parts.engine
    }

    fn context<'a>(&'a self, account: &'a str, tunnel: Option<&'a str>) -> Context<'a> {
        Context {
            account,
            machine_name: &self.parts.machine_name,
            tunnel,
        }
    }

    async fn api(&self, account: &str) -> BackendResult<cf_api::Client> {
        self.parts.accounts.client(account).await.map_err(msg)
    }

    fn quick_info(share: &QuickShare) -> ShareInfo {
        ShareInfo {
            id: share.id.clone(),
            kind: ShareKind::Quick,
            url: share.url.clone(),
            origin: share.origin.to_string(),
            status: status_text(&share.status),
            started_by: "this agent".into(),
            mine: true,
            account_id: None,
            started_at: share.started_at,
            expires_at: share.stop_at,
            paused: false,
        }
    }

    /// Waits until a Quick Share has its URL (or fails).
    async fn await_url(&self, id: &str) -> BackendResult<QuickShare> {
        let mut changes = self.parts.quick_shares.subscribe();
        let deadline = tokio::time::Instant::now() + URL_TIMEOUT;
        loop {
            match self
                .parts
                .quick_shares
                .list()
                .into_iter()
                .find(|s| s.id == id)
            {
                None => {
                    return Err(BackendError::message(
                        "The share stopped before it got a URL.",
                    ));
                }
                Some(QuickShare {
                    status: ShareStatus::Failed { message },
                    ..
                }) => return Err(BackendError::Message(message.english())),
                Some(share) if share.status == ShareStatus::Live && share.url.is_some() => {
                    return Ok(share);
                }
                Some(_) => {}
            }
            tokio::select! {
                _ = changes.recv() => {}
                () = tokio::time::sleep(Duration::from_millis(500)) => {}
                () = tokio::time::sleep_until(deadline) => {
                    let _ = self.parts.quick_shares.stop(id).await;
                    return Err(BackendError::message(
                        "Cloudflare didn't hand out a Quick Share address within 45 s. Check the internet connection and try again.",
                    ));
                }
            }
        }
    }

    async fn stop_domain(&self, account: &str, hostname: &str) -> BackendResult<()> {
        let api = self.api(account).await?;
        let connectors = self.source.connectors(Some(account)).await;
        domain_shares::stop(
            self.engine(),
            &api,
            &connectors,
            self.context(account, None),
            hostname,
        )
        .await
        .map_err(|e| BackendError::Message(e.english()))?;
        if let Some(inspector) = self.parts.quick_shares.inspector() {
            domain_shares::release_tap(inspector, account, hostname).await;
        }
        self.own_domain
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&(account.to_owned(), hostname.to_owned()));
        let _ = self.changes.send(ChangeEvent::Shares);
        let _ = self.changes.send(ChangeEvent::Routes);
        Ok(())
    }
}

impl<S: ConnectorSource> Backend for CoreBackend<S> {
    fn machine_name(&self) -> String {
        self.parts.machine_name.clone()
    }

    fn accounts(&self) -> BoxFuture<'_, BackendResult<Vec<Account>>> {
        Box::pin(async move { self.parts.accounts.list().await.map_err(msg) })
    }

    fn capabilities<'a>(
        &'a self,
        account: &'a str,
    ) -> BoxFuture<'a, BackendResult<serde_json::Value>> {
        Box::pin(async move {
            let capabilities = self
                .parts
                .accounts
                .capabilities(account)
                .await
                .map_err(msg)?;
            serde_json::to_value(capabilities).map_err(msg)
        })
    }

    fn domains<'a>(&'a self, account: &'a str) -> BoxFuture<'a, BackendResult<serde_json::Value>> {
        Box::pin(async move {
            let domains = self.parts.accounts.domains(account).await.map_err(msg)?;
            serde_json::to_value(domains).map_err(msg)
        })
    }

    fn overview<'a>(&'a self, account: &'a str) -> BoxFuture<'a, BackendResult<RoutesOverview>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            let connectors = self.source.connectors(Some(account)).await;
            self.engine()
                .overview(&api, &connectors, self.context(account, None))
                .await
                .map_err(msg)
        })
    }

    fn tunnels<'a>(&'a self, account: &'a str) -> BoxFuture<'a, BackendResult<Vec<TunnelSummary>>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            let connectors = self.source.connectors(Some(account)).await;
            self.engine()
                .tunnels(&api, &connectors, account)
                .await
                .map_err(msg)
        })
    }

    fn preview<'a>(
        &'a self,
        target: &'a Target,
        change: &'a Change,
    ) -> BoxFuture<'a, BackendResult<PlanView>> {
        Box::pin(async move {
            let api = self.api(&target.account).await?;
            let ctx = self.context(&target.account, target.tunnel.as_deref());
            let intent = self
                .engine()
                .intent_for(&api, ctx, change)
                .await
                .map_err(|e| engine_error(e, &target.account))?;
            let plan = self
                .engine()
                .preview(&api, ctx, &intent)
                .await
                .map_err(|e| engine_error(e, &target.account))?;
            Ok(plan.view(&target.account))
        })
    }

    fn apply<'a>(
        &'a self,
        target: &'a Target,
        change: &'a Change,
        approval: ApplyApproval,
        actor: Option<Actor>,
        progress: ProgressSink,
    ) -> BoxFuture<'a, BackendResult<Outcome>> {
        Box::pin(async move {
            let api = self.api(&target.account).await?;
            let connectors = self.source.connectors(Some(&target.account)).await;
            let ctx = self.context(&target.account, target.tunnel.as_deref());
            let run = async {
                let intent = self.engine().intent_for(&api, ctx, change).await?;
                self.engine()
                    .apply(
                        &api,
                        &connectors,
                        ctx,
                        &intent,
                        Approval {
                            fingerprint: &approval.fingerprint,
                            confirmed: approval.confirmed,
                        },
                        progress,
                    )
                    .await
            };
            let outcome = match actor {
                Some(actor) => with_actor(actor, run).await,
                None => run.await,
            }
            .map_err(|e| engine_error(e, &target.account))?;
            let _ = self.changes.send(ChangeEvent::Routes);
            Ok(outcome)
        })
    }

    fn protection<'a>(
        &'a self,
        account: &'a str,
        hostname: &'a str,
    ) -> BoxFuture<'a, BackendResult<teitunnel_core::protection::ProtectionView>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            teitunnel_core::protection::view(
                self.engine(),
                &api,
                self.context(account, None),
                hostname,
            )
            .await
            .map_err(|e| engine_error(e, account))
        })
    }

    fn service_tokens<'a>(
        &'a self,
        account: &'a str,
        hostname: &'a str,
    ) -> BoxFuture<'a, BackendResult<Vec<teitunnel_core::protection::ServiceTokenView>>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            teitunnel_core::protection::tokens(self.engine(), &api, account, hostname)
                .await
                .map_err(|e| engine_error(e, account))
        })
    }

    fn preview_protection<'a>(
        &'a self,
        account: &'a str,
        change: &'a teitunnel_core::protection::ProtectionChange,
    ) -> BoxFuture<'a, BackendResult<PlanView>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            teitunnel_core::protection::preview(
                self.engine(),
                &api,
                self.context(account, None),
                change,
            )
            .await
            .map_err(|e| engine_error(e, account))
        })
    }

    fn apply_protection<'a>(
        &'a self,
        account: &'a str,
        change: &'a teitunnel_core::protection::ProtectionChange,
        approval: ApplyApproval,
        actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<(Outcome, Vec<teitunnel_core::engine::edge::IssuedToken>)>>
    {
        Box::pin(async move {
            let api = self.api(account).await?;
            let connectors = self.source.connectors(Some(account)).await;
            let run = teitunnel_core::protection::apply(
                self.engine(),
                &api,
                &connectors,
                self.context(account, None),
                change,
                Approval {
                    fingerprint: &approval.fingerprint,
                    confirmed: approval.confirmed,
                },
                |_| {},
            );
            let result = match actor {
                Some(actor) => with_actor(actor, run).await,
                None => run.await,
            }
            .map_err(|e| engine_error(e, account))?;
            let _ = self.changes.send(ChangeEvent::Routes);
            Ok(result)
        })
    }

    fn verify<'a>(
        &'a self,
        account: &'a str,
        hostname: &'a str,
        patience: Duration,
    ) -> BoxFuture<'a, BackendResult<Verification>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            let host = Hostname::parse(hostname).map_err(msg)?;
            self.engine()
                .verify(
                    &api,
                    self.context(account, None),
                    &host,
                    self.parts.edge,
                    patience,
                )
                .await
                .map_err(msg)
        })
    }

    fn doctor(&self) -> BoxFuture<'_, BackendResult<Vec<Issue>>> {
        Box::pin(async move {
            let connectors = self.source.connectors(None).await;
            let ignored: HashSet<String> = teitunnel_core::settings::load(&self.parts.store)
                .await
                .map(|s| s.ignored_issues.into_iter().collect())
                .unwrap_or_default();
            Ok(doctor::run(
                &self.parts.accounts,
                self.engine(),
                &connectors,
                &self.parts.binary,
                &self.parts.machine_name,
            )
            .await
            .into_iter()
            .filter(|i| !ignored.contains(&i.id))
            .collect())
        })
    }

    fn run_fix<'a>(
        &'a self,
        issue: &'a Issue,
        fix: &'a Fix,
        _actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<String>> {
        Box::pin(async move {
            match fix {
                Fix::StartConnector { account_id } => {
                    if !self.source.runs_connectors() {
                        return Err(BackendError::Unsupported(
                            "This MCP server runs in a terminal and doesn't run connectors. Ask the person to open Teitunnel (it starts the connector), turn on Always-on, or run `teitunnel up`.".into(),
                        ));
                    }
                    let api = self.api(account_id).await?;
                    self.parts
                        .machine
                        .resume(&api, account_id)
                        .await
                        .map_err(|e| BackendError::Message(e.english()))?;
                    let _ = self.changes.send(ChangeEvent::Routes);
                    Ok("Started this machine's connector.".into())
                }
                Fix::KeepTheirs { account_id } => {
                    let api = self.api(account_id).await?;
                    let drift = self
                        .engine()
                        .drift(&api, account_id, issue.tunnel_id.as_deref())
                        .await
                        .map_err(msg)?
                        .ok_or_else(|| {
                            BackendError::message("There's no outside edit to keep any more.")
                        })?;
                    self.engine()
                        .keep_theirs(account_id, &drift)
                        .await
                        .map_err(msg)?;
                    let _ = self.changes.send(ChangeEvent::Routes);
                    Ok("Kept the outside edit: it's the new baseline.".into())
                }
                Fix::CleanConnections {
                    account_id,
                    tunnel_id,
                } => {
                    let api = self.api(account_id).await?;
                    api.clean_connections(account_id, tunnel_id)
                        .await
                        .map_err(msg)?;
                    Ok("Removed the tunnel's stale connections.".into())
                }
                Fix::Change { .. } | Fix::InstallBinary | Fix::Reconnect => Err(
                    BackendError::Unsupported("This fix isn't run this way.".into()),
                ),
            }
        })
    }

    fn start_quick_share<'a>(
        &'a self,
        origin: &'a str,
        stop_after: Option<Duration>,
    ) -> BoxFuture<'a, BackendResult<ShareInfo>> {
        Box::pin(async move {
            let origin = OriginUrl::parse(origin).map_err(msg)?;
            let share = self
                .parts
                .quick_shares
                // Like the app: dev servers that need their own Host get it.
                .start(origin, stop_after, &HostHeaderChoice::Auto)
                .await
                .map_err(|e| match e {
                    teitunnel_core::quick_share::QuickShareError::Binary(cloudflared::Error::NotFound) => {
                        BackendError::message(
                            "cloudflared isn't installed. Ask the person to open Teitunnel (it installs it) or install it with their package manager.",
                        )
                    }
                    other => msg(other),
                })?;
            let live = self.await_url(&share.id).await?;
            let url = live.url.clone().unwrap_or_default();
            let record = CliShare {
                owner: self.owner.clone(),
                origin: live.origin.to_string(),
                url,
                started_at: live.started_at,
                stop_at: live.stop_at,
            };
            if let Err(err) = cli_shares::record_as(&self.owner_dir(), &live.id, &record) {
                tracing::warn!(%err, "couldn't record the share for the app");
            }
            let _ = self.changes.send(ChangeEvent::Shares);
            Ok(Self::quick_info(&live))
        })
    }

    fn start_domain_share(
        &self,
        request: DomainShareRequest,
        actor: Option<Actor>,
    ) -> BoxFuture<'_, BackendResult<(ShareInfo, Outcome)>> {
        Box::pin(async move {
            let api = self.api(&request.account).await?;
            let connectors = self.source.connectors(Some(&request.account)).await;
            let now = domain_shares::now_ms();
            let expires_at = request
                .expires_in
                .map(|d| now + u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            let hostname = request.hostname.trim().to_ascii_lowercase();
            let host_header = HostHeaderChoice::Auto
                .resolve(&request.origin)
                .await
                .ok()
                .flatten()
                .map(|h| h.value);
            let run = domain_shares::start(
                self.engine(),
                &api,
                &connectors,
                self.context(&request.account, None),
                ShareRequest {
                    hostname: &hostname,
                    origin: &request.origin,
                    access: request.access.clone(),
                    expires_at,
                    owner: &self.owner,
                    host_header,
                    source: None,
                    folder: false,
                },
            );
            let outcome = match actor {
                Some(actor) => with_actor(actor, run).await,
                None => run.await,
            }
            .map_err(|e| match e {
                EngineError::NeedsConfirmation => BackendError::Message(format!(
                    "{hostname} already has a DNS record Teitunnel didn't create; a share never takes it over. Choose another hostname."
                )),
                other => msg(other),
            })?;
            if let Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } =
                &outcome
            {
                return Err(BackendError::Message(format!(
                    "Couldn't share: {}",
                    error.english()
                )));
            }
            self.own_domain
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert((request.account.clone(), hostname.clone()));
            let _ = self.changes.send(ChangeEvent::Shares);
            let _ = self.changes.send(ChangeEvent::Routes);
            if let Some(after) = request.expires_in {
                let account = request.account.clone();
                let host = hostname.clone();
                let accounts = self.parts.accounts.clone();
                let engine = Arc::clone(&self.parts.engine);
                let machine_name = self.parts.machine_name.clone();
                let connectors = self.source.connectors(Some(&account)).await;
                let changes = self.changes.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(after).await;
                    let Ok(api) = accounts.client(&account).await else {
                        return;
                    };
                    let ctx = Context {
                        account: &account,
                        machine_name: &machine_name,
                        tunnel: None,
                    };
                    if domain_shares::stop(&engine, &api, &connectors, ctx, &host)
                        .await
                        .is_ok()
                    {
                        let _ = changes.send(ChangeEvent::Shares);
                        let _ = changes.send(ChangeEvent::Routes);
                    }
                });
            }
            let info = ShareInfo {
                id: hostname.clone(),
                kind: ShareKind::Domain,
                url: Some(format!("https://{hostname}")),
                origin: request.origin.trim().to_owned(),
                status: "live".into(),
                started_by: "this agent".into(),
                mine: true,
                account_id: Some(request.account.clone()),
                started_at: now,
                expires_at,
                paused: false,
            };
            Ok((info, outcome))
        })
    }

    fn shares(&self) -> BoxFuture<'_, BackendResult<Vec<ShareInfo>>> {
        Box::pin(async move {
            let mut shares: Vec<ShareInfo> = self
                .parts
                .quick_shares
                .list()
                .iter()
                .map(|s| Self::quick_info(s))
                .collect();
            shares.extend(
                cli_shares::list(&self.parts.runs)
                    .into_iter()
                    .filter(|s| s.owner != self.owner)
                    .map(|s| ShareInfo {
                        id: s.url.clone(),
                        kind: ShareKind::Quick,
                        url: Some(s.url),
                        origin: s.origin,
                        status: "live".into(),
                        started_by: "a terminal".into(),
                        mine: false,
                        account_id: None,
                        started_at: s.started_at,
                        expires_at: s.stop_at,
                        paused: false,
                    }),
            );
            let domain = self.engine().local().shares(None).await.map_err(msg)?;
            shares.extend(domain.into_iter().map(|s| ShareInfo {
                id: s.hostname.clone(),
                kind: ShareKind::Domain,
                url: Some(format!("https://{}", s.hostname)),
                origin: s.source.unwrap_or(s.origin),
                status: if s.paused { "paused" } else { "live" }.into(),
                started_by: if s.owner == APP_OWNER {
                    "the app".into()
                } else if s.owner == self.owner {
                    "this agent".into()
                } else {
                    "a terminal".into()
                },
                mine: s.owner == self.owner,
                account_id: Some(s.account_id),
                started_at: s.created_at,
                expires_at: s.expires_at,
                paused: s.paused,
            }));
            Ok(shares)
        })
    }

    fn stop_share<'a>(
        &'a self,
        share: &'a ShareInfo,
        actor: Option<Actor>,
    ) -> BoxFuture<'a, BackendResult<()>> {
        Box::pin(async move {
            match share.kind {
                ShareKind::Quick if share.mine => {
                    self.parts.quick_shares.stop(&share.id).await.map_err(msg)?;
                    cli_shares::forget_as(&self.owner_dir(), &share.id);
                    let _ = self.changes.send(ChangeEvent::Shares);
                    Ok(())
                }
                ShareKind::Quick => {
                    let owner = cli_shares::list(&self.parts.runs)
                        .into_iter()
                        .find(|s| Some(&s.url) == share.url.as_ref())
                        .map(|s| s.owner)
                        .ok_or_else(|| {
                            BackendError::NotFound("That share isn't running any more.".into())
                        })?;
                    cli_shares::stop(&self.parts.runs, &owner).await;
                    let _ = self.changes.send(ChangeEvent::Shares);
                    Ok(())
                }
                ShareKind::Domain => {
                    let account = share
                        .account_id
                        .clone()
                        .ok_or_else(|| BackendError::message("The share has no account."))?;
                    let stop = self.stop_domain(&account, &share.id);
                    match actor {
                        Some(actor) => with_actor(actor, stop).await,
                        None => stop.await,
                    }
                }
            }
        })
    }

    fn services(&self) -> BoxFuture<'_, BackendResult<Vec<LocalService>>> {
        Box::pin(async move { Ok(teitunnel_core::discovery::services().await) })
    }

    fn export<'a>(
        &'a self,
        account: &'a str,
        tunnel: Option<&'a str>,
        format: ExportFormat,
    ) -> BoxFuture<'a, BackendResult<Option<ExportFile>>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            let version = self
                .parts
                .binary
                .current()
                .await
                .ok()
                .and_then(|b| b.version)
                .map(|v| v.to_string());
            let input = self
                .engine()
                .export_input(&api, self.context(account, tunnel), version)
                .await
                .map_err(msg)?;
            Ok(input.map(|input| render(&input, format)))
        })
    }

    fn import_scan(&self) -> BoxFuture<'_, BackendResult<Vec<LocalSetup>>> {
        Box::pin(async move {
            use teitunnel_core::discovery::cloudflared::{ForeignMode, foreign};
            let running: Vec<PathBuf> = foreign()
                .await
                .into_iter()
                .filter_map(|process| match process.mode {
                    ForeignMode::Named {
                        config: Some(config),
                        ..
                    } => Some(PathBuf::from(config)),
                    _ => None,
                })
                .collect();
            tokio::task::spawn_blocking(move || teitunnel_core::import::scan(&running))
                .await
                .map_err(msg)
        })
    }

    fn logs<'a>(
        &'a self,
        account: &'a str,
        tunnel: Option<&'a str>,
        route: Option<(&'a str, Option<&'a str>)>,
        limit: usize,
    ) -> BoxFuture<'a, BackendResult<LogBatch>> {
        Box::pin(async move {
            let machine = &self.parts.machine;
            let local = self.engine().local();
            let tunnel_id = match (tunnel, route) {
                (Some(id), _) => Some(id.to_owned()),
                (None, Some((host, _))) => {
                    local.tunnel_routing(account, host).await.map_err(msg)?
                }
                (None, None) => local
                    .tunnel(account, None)
                    .await
                    .map_err(msg)?
                    .map(|t| t.tunnel_id),
            };
            let Some(tunnel_id) = tunnel_id else {
                return Ok(LogBatch {
                    source: "none".into(),
                    lines: Vec::new(),
                    note: Some(
                        "This machine has no tunnel in this account (or none carries that route)."
                            .into(),
                    ),
                });
            };
            let events = match route {
                Some((host, path)) => machine
                    .route_logs(account, host, path, limit)
                    .await
                    .map_err(|e| BackendError::Message(e.english()))?,
                None => machine.logs(&tunnel_id, limit),
            };
            let always_on = machine.is_always_on(&tunnel_id);
            let source = if always_on {
                "the Always-on connector's log file".to_owned()
            } else if self.source.runs_connectors() {
                "the connector this server runs".to_owned()
            } else {
                "this machine".to_owned()
            };
            let note = (events.is_empty() && !always_on && !self.source.runs_connectors()).then(|| {
                "This tunnel's connector runs in the Teitunnel app, whose logs this terminal-hosted server can't read. Ask the person to open the route's logs in the app, or turn on Always-on (its logs are readable here).".to_owned()
            });
            Ok(LogBatch {
                source,
                lines: events.iter().map(|e| LogLine::from_event(e)).collect(),
                note,
            })
        })
    }

    fn remote_logs<'a>(
        &'a self,
        account: &'a str,
        tunnel: &'a str,
        connector: &'a str,
        limit: usize,
    ) -> BoxFuture<'a, BackendResult<RemoteLogBatch>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            let batch = self
                .parts
                .remote_logs
                .read(&api, account, tunnel, connector, limit);
            Ok(RemoteLogBatch {
                state: batch.state,
                lines: batch.lines.iter().map(|e| LogLine::from_event(e)).collect(),
            })
        })
    }

    fn activity<'a>(
        &'a self,
        account: &'a str,
        limit: u32,
    ) -> BoxFuture<'a, BackendResult<Vec<ActivityEntry>>> {
        Box::pin(async move {
            self.engine()
                .local()
                .activity(account, limit)
                .await
                .map_err(msg)
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<ChangeEvent> {
        self.changes.subscribe()
    }

    fn reservations<'a>(
        &'a self,
        account: &'a str,
    ) -> BoxFuture<'a, BackendResult<teitunnel_core::reservations::Reservations>> {
        Box::pin(async move {
            let api = self.api(account).await?;
            teitunnel_core::reservations::list(self.engine(), &api, account)
                .await
                .map_err(|e| engine_error(e, account))
        })
    }

    fn stop_own_shares(&self) -> BoxFuture<'_, usize> {
        Box::pin(async move {
            let quick: Vec<String> = self
                .parts
                .quick_shares
                .list()
                .into_iter()
                .map(|s| s.id)
                .collect();
            self.parts.quick_shares.stop_all().await;
            for id in &quick {
                cli_shares::forget_as(&self.owner_dir(), id);
            }
            let domain: Vec<(String, String)> = self
                .own_domain
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .cloned()
                .collect();
            let mut stopped = quick.len();
            for (account, hostname) in domain {
                match self.stop_domain(&account, &hostname).await {
                    Ok(()) => stopped += 1,
                    Err(err) => {
                        tracing::warn!(%hostname, %err, "couldn't stop a share on a domain");
                    }
                }
            }
            if stopped > 0 {
                let _ = self.changes.send(ChangeEvent::Shares);
            }
            stopped
        })
    }

    fn set_paused<'a>(
        &'a self,
        account: &'a str,
        hostname: &'a str,
        paused: bool,
    ) -> BoxFuture<'a, BackendResult<()>> {
        Box::pin(async move {
            let inspector = self.parts.quick_shares.inspector().ok_or_else(|| {
                BackendError::message("This server has no inspector to show a paused page.")
            })?;
            let connectors = self.source.connectors(Some(account)).await;
            let here = teitunnel_core::pause::Here {
                accounts: &self.parts.accounts,
                engine: &self.parts.engine,
                connectors: &connectors,
                machine_name: &self.parts.machine_name,
                inspector,
                enforcer: &self.parts.pauses,
            };
            teitunnel_core::pause::set_paused(here, account, hostname, paused)
                .await
                .map_err(|e| BackendError::Message(e.english()))?;
            let _ = self.changes.send(ChangeEvent::Shares);
            Ok(())
        })
    }

    fn set_schedule<'a>(
        &'a self,
        account: &'a str,
        hostname: &'a str,
        schedule: Option<teitunnel_core::schedule::Schedule>,
    ) -> BoxFuture<'a, BackendResult<()>> {
        Box::pin(async move {
            teitunnel_core::schedule::set(&self.parts.store, account, hostname, schedule.as_ref())
                .await
                .map_err(msg)?;
            let _ = self.changes.send(ChangeEvent::Shares);
            Ok(())
        })
    }

    fn schedules(
        &self,
    ) -> BoxFuture<'_, BackendResult<Vec<teitunnel_core::schedule::RouteSchedule>>> {
        Box::pin(async move {
            teitunnel_core::schedule::list(&self.parts.store, None)
                .await
                .map_err(msg)
        })
    }

    fn share_folder(
        &self,
        folder: teitunnel_core::folder_share::FolderShare,
        domain: Option<(String, String)>,
        expires_in: Option<Duration>,
        actor: Option<Actor>,
    ) -> BoxFuture<'_, BackendResult<ShareInfo>> {
        Box::pin(async move {
            let Some((account, hostname)) = domain else {
                let share = self
                    .parts
                    .quick_shares
                    .start_folder(folder.clone(), expires_in)
                    .await
                    .map_err(|e| BackendError::Message(e.to_string()))?;
                let live = self.await_url(&share.id).await?;
                let record = CliShare {
                    owner: self.owner.clone(),
                    origin: folder.path.clone(),
                    url: live.url.clone().unwrap_or_default(),
                    started_at: live.started_at,
                    stop_at: live.stop_at,
                };
                if let Err(err) = cli_shares::record_as(&self.owner_dir(), &live.id, &record) {
                    tracing::warn!(%err, "couldn't record the share for the app");
                }
                let _ = self.changes.send(ChangeEvent::Shares);
                let mut info = Self::quick_info(&live);
                info.origin = folder.path;
                return Ok(info);
            };
            let inspector = self.parts.quick_shares.inspector().ok_or_else(|| {
                BackendError::message("This server has no inspector to serve the folder.")
            })?;
            let api = self.api(&account).await?;
            let connectors = self.source.connectors(Some(&account)).await;
            let now = domain_shares::now_ms();
            let expires_at =
                expires_in.map(|d| now + u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            let hostname = hostname.trim().to_ascii_lowercase();
            let run = domain_shares::start_folder(
                self.engine(),
                &api,
                &connectors,
                self.context(&account, None),
                inspector,
                &hostname,
                &folder,
                None,
                expires_at,
            );
            let outcome = match actor {
                Some(actor) => with_actor(actor, run).await,
                None => run.await,
            }
            .map_err(|e| match e {
                teitunnel_core::inspect::InspectError::Engine(EngineError::NeedsConfirmation) => {
                    BackendError::Message(format!(
                        "{hostname} already has a DNS record Teitunnel didn't create; a share never takes it over. Choose another hostname."
                    ))
                }
                other => msg(other),
            })?;
            if let Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } =
                &outcome
            {
                return Err(BackendError::Message(format!(
                    "Couldn't share: {}",
                    error.english()
                )));
            }
            self.own_domain
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .insert((account.clone(), hostname.clone()));
            let _ = self.changes.send(ChangeEvent::Shares);
            let _ = self.changes.send(ChangeEvent::Routes);
            Ok(ShareInfo {
                id: hostname.clone(),
                kind: ShareKind::Domain,
                url: Some(format!("https://{hostname}")),
                origin: folder.path,
                status: "live".into(),
                started_by: "this agent".into(),
                mine: true,
                account_id: Some(account),
                started_at: now,
                expires_at,
                paused: false,
            })
        })
    }
}
