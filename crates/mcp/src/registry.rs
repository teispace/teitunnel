//! Tool providers: the extension point for tools.
//!
//! The server lists and calls tools through [`ToolProvider`]s. Teitunnel's own tools are
//! one provider ([`crate::tools::CoreTools`]); snapshots, analytics and anything else
//! add theirs with [`crate::ServerBuilder::provider`] without touching the server. A
//! provider declares each tool's [`ToolClass`]; the server then enforces the mode (a
//! `read-only` server hides everything but [`ToolClass::Read`] and
//! [`ToolClass::Wait`]), rate limits per class, timeouts and cancellation, and redacts
//! what comes back. Changes still need the person's approval: providers call
//! [`ToolContext::approve`] before changing anything.

use std::{sync::Arc, time::Duration};

use rmcp::{
    model::{
        ElicitRequestParams, ElicitationAction, ElicitationSchema, JsonObject,
        ProgressNotificationParam, ProgressToken, Tool,
    },
    service::{Peer, RoleServer},
};
use serde::Serialize;
use serde_json::Value;
use teitunnel_core::engine::Actor;
use tokio_util::sync::CancellationToken;

use crate::{
    backend::BoxFuture,
    config::{Mode, Settings},
};

/// What a tool does, for permissions and rate limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolClass {
    /// Reads only.
    Read,
    /// Waits for something to happen (bounded), reading only.
    Wait,
    /// Changes something (a share, a route); additive or reversible.
    Change,
    /// Removes or replaces something.
    Destructive,
}

impl ToolClass {
    /// Whether a `read-only` server offers it.
    pub fn read_only(self) -> bool {
        matches!(self, Self::Read | Self::Wait)
    }
}

/// A tool and its class.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    /// The tool as listed (name, description, schemas, annotations).
    pub tool: Tool,
    /// Its class.
    pub class: ToolClass,
    /// How long one call may take before it's stopped.
    pub timeout: Duration,
}

/// What a tool answered.
#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// The structured result (matches the tool's output schema).
    pub structured: Value,
    /// A short human sentence to put before the JSON, if any.
    pub summary: Option<String>,
}

impl ToolOutput {
    /// A structured result from any serializable value (messages become English).
    pub fn new(value: &impl Serialize) -> Self {
        Self {
            structured: crate::redaction::english(value),
            summary: None,
        }
    }

    /// With a sentence to show first.
    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }
}

/// Why a tool call failed, for the agent. Shown as a tool error (not a protocol error),
/// so the model reads the message and can correct itself.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ToolError {
    /// What went wrong and what to do.
    pub message: String,
}

impl ToolError {
    /// An error with a message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl From<crate::backend::BackendError> for ToolError {
    fn from(err: crate::backend::BackendError) -> Self {
        Self::new(err.to_string())
    }
}

impl From<crate::traffic::TrafficError> for ToolError {
    fn from(err: crate::traffic::TrafficError) -> Self {
        Self::new(err.to_string())
    }
}

/// A tool's result.
pub type ToolResult = Result<ToolOutput, ToolError>;

/// Supplies tools.
pub trait ToolProvider: Send + Sync + 'static {
    /// The tools it offers (names must be unique across providers).
    fn tools(&self) -> Vec<ToolSpec>;

    /// Runs one of its tools.
    fn call<'a>(
        &'a self,
        name: &'a str,
        arguments: JsonObject,
        ctx: &'a ToolContext,
    ) -> BoxFuture<'a, ToolResult>;
}

/// Asks the person outside MCP (e.g. a dialog in the desktop app). `None`: it can't
/// ask right now, so the server falls back to asking through the client.
pub trait Approver: Send + Sync + 'static {
    /// Asks the person to approve `request` made by `actor`.
    fn approve<'a>(
        &'a self,
        actor: &'a Actor,
        request: &'a ApprovalRequest,
    ) -> BoxFuture<'a, Option<bool>>;

    /// An agent connected (its MCP client finished initializing).
    fn agent_connected<'a>(&'a self, actor: &'a Actor) -> BoxFuture<'a, ()> {
        let _ = actor;
        Box::pin(async {})
    }
}

/// What needs approving.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalRequest {
    /// One line, e.g. "Add app.teispace.com → http://localhost:3000".
    pub title: String,
    /// The plan or details, as the person should read them.
    pub details: String,
    /// The agent passed `confirmed: true` (counts only when nobody can be asked).
    pub confirmed: bool,
}

/// The answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Approval {
    /// Go ahead. `how` says who approved (for the record).
    Granted {
        /// `mode`, `person` or `confirmed`.
        how: &'static str,
    },
    /// The person said no (or dismissed the question).
    Declined(String),
    /// Nobody could be asked: show the details to the person and call again with
    /// `confirmed: true` once they agree.
    NeedsConfirmation,
}

/// How long the person has to answer an approval question.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// One tool call's context: the server's settings, who's calling, cancellation,
/// progress and approvals.
#[derive(Clone)]
pub struct ToolContext {
    settings: Settings,
    actor: Actor,
    cancel: CancellationToken,
    peer: Option<Peer<RoleServer>>,
    progress: Option<ProgressToken>,
    elicitation: bool,
    approver: Option<Arc<dyn Approver>>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("settings", &self.settings)
            .field("actor", &self.actor)
            .field("elicitation", &self.elicitation)
            .finish_non_exhaustive()
    }
}

impl ToolContext {
    /// A context without a client (tests, or calls from outside MCP): no progress, and
    /// nobody to ask.
    pub fn detached(settings: Settings, actor: Actor) -> Self {
        Self {
            settings,
            actor,
            cancel: CancellationToken::new(),
            peer: None,
            progress: None,
            elicitation: false,
            approver: None,
        }
    }

    /// A context without a client whose approvals go to `approver` (tests).
    #[cfg(test)]
    pub(crate) fn with_approver(
        settings: Settings,
        actor: Actor,
        approver: Arc<dyn Approver>,
    ) -> Self {
        Self {
            approver: Some(approver),
            ..Self::detached(settings, actor)
        }
    }

    /// A context for a call from `peer`.
    pub(crate) fn for_call(
        settings: Settings,
        actor: Actor,
        cancel: CancellationToken,
        peer: Peer<RoleServer>,
        progress: Option<ProgressToken>,
        elicitation: bool,
        approver: Option<Arc<dyn Approver>>,
    ) -> Self {
        Self {
            settings,
            actor,
            cancel,
            peer: Some(peer),
            progress,
            elicitation,
            approver,
        }
    }

    /// The server's permission mode.
    pub fn mode(&self) -> Mode {
        self.settings.mode
    }

    /// Whether secrets may be shown.
    pub fn allow_secrets(&self) -> bool {
        self.settings.allow_secrets
    }

    /// Who's calling (recorded with every change).
    pub fn actor(&self) -> &Actor {
        &self.actor
    }

    /// Cancelled when the client cancels the call (or it times out).
    pub fn cancelled(&self) -> &CancellationToken {
        &self.cancel
    }

    /// Reports progress, if the client asked for it (`progress` of `total`).
    pub async fn progress(&self, progress: f64, total: Option<f64>, message: impl Into<String>) {
        let (Some(peer), Some(token)) = (&self.peer, &self.progress) else {
            return;
        };
        let mut param =
            ProgressNotificationParam::new(token.clone(), progress).with_message(message);
        if let Some(total) = total {
            param = param.with_total(total);
        }
        let _ = peer.notify_progress(param).await;
    }

    /// A handle that reports progress from synchronous code (e.g. the engine's progress
    /// callback): messages are forwarded in order on a task.
    pub fn progress_sender(
        &self,
    ) -> Option<tokio::sync::mpsc::UnboundedSender<(f64, Option<f64>, String)>> {
        let (Some(peer), Some(token)) = (self.peer.clone(), self.progress.clone()) else {
            return None;
        };
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(f64, Option<f64>, String)>();
        tokio::spawn(async move {
            while let Some((progress, total, message)) = rx.recv().await {
                let mut param =
                    ProgressNotificationParam::new(token.clone(), progress).with_message(message);
                if let Some(total) = total {
                    param = param.with_total(total);
                }
                let _ = peer.notify_progress(param).await;
            }
        });
        Some(tx)
    }

    /// Whether the client can ask the person (MCP elicitation, form mode).
    pub fn can_ask(&self) -> bool {
        self.elicitation && self.peer.is_some()
    }

    /// Gets the person's go-ahead for a change, per the server's mode:
    /// - `full`: granted.
    /// - `read-only`: declined.
    /// - `ask`: the host's own approver (e.g. the desktop app) if it can ask; otherwise
    ///   the client (MCP elicitation) if it supports it; otherwise `confirmed: true` from
    ///   the agent counts, after it showed the person the details; without it, the
    ///   answer is [`Approval::NeedsConfirmation`]. An agent can never approve its own
    ///   change while the person can be asked.
    pub async fn approve(&self, request: &ApprovalRequest) -> Approval {
        match self.settings.mode {
            Mode::Full => return Approval::Granted { how: "mode" },
            Mode::ReadOnly => {
                return Approval::Declined(
                    "This Teitunnel MCP server is read-only; it can't change anything.".into(),
                );
            }
            Mode::Ask => {}
        }
        if let Some(approver) = &self.approver
            && let Some(answer) = approver.approve(&self.actor, request).await
        {
            return if answer {
                Approval::Granted { how: "person" }
            } else {
                Approval::Declined("The person declined it in Teitunnel.".into())
            };
        }
        if self.can_ask()
            && let Some(answer) = self.elicit(request).await
        {
            return answer;
        }
        if request.confirmed {
            Approval::Granted { how: "confirmed" }
        } else {
            Approval::NeedsConfirmation
        }
    }

    /// Asks through the client. `None` if the client couldn't ask.
    async fn elicit(&self, request: &ApprovalRequest) -> Option<Approval> {
        let peer = self.peer.as_ref()?;
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "approve": {
                    "type": "boolean",
                    "title": "Approve this change",
                    "description": "Teitunnel applies it only if you approve.",
                    "default": false
                }
            },
            "required": ["approve"]
        });
        let Value::Object(schema) = schema else {
            return None;
        };
        let schema = ElicitationSchema::from_json_schema(schema).ok()?;
        let message = format!(
            "{} asks Teitunnel to: {}\n\n{}",
            self.actor.client, request.title, request.details
        );
        let params = ElicitRequestParams::FormElicitationParams {
            meta: None,
            message: crate::limits::truncate(message),
            requested_schema: schema,
        };
        let result = tokio::select! {
            result = peer.create_elicitation_with_timeout(params, Some(APPROVAL_TIMEOUT)) => result.ok()?,
            () = self.cancel.cancelled() => return Some(Approval::Declined("The request was cancelled.".into())),
        };
        Some(match result.action {
            ElicitationAction::Accept
                if result
                    .content
                    .as_ref()
                    .and_then(|c| c.get("approve"))
                    .and_then(Value::as_bool)
                    == Some(true) =>
            {
                Approval::Granted { how: "person" }
            }
            ElicitationAction::Accept | ElicitationAction::Decline => {
                Approval::Declined("The person declined this change.".into())
            }
            _ => Approval::Declined("The person dismissed the question without approving.".into()),
        })
    }
}

/// Parses a tool's arguments into `T`, with a message the model can act on.
///
/// # Errors
/// The arguments don't match the tool's input schema.
pub fn arguments<T: serde::de::DeserializeOwned>(arguments: JsonObject) -> Result<T, ToolError> {
    serde_json::from_value(Value::Object(arguments)).map_err(|e| {
        ToolError::new(format!(
            "Invalid arguments: {e}. Check the tool's input schema."
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor() -> Actor {
        Actor {
            via: "mcp".into(),
            client: "test".into(),
            version: None,
        }
    }

    fn request(confirmed: bool) -> ApprovalRequest {
        ApprovalRequest {
            title: "Add a.xyz.com".into(),
            details: "1. Create DNS record".into(),
            confirmed,
        }
    }

    fn ctx(mode: Mode) -> ToolContext {
        ToolContext::detached(
            Settings {
                mode,
                allow_secrets: false,
            },
            actor(),
        )
    }

    struct Always(Option<bool>);

    impl Approver for Always {
        fn approve<'a>(
            &'a self,
            _actor: &'a Actor,
            _request: &'a ApprovalRequest,
        ) -> BoxFuture<'a, Option<bool>> {
            let answer = self.0;
            Box::pin(async move { answer })
        }
    }

    #[tokio::test]
    async fn approval_follows_the_mode() {
        assert_eq!(
            ctx(Mode::Full).approve(&request(false)).await,
            Approval::Granted { how: "mode" }
        );
        assert!(matches!(
            ctx(Mode::ReadOnly).approve(&request(true)).await,
            Approval::Declined(_)
        ));
        // Ask, nobody to ask: only an explicit second call with `confirmed` goes ahead.
        assert_eq!(
            ctx(Mode::Ask).approve(&request(false)).await,
            Approval::NeedsConfirmation
        );
        assert_eq!(
            ctx(Mode::Ask).approve(&request(true)).await,
            Approval::Granted { how: "confirmed" }
        );
    }

    #[tokio::test]
    async fn the_hosts_approver_decides_when_it_can_ask() {
        let mut no = ctx(Mode::Ask);
        no.approver = Some(Arc::new(Always(Some(false))));
        assert!(
            matches!(no.approve(&request(true)).await, Approval::Declined(_)),
            "the agent's `confirmed` doesn't override the person"
        );
        let mut yes = ctx(Mode::Ask);
        yes.approver = Some(Arc::new(Always(Some(true))));
        assert_eq!(
            yes.approve(&request(false)).await,
            Approval::Granted { how: "person" }
        );
        let mut away = ctx(Mode::Ask);
        away.approver = Some(Arc::new(Always(None)));
        assert_eq!(
            away.approve(&request(false)).await,
            Approval::NeedsConfirmation
        );
    }
}
