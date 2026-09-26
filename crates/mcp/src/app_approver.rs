//! Approvals in the Teitunnel app: while the app runs, `teitunnel mcp` asks the
//! person there, in a native dialog showing the plan, over the control connection
//! (`crates/control`, `agent.approve`), and the app lists the agent in Settings ▸ AI
//! Tools while it's connected (`agent.register`). Without the app, [`AppApprover`]
//! answers "can't ask", so the server falls back to MCP elicitation (the agent's own
//! prompt), or tells the agent the person must open Teitunnel (`approveInApp`, the
//! default), or accepts a confirmed second call (with `approveInApp` off). While the app
//! shows another question, it answers "busy", which is not a no.
//!
//! The agent keeps its stdio configuration; nothing listens on a new port, and the
//! control connection is already authenticated and limited to the current user.

use std::{
    path::Path,
    sync::{Arc, Mutex, PoisonError},
};

use teitunnel_control::{
    ClientError, ControlClient, Endpoint,
    protocol::{AgentApproval, AgentInfo, ClientInfo, code},
};
use teitunnel_core::engine::Actor;

use crate::{
    backend::BoxFuture,
    config::Mode,
    registry::{AppAnswer, ApprovalRequest, Approver},
};

/// Asks in the app when it runs.
#[derive(Debug)]
pub struct AppApprover {
    endpoint: Endpoint,
    mode: Mode,
    client: tokio::sync::Mutex<Option<Arc<ControlClient>>>,
    agent: Mutex<Option<AgentInfo>>,
}

/// How the MCP server introduces itself on the control connection.
fn client_info() -> ClientInfo {
    ClientInfo {
        name: "teitunnel-mcp".into(),
        version: env!("CARGO_PKG_VERSION").into(),
    }
}

impl AppApprover {
    /// An approver for the app of the data folder `data_dir`, for a server in `mode`.
    pub fn new(data_dir: &Path, mode: Mode) -> Arc<Self> {
        Arc::new(Self {
            endpoint: Endpoint::new(data_dir),
            mode,
            client: tokio::sync::Mutex::default(),
            agent: Mutex::default(),
        })
    }

    fn agent_info(&self) -> Option<AgentInfo> {
        self.agent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The connection to the app, opened (and the agent registered) when needed. `None`
    /// when the app isn't running or refuses.
    async fn connection(&self) -> Option<Arc<ControlClient>> {
        let mut client = self.client.lock().await;
        if let Some(existing) = client.as_ref() {
            return Some(Arc::clone(existing));
        }
        let connected = Arc::new(
            ControlClient::connect(&self.endpoint, client_info())
                .await
                .ok()?,
        );
        if let Some(agent) = self.agent_info() {
            let _ = connected.register_agent(&agent).await;
        }
        *client = Some(Arc::clone(&connected));
        Some(connected)
    }

    async fn forget_connection(&self) {
        *self.client.lock().await = None;
    }

    /// Tells the app which agent this server serves (shown in Settings ▸ AI Tools while
    /// connected). Does nothing when the app isn't running.
    pub async fn register(&self, actor: &Actor) {
        let agent = AgentInfo {
            name: actor.client.chars().take(64).collect(),
            version: actor.version.as_ref().map(|v| v.chars().take(64).collect()),
            mode: self.mode.to_string(),
        };
        *self.agent.lock().unwrap_or_else(PoisonError::into_inner) = Some(agent.clone());
        if let Some(client) = self.connection().await
            && client.register_agent(&agent).await.is_err()
        {
            self.forget_connection().await;
        }
    }

    /// Asks the person in the app. [`AppAnswer::Unavailable`]: the app can't ask (not
    /// running, the control connection is off, or it broke), so the caller asks some
    /// other way.
    pub async fn ask(&self, actor: &Actor, request: &ApprovalRequest) -> AppAnswer {
        let Some(client) = self.connection().await else {
            return AppAnswer::Unavailable;
        };
        let question = AgentApproval {
            agent: actor.client.clone(),
            title: request.title.clone(),
            details: request.details.clone(),
        };
        let answer = answer(client.approve_for_agent(&question).await);
        if answer == AppAnswer::Unavailable {
            self.forget_connection().await;
        }
        answer
    }
}

/// What the app's reply to `agent.approve` means.
fn answer(reply: Result<bool, ClientError>) -> AppAnswer {
    match reply {
        Ok(true) => AppAnswer::Approved,
        // The person said no, or didn't answer in time: that's a no.
        Ok(false) => AppAnswer::Declined,
        Err(ClientError::Rpc(error)) if error.code == code::TIMEOUT => AppAnswer::Declined,
        // One question is shown at a time: another is waiting for the person. That's
        // not their answer to this one.
        Err(ClientError::Rpc(error)) if error.code == code::RATE_LIMITED => AppAnswer::Busy,
        Err(_) => AppAnswer::Unavailable,
    }
}

impl Approver for AppApprover {
    fn approve<'a>(
        &'a self,
        actor: &'a Actor,
        request: &'a ApprovalRequest,
    ) -> BoxFuture<'a, AppAnswer> {
        Box::pin(self.ask(actor, request))
    }

    fn agent_connected<'a>(&'a self, actor: &'a Actor) -> BoxFuture<'a, ()> {
        Box::pin(self.register(actor))
    }
}

#[cfg(test)]
mod tests {
    use teitunnel_control::{Limits, testing};

    use super::*;

    fn actor() -> Actor {
        Actor {
            via: "mcp".into(),
            client: "claude-code".into(),
            version: Some("2.1".into()),
        }
    }

    fn request() -> ApprovalRequest {
        ApprovalRequest {
            title: "Add app.example.com".into(),
            details: "1. Create a DNS record".into(),
            confirmed: false,
        }
    }

    #[tokio::test]
    async fn asks_in_the_app_when_it_runs() {
        let dir = tempfile::tempdir().unwrap();
        let running = testing::serve(dir.path(), Limits::default()).await.unwrap();
        let approver = AppApprover::new(dir.path(), Mode::Ask);
        approver.register(&actor()).await;
        {
            let agents = running.host.agents.lock().unwrap();
            assert_eq!(agents[0].1.name, "claude-code");
            assert_eq!(agents[0].1.mode, "ask");
        }
        running.host.agent_answers.lock().unwrap().push_back(true);
        assert_eq!(
            approver.ask(&actor(), &request()).await,
            AppAnswer::Approved
        );
        assert_eq!(
            approver.ask(&actor(), &request()).await,
            AppAnswer::Declined,
            "no answer is a no"
        );
        let questions = running.host.agent_questions.lock().unwrap();
        assert_eq!(questions[0].agent, "claude-code");
        assert_eq!(questions[0].details, "1. Create a DNS record");
    }

    #[tokio::test]
    async fn cant_ask_without_the_app() {
        let dir = tempfile::tempdir().unwrap();
        let approver = AppApprover::new(dir.path(), Mode::Ask);
        approver.register(&actor()).await;
        assert_eq!(
            approver.ask(&actor(), &request()).await,
            AppAnswer::Unavailable
        );
    }

    #[test]
    fn another_open_question_isnt_a_no() {
        use teitunnel_control::protocol::RpcError;
        let rpc = |code| Err(ClientError::Rpc(RpcError::new(code, "…")));
        assert_eq!(answer(rpc(code::RATE_LIMITED)), AppAnswer::Busy);
        assert_eq!(answer(rpc(code::TIMEOUT)), AppAnswer::Declined);
        assert_eq!(answer(rpc(code::DISABLED)), AppAnswer::Unavailable);
        assert_eq!(answer(rpc(code::METHOD_NOT_FOUND)), AppAnswer::Unavailable);
        assert_eq!(answer(Ok(false)), AppAnswer::Declined);
        assert_eq!(answer(Ok(true)), AppAnswer::Approved);
    }

    #[tokio::test]
    async fn the_person_decides_through_the_server() {
        use crate::{
            config::Settings,
            registry::{Approval, ToolContext},
        };
        let dir = tempfile::tempdir().unwrap();
        let running = testing::serve(dir.path(), Limits::default()).await.unwrap();
        let approver = AppApprover::new(dir.path(), Mode::Ask);
        let ctx = ToolContext::with_approver(
            Settings {
                mode: Mode::Ask,
                ..Settings::default()
            },
            actor(),
            approver,
        );
        // Even `confirmed: true` from the agent doesn't count while the person can be
        // asked in the app.
        let confirmed = ApprovalRequest {
            confirmed: true,
            ..request()
        };
        assert!(matches!(
            ctx.approve(&confirmed).await,
            Approval::Declined(_)
        ));
        running.host.agent_answers.lock().unwrap().push_back(true);
        assert_eq!(
            ctx.approve(&request()).await,
            Approval::Granted { how: "app" }
        );
    }
}
