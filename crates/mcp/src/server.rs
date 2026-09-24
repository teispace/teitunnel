//! The MCP server: lists and calls tools from its providers (enforcing the mode, rate
//! limits, timeouts, cancellation and redaction), serves resources and prompts, and
//! notifies subscribers when routes or shares change.

// Logging notifications are deprecated from protocol 2026-07-28 (SEP-2577) but still
// what clients on 2025-11-25 use; they're sent only to a client that set a level.
#![allow(deprecated)]

use std::{
    collections::HashSet,
    sync::{Arc, Mutex, PoisonError},
};

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CompleteRequestParams,
        CompleteResult, CompletionInfo, ContentBlock, GetPromptRequestParams, GetPromptResponse,
        Implementation, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
        ListToolsResult, LoggingLevel, LoggingMessageNotificationParam, PaginatedRequestParams,
        ReadResourceRequestParams, ReadResourceResponse, ResourceUpdatedNotificationParam,
        ServerCapabilities, ServerConfig, SetLevelRequestParams, SubscribeRequestParams,
        SubscriptionFilter, Tool, UnsubscribeRequestParams,
    },
    service::{NotificationContext, Peer, RequestContext, SubscriptionContext},
};
use teitunnel_core::engine::Actor;

use crate::{
    backend::{ChangeEvent, SharedBackend},
    config::{Mode, Settings},
    limits::{self, RateLimiter},
    plans::Plans,
    prompts, redaction,
    registry::{Approver, ToolClass, ToolContext, ToolProvider, ToolSpec},
    resources,
    tools::CoreTools,
    traffic::{NoTraffic, TrafficSource},
};

/// What agents are told when they connect (the `instructions` of the initialize
/// result): how to use Teitunnel well.
pub const INSTRUCTIONS: &str = "Teitunnel puts services running on this machine on the internet through the person's own Cloudflare account (Cloudflare Tunnel), and manages their routes (public hostname → local service), logins, DNS and connectors.\n\
\n\
How to work with it:\n\
- To show something running locally (a dev server, a webhook receiver): list_local_services to find the port, then share_port. Without a hostname it's a public trycloudflare.com URL (no account needed); with a hostname on the person's domain it can require a login (allow). Shares end when stopped, when they expire, or when this session ends.\n\
- Permanent routes and every other Cloudflare change go through plans: plan_change returns the exact steps; show them (and any warnings) to the person; then apply_plan with the plan's id and fingerprint. Never apply a plan the person hasn't seen. undo_last plans the reverse of a change.\n\
- When something doesn't work: doctor (issues with fixes), verify_route (where a hostname breaks), logs_tail (this machine's connector) or remote_logs (another machine's), connector_status. fix_issue applies a fix.\n\
- Webhooks: share_port, give the URL to the sender, wait_for_request (blocks until it arrives), traffic_get to inspect it, fix the handler, traffic_replay to resend it.\n\
- Moving routes elsewhere: export_config (config.yml, Docker Compose, Terraform).\n\
\n\
Safety: in `ask` mode (the default) every change needs the person's approval. When the client can ask them, Teitunnel does; otherwise a tool answers `needsApproval`: show the person what it says and call again with `confirmed: true` only after they agree. In `read-only` mode tools that change things aren't offered. Secrets (tokens, credentials, Authorization and Cookie headers) never reach you. Every change is recorded in Teitunnel's Activity under your client's name, where the person can see and undo it.\n\
\n\
Resources: teitunnel://routes, teitunnel://shares, teitunnel://domains, teitunnel://issues, teitunnel://activity, teitunnel://route/{hostname}, teitunnel://logs/{tunnel}. Prompts: put_online, debug_webhook, route_down, move_to_server.";

/// Builds a server.
pub struct ServerBuilder {
    backend: SharedBackend,
    settings: Settings,
    traffic: Arc<dyn TrafficSource>,
    providers: Vec<Arc<dyn ToolProvider>>,
    approver: Option<Arc<dyn Approver>>,
    via: String,
}

impl std::fmt::Debug for ServerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerBuilder")
            .field("settings", &self.settings)
            .field("via", &self.via)
            .finish_non_exhaustive()
    }
}

impl ServerBuilder {
    /// A server over `backend` with `settings`, no traffic source yet (the traffic tools
    /// say the inspector isn't running), and Teitunnel's own tools.
    pub fn new(backend: SharedBackend, settings: Settings) -> Self {
        Self {
            backend,
            settings,
            traffic: Arc::new(NoTraffic),
            providers: Vec::new(),
            approver: None,
            via: "mcp".into(),
        }
    }

    /// Where captured traffic comes from (the inspector).
    #[must_use]
    pub fn traffic(mut self, traffic: Arc<dyn TrafficSource>) -> Self {
        self.traffic = traffic;
        self
    }

    /// Adds a tool provider (e.g. snapshots, analytics). Its tools are listed after
    /// Teitunnel's own; a tool whose name is already taken is left out.
    #[must_use]
    pub fn provider(mut self, provider: Arc<dyn ToolProvider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// Asks the person outside MCP before changes (e.g. the desktop app's dialog).
    #[must_use]
    pub fn approver(mut self, approver: Arc<dyn Approver>) -> Self {
        self.approver = Some(approver);
        self
    }

    /// How this server is reached, recorded with changes (`mcp`, `mcp over HTTP`).
    #[must_use]
    pub fn via(mut self, via: impl Into<String>) -> Self {
        self.via = via.into();
        self
    }

    /// The server.
    pub fn build(self) -> McpServer {
        let plans = Arc::new(Plans::default());
        let core: Arc<dyn ToolProvider> = Arc::new(CoreTools::new(
            Arc::clone(&self.backend),
            Arc::clone(&self.traffic),
            Arc::clone(&plans),
        ));
        // Edge protection plans are applied with apply_plan, so they share the plans.
        let protection: Arc<dyn ToolProvider> = Arc::new(crate::tools::ProtectionTools::new(
            Arc::clone(&self.backend),
            plans,
        ));
        let mut providers = vec![core, protection];
        providers.extend(self.providers);
        let mut tools: Vec<(ToolSpec, usize)> = Vec::new();
        let mut names = HashSet::new();
        for (index, provider) in providers.iter().enumerate() {
            for spec in provider.tools() {
                if names.insert(spec.tool.name.to_string()) {
                    tools.push((spec, index));
                } else {
                    tracing::warn!(tool = %spec.tool.name, "a tool with this name already exists; left out");
                }
            }
        }
        McpServer {
            shared: Arc::new(Shared {
                backend: self.backend,
                settings: self.settings,
                providers,
                tools,
                limiter: RateLimiter::default(),
                approver: self.approver,
                via: self.via,
            }),
            session: Arc::new(SessionState::default()),
        }
    }
}

struct Shared {
    backend: SharedBackend,
    settings: Settings,
    providers: Vec<Arc<dyn ToolProvider>>,
    tools: Vec<(ToolSpec, usize)>,
    limiter: RateLimiter,
    approver: Option<Arc<dyn Approver>>,
    via: String,
}

/// One client connection's state.
#[derive(Default)]
struct SessionState {
    /// Resource URIs subscribed to (legacy `resources/subscribe`).
    subscriptions: Mutex<HashSet<String>>,
    /// The forwarding task for subscriptions has started.
    forwarding: Mutex<bool>,
    /// Log messages at or above this level are sent (after `logging/setLevel`).
    log_level: Mutex<Option<LoggingLevel>>,
}

/// Teitunnel's MCP server. Cheap to clone; [`McpServer::session`] gives each
/// connection its own subscriptions and log level.
#[derive(Clone)]
pub struct McpServer {
    shared: Arc<Shared>,
    session: Arc<SessionState>,
}

impl std::fmt::Debug for McpServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpServer")
            .field("settings", &self.shared.settings)
            .finish_non_exhaustive()
    }
}

/// An HTTP caller's identity (the API key's name), put in request extensions by the
/// HTTP transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpIdentity {
    /// The API key's name.
    pub key_name: String,
}

fn severity(level: LoggingLevel) -> u8 {
    match level {
        LoggingLevel::Debug => 0,
        LoggingLevel::Info => 1,
        LoggingLevel::Notice => 2,
        LoggingLevel::Warning => 3,
        LoggingLevel::Error => 4,
        LoggingLevel::Critical => 5,
        LoggingLevel::Alert => 6,
        LoggingLevel::Emergency => 7,
    }
}

impl McpServer {
    /// A server over `backend`: see [`ServerBuilder`].
    pub fn builder(backend: SharedBackend, settings: Settings) -> ServerBuilder {
        ServerBuilder::new(backend, settings)
    }

    /// The same server for another connection.
    #[must_use]
    pub fn session(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
            session: Arc::new(SessionState::default()),
        }
    }

    /// The server's settings.
    pub fn settings(&self) -> &Settings {
        &self.shared.settings
    }

    /// The backend.
    pub fn backend(&self) -> &SharedBackend {
        &self.shared.backend
    }

    /// The tools this server offers in its mode.
    pub fn tools(&self) -> Vec<Tool> {
        self.offered().map(|(spec, _)| spec.tool.clone()).collect()
    }

    fn offered(&self) -> impl Iterator<Item = &(ToolSpec, usize)> {
        let read_only = self.shared.settings.mode == Mode::ReadOnly;
        self.shared
            .tools
            .iter()
            .filter(move |(spec, _)| !read_only || spec.class.read_only())
    }

    fn actor(&self, context: &RequestContext<RoleServer>) -> Actor {
        let info = context.client_info();
        let key = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.extensions.get::<HttpIdentity>())
            .map(|id| id.key_name.clone());
        Actor {
            via: match key {
                Some(key) => format!("{} (API key \"{key}\")", self.shared.via),
                None => self.shared.via.clone(),
            },
            client: info
                .as_ref()
                .map(|i| i.title.clone().unwrap_or_else(|| i.name.clone()))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "an AI agent".to_owned()),
            version: info.map(|i| i.version).filter(|v| !v.is_empty()),
        }
    }

    async fn log(&self, peer: &Peer<RoleServer>, level: LoggingLevel, message: String) {
        let wanted = *self
            .session
            .log_level
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let Some(wanted) = wanted else { return };
        if severity(level) < severity(wanted) {
            return;
        }
        #[allow(deprecated)]
        let _ = peer
            .notify_logging_message(
                LoggingMessageNotificationParam::new(level, serde_json::Value::String(message))
                    .with_logger("teitunnel"),
            )
            .await;
    }

    /// Runs one tool call.
    async fn run_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let name = request.name.to_string();
        let Some((spec, index)) = self.shared.tools.iter().find(|(s, _)| s.tool.name == name)
        else {
            return error(format!("There's no tool called {name}."));
        };
        let settings = self.shared.settings.clone();
        if settings.mode == Mode::ReadOnly && !spec.class.read_only() {
            return error(format!(
                "{name} changes things, and this Teitunnel MCP server is read-only. The person can start it in `ask` mode (`teitunnel mcp --mode ask`) to allow changes with their approval."
            ));
        }
        if let Err(wait) = self.shared.limiter.check(spec.class) {
            return error(format!(
                "Too many calls of this kind in the last minute. Try again in {} s.",
                wait.as_secs().max(1)
            ));
        }
        let Some(provider) = self.shared.providers.get(*index) else {
            return error(format!("There's no tool called {name}."));
        };
        let actor = self.actor(&context);
        let elicitation = context
            .client_capabilities()
            .is_some_and(|c| c.elicitation.is_some());
        let ctx = ToolContext::for_call(
            settings.clone(),
            actor.clone(),
            context.ct.child_token(),
            context.peer.clone(),
            context.meta.get_progress_token(),
            elicitation,
            self.shared.approver.clone(),
        );
        let arguments = request.arguments.unwrap_or_default();
        let call = provider.call(&name, arguments, &ctx);
        let outcome = tokio::select! {
            result = tokio::time::timeout(spec.timeout, call) => match result {
                Ok(result) => result,
                Err(_) => {
                    ctx.cancelled().cancel();
                    Err(crate::registry::ToolError::new(format!(
                        "{name} took longer than {} s and was stopped. Anything it had applied is in recent_activity.",
                        spec.timeout.as_secs()
                    )))
                }
            },
            () = context.ct.cancelled() => Err(crate::registry::ToolError::new("Cancelled.")),
        };
        match outcome {
            Ok(output) => {
                if matches!(spec.class, ToolClass::Change | ToolClass::Destructive) {
                    let what = output
                        .structured
                        .get("outcome")
                        .and_then(|o| o.as_str())
                        .unwrap_or("done");
                    self.log(
                        &context.peer,
                        LoggingLevel::Info,
                        format!("{name} for {}: {what}", actor.client),
                    )
                    .await;
                }
                let structured = redaction::value(output.structured, settings.allow_secrets);
                let json = serde_json::to_string_pretty(&structured).unwrap_or_default();
                let text = match output.summary {
                    Some(summary) => format!(
                        "{}\n\n{json}",
                        redaction::text(&summary, settings.allow_secrets)
                    ),
                    None => json,
                };
                let mut result =
                    CallToolResult::success(vec![ContentBlock::text(limits::truncate(text))]);
                result.structured_content = Some(structured);
                result
            }
            Err(err) => {
                self.log(
                    &context.peer,
                    LoggingLevel::Warning,
                    format!("{name} for {} failed: {}", actor.client, err.message),
                )
                .await;
                error(redaction::text(&err.message, settings.allow_secrets))
            }
        }
    }

    /// Starts forwarding change events to this session's subscribers (legacy clients).
    fn forward_updates(&self, peer: Peer<RoleServer>) {
        {
            let mut started = self
                .session
                .forwarding
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if *started {
                return;
            }
            *started = true;
        }
        let mut events = self.shared.backend.subscribe();
        let session = Arc::clone(&self.session);
        tokio::spawn(async move {
            loop {
                let event = match events.recv().await {
                    Ok(event) => event,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => ChangeEvent::Routes,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                };
                let uris: Vec<String> = {
                    let subscriptions = session
                        .subscriptions
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner);
                    subscriptions
                        .iter()
                        .filter(|uri| resources::affected(uri, event))
                        .cloned()
                        .collect()
                };
                for uri in uris {
                    if peer
                        .notify_resource_updated(ResourceUpdatedNotificationParam::new(uri))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
            }
        });
    }
}

fn error(message: String) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message)])
}

impl ServerHandler for McpServer {
    async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
        let Some(approver) = self.shared.approver.clone() else {
            return;
        };
        let info = context.peer.peer_info().map(|p| p.client_info.clone());
        let actor = Actor {
            via: self.shared.via.clone(),
            client: info
                .as_ref()
                .map(|i| i.title.clone().unwrap_or_else(|| i.name.clone()))
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| "an AI agent".to_owned()),
            version: info.map(|i| i.version).filter(|v| !v.is_empty()),
        };
        // In the background: the app may take a moment to answer.
        tokio::spawn(async move { approver.agent_connected(&actor).await });
    }

    fn get_info(&self) -> ServerConfig {
        let capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .enable_resources_subscribe()
            .enable_prompts()
            .enable_logging()
            .enable_completions()
            .build();
        ServerConfig::new(capabilities)
            .with_server_info(
                Implementation::new("teitunnel", env!("CARGO_PKG_VERSION")).with_title("Teitunnel"),
            )
            .with_instructions(format!(
                "{INSTRUCTIONS}\n\nThis server's mode: {}.",
                self.shared.settings.mode
            ))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult::with_all_items(self.tools()))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        self.offered()
            .find(|(spec, _)| spec.tool.name == name)
            .map(|(spec, _)| spec.tool.clone())
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        Ok(CallToolResponse::Complete(
            self.run_tool(request, context).await,
        ))
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(resources::list()))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(
            resources::templates(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        resources::read(
            &self.shared.backend,
            &request.uri,
            self.shared.settings.allow_secrets,
        )
        .await
        .map(ReadResourceResponse::from)
    }

    #[allow(deprecated)]
    async fn subscribe(
        &self,
        request: SubscribeRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        if !resources::is_ours(&request.uri) {
            return Err(McpError::resource_not_found(
                format!("No resource {}", request.uri),
                None,
            ));
        }
        self.session
            .subscriptions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(request.uri);
        self.forward_updates(context.peer.clone());
        Ok(())
    }

    #[allow(deprecated)]
    async fn unsubscribe(
        &self,
        request: UnsubscribeRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        self.session
            .subscriptions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&request.uri);
        Ok(())
    }

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        let uris: Vec<String> = requested
            .resource_subscriptions
            .iter()
            .flatten()
            .filter(|uri| resources::is_ours(uri))
            .cloned()
            .collect();
        let mut accepted = SubscriptionFilter::new();
        if !uris.is_empty() {
            accepted.resource_subscriptions = Some(uris);
        }
        Some(accepted)
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), McpError> {
        let mut events = self.shared.backend.subscribe();
        let uris: Vec<String> = context
            .accepted()
            .resource_subscriptions
            .clone()
            .unwrap_or_default();
        loop {
            tokio::select! {
                () = context.cancelled() => return Ok(()),
                event = events.recv() => {
                    let event = match event {
                        Ok(event) => event,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => ChangeEvent::Routes,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => return Ok(()),
                    };
                    for uri in uris.iter().filter(|uri| resources::affected(uri, event)) {
                        if context.sink().notify_resource_updated(uri.clone()).await.is_err() {
                            return Ok(());
                        }
                    }
                }
            }
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(prompts::list()))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        prompts::get(
            &request.name,
            request.arguments.as_ref(),
            self.shared.settings.mode,
        )
        .map(GetPromptResponse::from)
    }

    async fn set_level(
        &self,
        request: SetLevelRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), McpError> {
        *self
            .session
            .log_level
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(request.level);
        Ok(())
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, McpError> {
        let values = resources::complete(
            &self.shared.backend,
            &request.argument.name,
            &request.argument.value,
        )
        .await;
        let info = CompletionInfo::with_all_values(values)
            .unwrap_or_else(|_| CompletionInfo::with_all_values(Vec::new()).unwrap_or_default());
        Ok(CompleteResult::new(info))
    }
}
