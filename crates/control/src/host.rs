//! What the server needs from the app: the same services its windows use, as one
//! object-safe trait. The core implements it over the app's shares, engine and store
//! (`teitunnel_core::control::CoreHost`); tests use an in-memory one.

use std::{future::Future, pin::Pin};

use tokio::sync::broadcast;

use crate::protocol::{
    AgentApproval, AgentInfo, AppInfo, ApplyParams, ApplyResult, ClientInfo, DoctorIssue, Event,
    LocalDomainsInfo, OAuthApproval, PauseShare, PlanInfo, PreviewParams, RoutesList, RoutesParams,
    RpcError, ShareInfo, StartShare, Status, StopShare, View, code,
};

/// A boxed, sendable future (the trait is object-safe).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A host call's result; errors carry a protocol code and an English message.
pub type HostResult<T> = Result<T, RpcError>;

/// A change a client asked for, for the person to approve.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Share a local service.
    StartShare(StartShare),
    /// Stop a share.
    StopShare(StopShare),
    /// Pause a share on your domain or a route.
    PauseShare(PauseShare),
    /// Serve a paused share or route again.
    ResumeShare(PauseShare),
    /// Apply a plan to Cloudflare.
    Apply(ApplyParams),
}

/// Who asks for a change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Requester {
    /// A program connected to the control connection.
    Client(ClientInfo),
    /// A `teitunnel://` link (any website or app can open one).
    Link,
}

/// What the person is asked.
#[derive(Debug, Clone, PartialEq)]
pub struct ConfirmRequest {
    /// Who asks.
    pub requester: Requester,
    /// For what.
    pub action: Action,
    /// Whether "Always Allow" is offered (not for changes to records Teitunnel didn't
    /// create, which are confirmed every time).
    pub offer_always: bool,
}

/// The person's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Allow this once.
    Once,
    /// Allow this client from now on (until revoked in Settings ▸ Integrations).
    Always,
    /// Don't allow it (also: the dialog was dismissed or timed out).
    Deny,
}

/// The app, as the control server sees it.
pub trait Host: Send + Sync + 'static {
    /// The app's name and version.
    fn app(&self) -> AppInfo;

    /// Accounts, this machine's tunnels and every share (no network).
    fn status(&self) -> BoxFuture<'_, HostResult<Status>>;

    /// Every share.
    fn shares(&self) -> BoxFuture<'_, HostResult<Vec<ShareInfo>>>;

    /// Starts a Quick Share in the app and waits (bounded) for its URL.
    fn start_share(&self, request: StartShare) -> BoxFuture<'_, HostResult<ShareInfo>>;

    /// Stops a share.
    fn stop_share(&self, request: StopShare) -> BoxFuture<'_, HostResult<()>>;

    /// Pauses (`paused`) or resumes a share on your domain or a route.
    fn pause_share(&self, request: PauseShare, paused: bool) -> BoxFuture<'_, HostResult<()>> {
        let _ = (request, paused);
        Box::pin(async {
            Err(RpcError::new(
                code::METHOD_NOT_FOUND,
                "Pausing isn't available.",
            ))
        })
    }

    /// An MCP server on connection `session` serves `agent` (until
    /// [`Self::agent_disconnected`]).
    fn agent_connected(&self, session: u64, agent: AgentInfo, client: &ClientInfo) {
        let _ = (session, agent, client);
    }

    /// Connection `session` closed.
    fn agent_disconnected(&self, session: u64) {
        let _ = session;
    }

    /// Asks the person, in the app, to approve an agent's change (the dialog shows the
    /// plan). Dismissing or a timeout is a no.
    fn approve_for_agent(&self, session: u64, request: AgentApproval) -> BoxFuture<'_, bool> {
        let _ = (session, request);
        Box::pin(async { false })
    }

    /// Asks the person, in the app, whether a client may connect to a shared MCP server
    /// (the dialog shows the code the client's browser shows). Dismissing or a timeout
    /// is a no.
    fn approve_oauth(&self, request: OAuthApproval) -> BoxFuture<'_, bool> {
        let _ = request;
        Box::pin(async { false })
    }

    /// This machine's routes in an account.
    fn routes(&self, request: RoutesParams) -> BoxFuture<'_, HostResult<RoutesList>>;

    /// Plans a change for review.
    fn preview(&self, request: PreviewParams) -> BoxFuture<'_, HostResult<PlanInfo>>;

    /// Applies a reviewed plan on behalf of `client` (recorded in Activity).
    fn apply<'a>(
        &'a self,
        request: ApplyParams,
        client: &'a ClientInfo,
    ) -> BoxFuture<'a, HostResult<ApplyResult>>;

    /// Runs the Doctor.
    fn doctor(&self) -> BoxFuture<'_, HostResult<Vec<DoctorIssue>>>;

    /// Local HTTPS domains and whether they're served.
    fn local_domains(&self) -> BoxFuture<'_, HostResult<LocalDomainsInfo>>;

    /// Serves what's in the database now (the CLI or a project changed it). It only
    /// makes the app read its own database, so it needs no approval.
    fn reload_local_domains(&self) -> BoxFuture<'_, HostResult<LocalDomainsInfo>>;

    /// Brings the app's window to a view.
    fn open(&self, view: View) -> BoxFuture<'_, HostResult<()>>;

    /// Asks the person (a native dialog in the app).
    fn confirm(&self, request: ConfirmRequest) -> BoxFuture<'_, Decision>;

    /// Whether the person allowed `client` to make changes without asking.
    fn is_approved<'a>(&'a self, client: &'a ClientInfo) -> BoxFuture<'a, bool>;

    /// Remembers that the person allowed `client` from now on.
    fn approve<'a>(&'a self, client: &'a ClientInfo) -> BoxFuture<'a, ()>;

    /// Events for subscribers.
    fn subscribe(&self) -> broadcast::Receiver<Event>;
}
