//! A scripted [`Host`] and a server over it, for tests of this crate and of clients
//! (the CLI). Enabled with the `testing` feature.

use std::{
    collections::{HashSet, VecDeque},
    path::Path,
    sync::{Arc, Mutex, PoisonError},
};

use tokio::sync::broadcast;

use crate::{
    BoxFuture, ConfirmRequest, Decision, Endpoint, Host, HostResult, Limits, Server,
    protocol::{
        AccountInfo, AgentApproval, AgentInfo, AppInfo, ApplyOutcome, ApplyParams, ApplyResult,
        ClientInfo, DoctorIssue, Event, PauseShare, PlanInfo, PreviewParams, RoutesList,
        RoutesParams, RpcError, ShareInfo, ShareKind, StartShare, Status, StopShare, View, code,
    },
};

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A host that records what it's asked and answers confirmations from a queue (no
/// answer queued: [`Decision::Deny`]).
#[derive(Debug)]
pub struct FakeHost {
    /// Answers for the next confirmations.
    pub answers: Mutex<VecDeque<Decision>>,
    /// Confirmations asked, in order.
    pub asked: Mutex<Vec<ConfirmRequest>>,
    /// Programs always allowed.
    pub approved: Mutex<HashSet<String>>,
    /// Shares started.
    pub started: Mutex<Vec<StartShare>>,
    /// Views opened.
    pub opened: Mutex<Vec<View>>,
    /// Pauses (`true`) and resumes asked for, by id.
    pub paused: Mutex<Vec<(String, bool)>>,
    /// Agents connected now, by session.
    pub agents: Mutex<Vec<(u64, AgentInfo)>>,
    /// Agents' approval questions, in order.
    pub agent_questions: Mutex<Vec<AgentApproval>>,
    /// Answers for the next agent approvals (none queued: no).
    pub agent_answers: Mutex<VecDeque<bool>>,
    events: broadcast::Sender<Event>,
}

impl FakeHost {
    /// A host with one account (`a1`, Personal) and one live share (`qs-1`).
    pub fn new() -> Arc<Self> {
        let (events, _) = broadcast::channel(16);
        Arc::new(Self {
            answers: Mutex::default(),
            asked: Mutex::default(),
            approved: Mutex::default(),
            started: Mutex::default(),
            opened: Mutex::default(),
            paused: Mutex::default(),
            agents: Mutex::default(),
            agent_questions: Mutex::default(),
            agent_answers: Mutex::default(),
            events,
        })
    }

    /// Queues the answer to the next confirmation.
    pub fn answer(&self, decision: Decision) {
        lock(&self.answers).push_back(decision);
    }

    /// How many confirmations were asked.
    pub fn asked(&self) -> usize {
        lock(&self.asked).len()
    }

    /// Sends an event to subscribers.
    pub fn emit(&self, event: Event) {
        let _ = self.events.send(event);
    }
}

/// A live share as the fake host reports it.
pub fn share(id: &str) -> ShareInfo {
    ShareInfo {
        id: id.into(),
        kind: ShareKind::Quick,
        url: Some(format!("https://{id}.trycloudflare.com")),
        origin: "http://localhost:3000".into(),
        status: "live".into(),
        error: None,
        started_at: 1,
        expires_at: None,
        requests: Some(0),
        account_id: None,
        paused: false,
    }
}

impl Host for FakeHost {
    fn app(&self) -> AppInfo {
        AppInfo {
            name: "Teitunnel".into(),
            version: "9.9.9".into(),
        }
    }

    fn status(&self) -> BoxFuture<'_, HostResult<Status>> {
        Box::pin(async move {
            Ok(Status {
                app: self.app(),
                accounts: vec![AccountInfo {
                    id: "a1".into(),
                    name: "Personal".into(),
                }],
                tunnels: Vec::new(),
                shares: vec![share("qs-1")],
            })
        })
    }

    fn shares(&self) -> BoxFuture<'_, HostResult<Vec<ShareInfo>>> {
        Box::pin(async move { Ok(vec![share("qs-1")]) })
    }

    fn start_share(&self, request: StartShare) -> BoxFuture<'_, HostResult<ShareInfo>> {
        Box::pin(async move {
            lock(&self.started).push(request);
            Ok(share("qs-2"))
        })
    }

    fn stop_share(&self, request: StopShare) -> BoxFuture<'_, HostResult<()>> {
        Box::pin(async move {
            if request.id == "qs-1" {
                Ok(())
            } else {
                Err(RpcError::new(code::NOT_FOUND, "No such share."))
            }
        })
    }

    fn pause_share(&self, request: PauseShare, paused: bool) -> BoxFuture<'_, HostResult<()>> {
        Box::pin(async move {
            lock(&self.paused).push((request.id, paused));
            Ok(())
        })
    }

    fn agent_connected(&self, session: u64, agent: AgentInfo, _: &ClientInfo) {
        lock(&self.agents).push((session, agent));
    }

    fn agent_disconnected(&self, session: u64) {
        lock(&self.agents).retain(|(s, _)| *s != session);
    }

    fn approve_for_agent(&self, _: u64, request: AgentApproval) -> BoxFuture<'_, bool> {
        Box::pin(async move {
            lock(&self.agent_questions).push(request);
            lock(&self.agent_answers).pop_front().unwrap_or(false)
        })
    }

    fn routes(&self, _: RoutesParams) -> BoxFuture<'_, HostResult<RoutesList>> {
        Box::pin(async move {
            Ok(RoutesList {
                account: AccountInfo {
                    id: "a1".into(),
                    name: "Personal".into(),
                },
                tunnels: Vec::new(),
                routes: Vec::new(),
            })
        })
    }

    fn preview(&self, request: PreviewParams) -> BoxFuture<'_, HostResult<PlanInfo>> {
        Box::pin(async move {
            Ok(PlanInfo {
                account_id: "a1".into(),
                steps: Vec::new(),
                warnings: vec![request.change],
                requires_confirmation: false,
                fingerprint: "f1".into(),
            })
        })
    }

    fn apply<'a>(
        &'a self,
        _: ApplyParams,
        _: &'a ClientInfo,
    ) -> BoxFuture<'a, HostResult<ApplyResult>> {
        Box::pin(async move {
            Ok(ApplyResult {
                outcome: ApplyOutcome::Applied,
                error: None,
                leftovers: Vec::new(),
                verify: vec!["app.example.com".into()],
                connector_error: None,
            })
        })
    }

    fn doctor(&self) -> BoxFuture<'_, HostResult<Vec<DoctorIssue>>> {
        Box::pin(async move { Ok(Vec::new()) })
    }

    fn open(&self, view: View) -> BoxFuture<'_, HostResult<()>> {
        Box::pin(async move {
            lock(&self.opened).push(view);
            Ok(())
        })
    }

    fn confirm(&self, request: ConfirmRequest) -> BoxFuture<'_, Decision> {
        Box::pin(async move {
            lock(&self.asked).push(request);
            lock(&self.answers).pop_front().unwrap_or(Decision::Deny)
        })
    }

    fn is_approved<'a>(&'a self, client: &'a ClientInfo) -> BoxFuture<'a, bool> {
        Box::pin(async move { lock(&self.approved).contains(&client.name) })
    }

    fn approve<'a>(&'a self, client: &'a ClientInfo) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            lock(&self.approved).insert(client.name.clone());
        })
    }

    fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.events.subscribe()
    }
}

/// A server over a [`FakeHost`], stopped when dropped.
#[derive(Debug)]
pub struct Running {
    /// Where it listens.
    pub endpoint: Endpoint,
    /// Its host.
    pub host: Arc<FakeHost>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
}

impl Drop for Running {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}

/// Serves a new [`FakeHost`] for the data folder `data_dir` (call from a Tokio runtime).
///
/// # Errors
/// The endpoint can't be created.
pub async fn serve(data_dir: &Path, limits: Limits) -> std::io::Result<Running> {
    let endpoint = Endpoint::new(data_dir);
    let token = endpoint.ensure_token()?;
    let listener = endpoint.listen().await?;
    let host = FakeHost::new();
    let server = Server::with_limits(host.clone(), token, limits);
    let (stop, stopped) = tokio::sync::oneshot::channel();
    tokio::spawn(server.run(listener, async {
        let _ = stopped.await;
    }));
    Ok(Running {
        endpoint,
        host,
        stop: Some(stop),
    })
}
