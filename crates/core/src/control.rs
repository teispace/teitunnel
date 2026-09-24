//! The app's side of the control connection (`teitunnel-control`): [`CoreHost`] answers
//! the CLI, extensions and `teitunnel://` links from the same services the app's
//! windows use (its Quick Shares, the engine, the Doctor), and [`integrations`] keeps
//! the person's choices (connection on or off, links on or off, programs always
//! allowed).
//!
//! What only the shell can do (native dialogs, windows, telling the webview something
//! changed) comes through the [`Ui`] port.

pub mod integrations;
pub mod requests;

use std::{path::PathBuf, sync::Arc, time::Duration};

use teitunnel_control::{
    Action, BoxFuture, ConfirmRequest, Decision, Host, HostResult, Requester,
    protocol::{
        self as wire, AccountInfo, AppInfo, ApplyOutcome, ApplyParams, ApplyResult, ClientInfo,
        DoctorIssue, Event, PlanInfo, PreviewParams, RoutesList, RoutesParams, RpcError, ShareInfo,
        ShareKind, StartShare, Status, StepInfo, StopShare, TunnelInfo, View, code,
    },
};
use tokio::sync::broadcast;

use crate::{
    accounts::{Account, Accounts},
    binary::BinaryManager,
    cli_shares,
    domain::OriginUrl,
    domain_shares,
    engine::{Actor, Approval, Change, Connectors, Context, Engine, EngineError, Outcome},
    machine::MachineTunnels,
    quick_share::{HostHeaderChoice, QuickShare, QuickShareError, QuickShares, ShareStatus},
    runtime::ConnectorState,
    store::Store,
    text::{Text, msg::control as m},
};

/// How long a Quick Share may take to get its address.
const URL_TIMEOUT: Duration = Duration::from_secs(45);

/// A question for the person, in their language.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The dialog's title.
    pub title: Text,
    /// What is asked.
    pub message: Text,
    /// The button that allows it once.
    pub allow: Text,
    /// The button that allows the program from now on (absent for links and for
    /// changes to records Teitunnel didn't create).
    pub always: Option<Text>,
    /// The button that refuses (also the default).
    pub deny: Text,
}

/// What changed, so the shell can refresh its windows and menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// Shares.
    Shares,
    /// An account's routes.
    Routes {
        /// The account.
        account_id: String,
    },
}

/// What the app's shell does for the control connection.
pub trait Ui: Send + Sync + 'static {
    /// Asks the person (a native dialog); dismissing it is [`Decision::Deny`].
    fn confirm(&self, prompt: Prompt) -> BoxFuture<'_, Decision>;
    /// Shows the window at `view`.
    fn open(&self, view: View);
    /// Something changed (refresh windows and the menu bar).
    fn changed(&self, change: Changed);
}

/// What the host needs from the app.
#[derive(Debug)]
pub struct HostParts {
    /// The app's version.
    pub version: String,
    /// The database.
    pub store: Store,
    /// Connected accounts.
    pub accounts: Accounts,
    /// The engine (shared with the app's windows, so applies are serialized).
    pub engine: Arc<Engine>,
    /// This machine's connectors.
    pub machine: MachineTunnels,
    /// The app's Quick Shares.
    pub quick_shares: QuickShares,
    /// cloudflared (for the Doctor).
    pub binary: BinaryManager,
    /// Where terminals record their shares (`<data>/run-cli`).
    pub runs: PathBuf,
    /// This machine's name.
    pub machine_name: String,
    /// Local HTTPS domains, when this process serves them.
    pub local_domains: Option<crate::local_domains::LocalDomains>,
}

/// The wire form of the local domains' status.
fn local_info(status: crate::local_domains::LocalDomainsStatus) -> wire::LocalDomainsInfo {
    wire::LocalDomainsInfo {
        running: status.running,
        https_port: status.https_port,
        http_port: status.http_port,
        error: status.error.map(|e| e.english()),
        domains: status
            .domains
            .into_iter()
            .map(|d| wire::LocalDomainInfo {
                name: d.name,
                url: d.url,
                origin: d.origin,
                wildcard: d.wildcard,
                https: d.https,
                inspect: d.inspect,
                serving: d.serving,
            })
            .collect(),
    }
}

/// [`Host`] over the core.
pub struct CoreHost {
    parts: HostParts,
    ui: Arc<dyn Ui>,
    events: broadcast::Sender<Event>,
}

impl std::fmt::Debug for CoreHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreHost").finish_non_exhaustive()
    }
}

fn error(code: i64, text: &Text) -> RpcError {
    RpcError::new(code, text.english())
}

fn internal(err: impl std::fmt::Display) -> RpcError {
    RpcError::new(code::INTERNAL, err.to_string())
}

fn engine_error(err: EngineError, account: &str) -> RpcError {
    match err {
        EngineError::Stale(plan) => {
            let data =
                serde_json::to_value(plan_info(&plan.view(account), account)).unwrap_or_default();
            error(code::STALE, &m::error::stale()).with_data(data)
        }
        EngineError::NeedsConfirmation => {
            error(code::NEEDS_CONFIRMATION, &m::error::needs_confirmation())
        }
        other => internal(other),
    }
}

fn plan_info(view: &crate::engine::PlanView, account: &str) -> PlanInfo {
    PlanInfo {
        account_id: account.to_owned(),
        steps: view
            .steps
            .iter()
            .map(|step| StepInfo {
                kind: serde_json::to_value(step.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                description: step.description.english(),
                command: step.command.clone(),
            })
            .collect(),
        warnings: view
            .warnings
            .iter()
            .filter_map(|w| serde_json::to_value(w).ok())
            .collect(),
        requires_confirmation: view.requires_confirmation,
        fingerprint: view.fingerprint.clone(),
    }
}

/// A connector state's name (`healthy`, `stopped`, …).
fn state_name(state: Option<&ConnectorState>) -> String {
    state
        .and_then(|s| serde_json::to_value(s).ok())
        .and_then(|v| v.get("state").and_then(|s| s.as_str()).map(str::to_owned))
        .unwrap_or_else(|| "stopped".into())
}

fn quick_info(share: &QuickShare, requests: Option<u64>) -> ShareInfo {
    let (status, error) = match &share.status {
        ShareStatus::Starting => ("starting", None),
        ShareStatus::Live => ("live", None),
        ShareStatus::Reconnecting => ("reconnecting", None),
        ShareStatus::Failed { message } => ("failed", Some(message.english())),
    };
    ShareInfo {
        id: share.id.clone(),
        kind: ShareKind::Quick,
        url: share.url.clone(),
        origin: share.origin.to_string(),
        status: status.into(),
        error,
        started_at: share.started_at,
        expires_at: share.stop_at,
        requests,
        account_id: None,
    }
}

/// Describes the client for the person.
fn who(requester: &Requester) -> String {
    match requester {
        Requester::Client(client) => client.name.clone(),
        Requester::Link => String::new(),
    }
}

impl CoreHost {
    /// A host over the app's services, asking and showing through `ui`.
    pub fn new(parts: HostParts, ui: Arc<dyn Ui>) -> Arc<Self> {
        let (events, _) = broadcast::channel(128);
        Arc::new(Self { parts, ui, events })
    }

    /// Tells subscribers something changed (the shell forwards what its windows are
    /// told).
    pub fn publish(&self, event: Event) {
        let _ = self.events.send(event);
    }

    /// The account named (by id or name) or, with none named, the only one.
    async fn account(&self, wanted: Option<&str>) -> HostResult<Account> {
        let accounts = self.parts.accounts.list().await.map_err(internal)?;
        match wanted.map(str::trim).filter(|w| !w.is_empty()) {
            Some(wanted) => accounts
                .into_iter()
                .find(|a| a.id == wanted || a.name.eq_ignore_ascii_case(wanted))
                .ok_or_else(|| error(code::NOT_FOUND, &m::error::unknown_account(wanted))),
            None => match accounts.len() {
                0 => Err(error(code::NOT_FOUND, &m::error::no_account())),
                1 => accounts
                    .into_iter()
                    .next()
                    .ok_or_else(|| error(code::NOT_FOUND, &m::error::no_account())),
                _ => {
                    let names: Vec<_> = accounts.iter().map(|a| a.name.as_str()).collect();
                    Err(error(
                        code::INVALID_PARAMS,
                        &m::error::choose_account(names.join(", ")),
                    ))
                }
            },
        }
    }

    /// A tunnel of this machine's by id or name.
    async fn tunnel(&self, account: &str, wanted: Option<&str>) -> HostResult<Option<String>> {
        let Some(wanted) = wanted.map(str::trim).filter(|w| !w.is_empty()) else {
            return Ok(None);
        };
        let tunnels = self
            .parts
            .engine
            .local()
            .tunnels(account)
            .await
            .map_err(internal)?;
        tunnels
            .into_iter()
            .find(|t| t.tunnel_id == wanted || t.name.eq_ignore_ascii_case(wanted))
            .map(|t| Some(t.tunnel_id))
            .ok_or_else(|| error(code::NOT_FOUND, &m::error::unknown_tunnel(wanted)))
    }

    fn context<'a>(&'a self, account: &'a str, tunnel: Option<&'a str>) -> Context<'a> {
        Context {
            account,
            machine_name: &self.parts.machine_name,
            tunnel,
        }
    }

    fn change(value: serde_json::Value) -> HostResult<Change> {
        serde_json::from_value(value)
            .map_err(|e| error(code::INVALID_PARAMS, &m::error::invalid_change(e)))
    }

    async fn all_shares(&self) -> Vec<ShareInfo> {
        let mut shares = Vec::new();
        for share in self.parts.quick_shares.list() {
            let requests = self
                .parts
                .quick_shares
                .stats(&share.id)
                .await
                .ok()
                .map(|s| u64::from(s.requests));
            shares.push(quick_info(&share, requests));
        }
        for share in self
            .parts
            .engine
            .local()
            .shares(None)
            .await
            .unwrap_or_default()
        {
            shares.push(ShareInfo {
                id: share.hostname.clone(),
                kind: ShareKind::Domain,
                url: Some(format!("https://{}", share.hostname)),
                origin: share.origin,
                status: "live".into(),
                error: None,
                started_at: share.created_at,
                expires_at: share.expires_at,
                requests: None,
                account_id: Some(share.account_id),
            });
        }
        for share in cli_shares::list(&self.parts.runs) {
            shares.push(ShareInfo {
                id: share.owner,
                kind: ShareKind::Terminal,
                url: Some(share.url),
                origin: share.origin,
                status: "live".into(),
                error: None,
                started_at: share.started_at,
                expires_at: share.stop_at,
                requests: None,
                account_id: None,
            });
        }
        shares
    }

    async fn await_url(&self, id: &str) -> HostResult<QuickShare> {
        use crate::quick_actions::{Live, wait_live};
        match wait_live(&self.parts.quick_shares, id, URL_TIMEOUT).await {
            Live::Ready(share) => Ok(*share),
            Live::Failed(message) => Err(error(code::INTERNAL, &message)),
            Live::Gone => Err(error(code::INTERNAL, &m::error::no_url())),
            Live::TimedOut => Err(error(code::TIMEOUT, &m::error::url_timeout())),
        }
    }

    /// The prompt for a request (for [`Action::Apply`], the plan's steps).
    async fn prompt(&self, request: &ConfirmRequest) -> Prompt {
        let client = who(&request.requester);
        let link = request.requester == Requester::Link;
        let (message, share_button) = match &request.action {
            Action::StartShare(start) => {
                let origin = OriginUrl::parse(&start.origin)
                    .map_or_else(|_| start.origin.clone(), |o| o.to_string());
                if link {
                    (m::link_share(&origin), true)
                } else {
                    (m::share(&client, &origin), false)
                }
            }
            Action::StopShare(stop) => (m::stop(&client, &stop.id), false),
            Action::Apply(apply) => (self.describe_apply(&client, apply).await, false),
        };
        Prompt {
            title: if link {
                m::link_title()
            } else {
                m::client_title(&client)
            },
            message,
            allow: if share_button {
                m::share_button()
            } else {
                m::allow()
            },
            always: request.offer_always.then(m::allow_always),
            deny: if link { m::cancel() } else { m::deny() },
        }
    }

    async fn describe_apply(&self, client: &str, apply: &ApplyParams) -> Text {
        let preview = PreviewParams {
            account: apply.account.clone(),
            tunnel: apply.tunnel.clone(),
            change: apply.change.clone(),
        };
        let Ok(plan) = self.preview(preview).await else {
            return m::apply_unknown(client);
        };
        let steps: Vec<String> = plan
            .steps
            .iter()
            .map(|s| format!("• {}", s.description))
            .collect();
        let count = u64::try_from(steps.len()).unwrap_or(u64::MAX);
        let mut steps = steps.join("\n");
        if apply.confirmed || plan.requires_confirmation {
            steps.push_str("\n\n");
            steps.push_str(&m::apply_foreign().to_string());
        }
        m::apply(count, client, steps)
    }
}

impl Host for CoreHost {
    fn app(&self) -> AppInfo {
        AppInfo {
            name: "Teitunnel".into(),
            version: self.parts.version.clone(),
        }
    }

    fn status(&self) -> BoxFuture<'_, HostResult<Status>> {
        Box::pin(async move {
            let accounts = self.parts.accounts.list().await.map_err(internal)?;
            let mut tunnels = Vec::new();
            for account in &accounts {
                for tunnel in self
                    .parts
                    .engine
                    .local()
                    .tunnels(&account.id)
                    .await
                    .unwrap_or_default()
                {
                    tunnels.push(TunnelInfo {
                        account_id: account.id.clone(),
                        state: state_name(self.parts.machine.state(&tunnel.tunnel_id).as_ref()),
                        id: tunnel.tunnel_id,
                        name: tunnel.name,
                        is_default: tunnel.is_default,
                    });
                }
            }
            Ok(Status {
                app: self.app(),
                accounts: accounts
                    .into_iter()
                    .map(|a| AccountInfo {
                        id: a.id,
                        name: a.name,
                    })
                    .collect(),
                tunnels,
                shares: self.all_shares().await,
            })
        })
    }

    fn shares(&self) -> BoxFuture<'_, HostResult<Vec<ShareInfo>>> {
        Box::pin(async move { Ok(self.all_shares().await) })
    }

    fn start_share(&self, request: StartShare) -> BoxFuture<'_, HostResult<ShareInfo>> {
        Box::pin(async move {
            let origin = OriginUrl::parse(&request.origin)
                .map_err(|e| RpcError::new(code::INVALID_PARAMS, e.to_string()))?;
            let stop_after = request.stop_after_seconds.map(Duration::from_secs);
            let choice = match request.host_header {
                wire::HostHeader::Auto => HostHeaderChoice::Auto,
                wire::HostHeader::Off => HostHeaderChoice::Off,
                wire::HostHeader::Set { value } => HostHeaderChoice::Set { value },
            };
            let share = self
                .parts
                .quick_shares
                .start(origin, stop_after, &choice)
                .await
                .map_err(|e| match e {
                    QuickShareError::Binary(cloudflared::Error::NotFound) => {
                        error(code::INTERNAL, &m::error::no_binary())
                    }
                    other => internal(other),
                })?;
            self.ui.changed(Changed::Shares);
            let live = self.await_url(&share.id).await?;
            Ok(quick_info(&live, Some(0)))
        })
    }

    fn stop_share(&self, request: StopShare) -> BoxFuture<'_, HostResult<()>> {
        Box::pin(async move {
            let wanted = request.id.trim().trim_end_matches('/');
            let host = wanted
                .trim_start_matches("https://")
                .trim_start_matches("http://");
            let shares = self.all_shares().await;
            let share = shares
                .iter()
                .find(|s| {
                    s.id == wanted
                        || s.url.as_deref().map(|u| u.trim_end_matches('/')) == Some(wanted)
                        || (s.kind == ShareKind::Domain && s.id.eq_ignore_ascii_case(host))
                })
                .ok_or_else(|| error(code::NOT_FOUND, &m::error::unknown_share(wanted)))?;
            match share.kind {
                ShareKind::Quick => self
                    .parts
                    .quick_shares
                    .stop(&share.id)
                    .await
                    .map_err(internal)?,
                ShareKind::Terminal => {
                    cli_shares::stop(&self.parts.runs, &share.id).await;
                }
                ShareKind::Domain => {
                    let account = share.account_id.clone().unwrap_or_default();
                    let api = self
                        .parts
                        .accounts
                        .client(&account)
                        .await
                        .map_err(internal)?;
                    domain_shares::stop(
                        &self.parts.engine,
                        &api,
                        &self.parts.machine,
                        self.context(&account, None),
                        &share.id,
                    )
                    .await
                    .map_err(|e| error(code::INTERNAL, &e))?;
                    self.ui.changed(Changed::Routes {
                        account_id: account,
                    });
                }
            }
            self.ui.changed(Changed::Shares);
            Ok(())
        })
    }

    fn routes(&self, request: RoutesParams) -> BoxFuture<'_, HostResult<RoutesList>> {
        Box::pin(async move {
            let account = self.account(request.account.as_deref()).await?;
            let api = self
                .parts
                .accounts
                .client(&account.id)
                .await
                .map_err(internal)?;
            let overview = self
                .parts
                .engine
                .overview(&api, &self.parts.machine, self.context(&account.id, None))
                .await
                .map_err(internal)?;
            let statuses = overview.statuses();
            let tunnel_name = |id: Option<&str>| {
                overview
                    .tunnels
                    .iter()
                    .find(|t| Some(t.id.as_str()) == id)
                    .map(|t| t.name.clone())
            };
            let routes = overview
                .routes
                .iter()
                .zip(&statuses)
                .map(|(route, (_, health))| wire::RouteInfo {
                    hostname: route.hostname.clone(),
                    path: route.path.clone(),
                    origin: route.origin.clone(),
                    status: serde_json::to_value(health)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default(),
                    status_text: health.text().english(),
                    login: route.access.as_ref().map(crate::engine::AccessRule::people),
                    connect: route.client.as_ref().map(|c| c.command.clone()),
                    tunnel_id: route.tunnel_id.clone(),
                    tunnel_name: tunnel_name(route.tunnel_id.as_deref()),
                    temporary: route.temporary,
                })
                .collect();
            let tunnels = overview
                .tunnels
                .iter()
                .map(|t| TunnelInfo {
                    account_id: account.id.clone(),
                    id: t.id.clone(),
                    name: t.name.clone(),
                    is_default: t.is_default,
                    state: state_name(t.connector.as_ref()),
                })
                .collect();
            Ok(RoutesList {
                account: AccountInfo {
                    id: account.id,
                    name: account.name,
                },
                tunnels,
                routes,
            })
        })
    }

    fn preview(&self, request: PreviewParams) -> BoxFuture<'_, HostResult<PlanInfo>> {
        Box::pin(async move {
            let change = Self::change(request.change)?;
            let account = self.account(request.account.as_deref()).await?;
            let tunnel = self.tunnel(&account.id, request.tunnel.as_deref()).await?;
            let api = self
                .parts
                .accounts
                .client(&account.id)
                .await
                .map_err(internal)?;
            let ctx = self.context(&account.id, tunnel.as_deref());
            let engine = &self.parts.engine;
            let intent = engine
                .intent_for(&api, ctx, &change)
                .await
                .map_err(|e| engine_error(e, &account.id))?;
            let plan = engine
                .preview(&api, ctx, &intent)
                .await
                .map_err(|e| engine_error(e, &account.id))?;
            Ok(plan_info(&plan.view(&account.id), &account.id))
        })
    }

    fn apply<'a>(
        &'a self,
        request: ApplyParams,
        client: &'a ClientInfo,
    ) -> BoxFuture<'a, HostResult<ApplyResult>> {
        Box::pin(async move {
            let change = Self::change(request.change)?;
            let account = self.account(request.account.as_deref()).await?;
            let tunnel = self.tunnel(&account.id, request.tunnel.as_deref()).await?;
            let api = self
                .parts
                .accounts
                .client(&account.id)
                .await
                .map_err(internal)?;
            let ctx = self.context(&account.id, tunnel.as_deref());
            let engine = &self.parts.engine;
            let intent = engine
                .intent_for(&api, ctx, &change)
                .await
                .map_err(|e| engine_error(e, &account.id))?;
            let approval = Approval {
                fingerprint: &request.fingerprint,
                confirmed: request.confirmed,
            };
            let actor = Actor {
                via: "control".into(),
                client: client.name.clone(),
                version: Some(client.version.clone()).filter(|v| !v.is_empty()),
            };
            let outcome = crate::engine::with_actor(
                actor,
                engine.apply(&api, &self.parts.machine, ctx, &intent, approval, |_| {}),
            )
            .await;
            self.ui.changed(Changed::Routes {
                account_id: account.id.clone(),
            });
            Ok(match outcome.map_err(|e| engine_error(e, &account.id))? {
                Outcome::Applied {
                    verify,
                    connector_error,
                    ..
                } => ApplyResult {
                    outcome: ApplyOutcome::Applied,
                    error: None,
                    leftovers: Vec::new(),
                    verify,
                    connector_error: connector_error.map(|e| e.english()),
                },
                Outcome::RolledBack { error, .. } => ApplyResult {
                    outcome: ApplyOutcome::RolledBack,
                    error: Some(error.english()),
                    leftovers: Vec::new(),
                    verify: Vec::new(),
                    connector_error: None,
                },
                Outcome::PartiallyApplied {
                    error, leftovers, ..
                } => ApplyResult {
                    outcome: ApplyOutcome::PartiallyApplied,
                    error: Some(error.english()),
                    leftovers: leftovers.iter().map(Text::english).collect(),
                    verify: Vec::new(),
                    connector_error: None,
                },
            })
        })
    }

    fn doctor(&self) -> BoxFuture<'_, HostResult<Vec<DoctorIssue>>> {
        Box::pin(async move {
            let ignored = crate::settings::load(&self.parts.store)
                .await
                .map(|s| s.ignored_issues)
                .unwrap_or_default();
            let mut issues = crate::doctor::run(
                &self.parts.accounts,
                &self.parts.engine,
                &self.parts.machine,
                &self.parts.binary,
                &self.parts.machine_name,
            )
            .await;
            if let Some(local) = &self.parts.local_domains {
                issues.extend(local.doctor().await);
            }
            Ok(issues
                .into_iter()
                .filter(|issue| !ignored.contains(&issue.id))
                .map(|issue| DoctorIssue {
                    severity: serde_json::to_value(issue.severity)
                        .ok()
                        .and_then(|v| v.as_str().map(str::to_owned))
                        .unwrap_or_default(),
                    subject: issue.label.english(),
                    title: issue.title.english(),
                    detail: issue.detail.english(),
                    id: issue.id,
                    check: issue.check,
                    account_id: issue.account_id,
                })
                .collect())
        })
    }

    fn local_domains(&self) -> BoxFuture<'_, HostResult<wire::LocalDomainsInfo>> {
        Box::pin(async move {
            let local = self.parts.local_domains.as_ref().ok_or_else(|| {
                RpcError::new(code::METHOD_NOT_FOUND, "Local domains aren't served here.")
            })?;
            Ok(local_info(local.status().await))
        })
    }

    fn reload_local_domains(&self) -> BoxFuture<'_, HostResult<wire::LocalDomainsInfo>> {
        Box::pin(async move {
            let local = self.parts.local_domains.as_ref().ok_or_else(|| {
                RpcError::new(code::METHOD_NOT_FOUND, "Local domains aren't served here.")
            })?;
            // A failure to serve is part of the status (and its error).
            let _ = local.sync().await;
            Ok(local_info(local.status().await))
        })
    }

    fn open(&self, view: View) -> BoxFuture<'_, HostResult<()>> {
        Box::pin(async move {
            self.ui.open(view);
            Ok(())
        })
    }

    fn confirm(&self, request: ConfirmRequest) -> BoxFuture<'_, Decision> {
        Box::pin(async move {
            let prompt = self.prompt(&request).await;
            match self.ui.confirm(prompt).await {
                // "Always" only counts where it was offered.
                Decision::Always if !request.offer_always => Decision::Once,
                decision => decision,
            }
        })
    }

    fn is_approved<'a>(&'a self, client: &'a ClientInfo) -> BoxFuture<'a, bool> {
        Box::pin(async move { integrations::is_approved(&self.parts.store, &client.name).await })
    }

    fn approve<'a>(&'a self, client: &'a ClientInfo) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            if let Err(err) = integrations::approve(&self.parts.store, client).await {
                tracing::warn!(%err, "couldn't remember the approved program");
            }
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }
}

/// The event for a change the shell told its windows about.
pub fn event_for(change: &Changed) -> Event {
    match change {
        Changed::Shares => Event::SharesChanged { id: None },
        Changed::Routes { account_id } => Event::RoutesChanged {
            account_id: Some(account_id.clone()),
        },
    }
}

#[cfg(test)]
mod tests;
