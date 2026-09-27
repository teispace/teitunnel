//! The server as clients see it, over an in-memory transport with rmcp's own client:
//! the handshake, tool listing per mode, calls with structured output, elicitation
//! approvals, progress, resources and subscriptions, prompts.

use std::sync::{Arc, Mutex, PoisonError};

use rmcp::{
    ClientHandler, ErrorData as McpError, RoleClient, ServiceExt,
    model::{
        CallToolRequestParams, ClientCapabilities, ClientConfig, ElicitRequestParams, ElicitResult,
        ElicitationAction, GetPromptRequestParams, Implementation, NumberOrString,
        ProgressNotificationParam, ProgressToken, ProtocolVersion, ReadResourceRequestParams,
        RequestMetaObject, ResourceUpdatedNotificationParam, SubscribeRequestParams,
    },
    service::{NotificationContext, RequestContext, RunningService},
};
use serde_json::{Value, json};

use crate::{
    McpServer,
    backend::SharedBackend,
    config::Mode,
    tools::tests::{FakeBackend, settings},
};

#[derive(Clone, Default)]
struct Seen {
    progress: Arc<Mutex<Vec<String>>>,
    updated: Arc<Mutex<Vec<String>>>,
    asked: Arc<Mutex<Vec<String>>>,
    logs: Arc<Mutex<Vec<String>>>,
}

#[derive(Clone)]
struct TestClient {
    /// `None`: no elicitation capability; otherwise the person's answer.
    approve: Option<bool>,
    legacy: bool,
    seen: Seen,
}

impl ClientHandler for TestClient {
    fn get_info(&self) -> ClientConfig {
        let capabilities = if self.approve.is_some() {
            ClientCapabilities::builder().enable_elicitation().build()
        } else {
            ClientCapabilities::default()
        };
        let mut config =
            ClientConfig::new(capabilities, Implementation::new("test-client", "1.2.3"));
        if self.legacy {
            config.protocol_version = ProtocolVersion::V_2025_11_25;
        }
        config
    }

    async fn create_elicitation(
        &self,
        request: ElicitRequestParams,
        _context: RequestContext<RoleClient>,
    ) -> Result<ElicitResult, McpError> {
        if let ElicitRequestParams::FormElicitationParams { message, .. } = &request {
            self.seen
                .asked
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(message.clone());
        }
        Ok(ElicitResult::new(ElicitationAction::Accept)
            .with_content(json!({ "approve": self.approve.unwrap_or(false) })))
    }

    async fn on_progress(
        &self,
        params: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.seen
            .progress
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(params.message.unwrap_or_default());
    }

    #[allow(deprecated)] // logging is how clients on 2025-11-25 and 2026-07-28 hear it
    async fn on_logging_message(
        &self,
        params: rmcp::model::LoggingMessageNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.seen
            .logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(params.data.to_string());
    }

    async fn on_resource_updated(
        &self,
        params: ResourceUpdatedNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.seen
            .updated
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(params.uri);
    }
}

struct Connected {
    client: RunningService<RoleClient, TestClient>,
    backend: Arc<FakeBackend>,
    seen: Seen,
}

async fn connect(mode: Mode, approve: Option<bool>, legacy: bool) -> Connected {
    let backend = FakeBackend::new();
    let shared: SharedBackend = backend.clone();
    let server = McpServer::builder(shared, settings(mode)).build();
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let seen = Seen::default();
    let client = TestClient {
        approve,
        legacy,
        seen: seen.clone(),
    }
    .serve(client_io)
    .await
    .expect("client connects");
    Connected {
        client,
        backend,
        seen,
    }
}

async fn call(
    client: &RunningService<RoleClient, TestClient>,
    name: &'static str,
    args: Value,
) -> (Value, bool, String) {
    let Value::Object(args) = args else {
        panic!("arguments must be an object")
    };
    let mut params = CallToolRequestParams::new(name).with_arguments(args);
    params.meta = Some(RequestMetaObject::with_progress_token(ProgressToken(
        NumberOrString::Number(7),
    )));
    let result = client.call_tool(params).await.expect("the call completes");
    let text = result
        .content
        .iter()
        .filter_map(|c| c.as_text().map(|t| t.text.clone()))
        .collect::<Vec<_>>()
        .join("\n");
    (
        result.structured_content.unwrap_or(Value::Null),
        result.is_error.unwrap_or(false),
        text,
    )
}

#[tokio::test]
async fn introduces_itself_with_instructions_and_capabilities() {
    let c = connect(Mode::Ask, None, false).await;
    let info = c.client.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|i| i.name.as_str()),
        Some("teitunnel")
    );
    let instructions = info.instructions.clone().unwrap_or_default();
    assert!(
        instructions.contains("plan_change") && instructions.contains("mode: ask"),
        "{instructions}"
    );
    assert!(info.capabilities.tools.is_some());
    assert!(info.capabilities.resources.is_some());
    assert!(info.capabilities.prompts.is_some());
}

#[tokio::test]
async fn lists_tools_by_mode() {
    let ask = connect(Mode::Ask, None, false).await;
    let tools = ask.client.list_all_tools().await.unwrap();
    // Teitunnel's 28 (with route_health and route_traffic), the 5 sharing extras and
    // OpenAPI, the 5 edge protection tools, and the offline page and inbox tools.
    assert_eq!(tools.len(), 41);
    for tool in &tools {
        assert_eq!(
            tool.input_schema.get("type"),
            Some(&json!("object")),
            "{}",
            tool.name
        );
        assert!(tool.output_schema.is_some(), "{}", tool.name);
    }
    let read_only = connect(Mode::ReadOnly, None, false).await;
    let tools = read_only.client.list_all_tools().await.unwrap();
    let names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    assert!(names.contains(&"plan_change") && names.contains(&"wait_for_request"));
    assert!(names.contains(&"traffic_openapi") && names.contains(&"route_health"));
    for hidden in [
        "share_port",
        "pause_share",
        "share_folder",
        "set_offline_page",
        "set_webhook_inbox",
        "apply_plan",
        "stop_share",
        "fix_issue",
        "traffic_replay",
    ] {
        assert!(
            !names.contains(&hidden),
            "{hidden} is hidden in read-only mode"
        );
    }
    // Calling one anyway is refused.
    let (_, is_error, text) =
        call(&read_only.client, "share_port", json!({ "target": "3000" })).await;
    assert!(is_error && text.contains("read-only"), "{text}");
    assert!(read_only.backend.lock().shares.is_empty());
}

#[tokio::test]
async fn structured_results_and_tool_errors() {
    let c = connect(Mode::Ask, None, false).await;
    let (routes, is_error, text) = call(&c.client, "list_routes", json!({})).await;
    assert!(!is_error);
    assert_eq!(routes["routes"][0]["hostname"], "app.xyz.com");
    assert!(
        text.contains("\"app.xyz.com\""),
        "the JSON is in the text too"
    );
    let (_, is_error, text) = call(&c.client, "list_routes", json!({ "account": "nobody" })).await;
    assert!(is_error && text.contains("No connected account"), "{text}");
    // An unknown tool is a protocol error (invalid params), not a tool error.
    let unknown = c
        .client
        .call_tool(CallToolRequestParams::new("no_such_tool"))
        .await
        .unwrap_err();
    let rmcp::ServiceError::McpError(unknown) = unknown else {
        panic!("a JSON-RPC error: {unknown:?}")
    };
    assert_eq!(unknown.code, rmcp::model::ErrorCode::INVALID_PARAMS);
    assert!(unknown.message.contains("no tool"), "{}", unknown.message);
    // Bad arguments stay a tool error the model reads and corrects.
    let (_, is_error, text) = call(&c.client, "list_routes", json!({ "nope": 1 })).await;
    assert!(is_error && text.contains("Invalid arguments"), "{text}");
    // The text is compact JSON (the same as structuredContent), not pretty-printed.
    let (_, _, text) = call(&c.client, "list_routes", json!({})).await;
    assert!(!text.contains("\n  "), "{text}");
    let (logs, _, text) = call(&c.client, "logs_tail", json!({})).await;
    assert!(!text.contains("abc123secret") && !logs.to_string().contains("abc123secret"));
}

#[tokio::test]
async fn the_person_approves_through_the_client() {
    let c = connect(Mode::Ask, Some(true), false).await;
    let (plan, ..) = call(
        &c.client,
        "plan_change",
        json!({ "change": { "type": "addRoute", "hostname": "new.xyz.com", "origin": "4000" } }),
    )
    .await;
    assert!(plan["plan"]["approval"].as_str().unwrap().contains("asked"));
    let (applied, is_error, text) = call(
        &c.client,
        "apply_plan",
        json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"] }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert_eq!(applied["outcome"], "applied", "{applied}");
    let asked = c
        .seen
        .asked
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(asked.len(), 1);
    assert!(
        asked[0].starts_with("test-client asks Teitunnel to: Add new.xyz.com"),
        "{}",
        asked[0]
    );
    assert!(asked[0].contains("1. Update routes for new.xyz.com"));
    let (change, actor) = c.backend.lock().applied[0].clone();
    assert!(matches!(
        change,
        teitunnel_core::engine::Change::AddRoute { .. }
    ));
    let actor = actor.expect("recorded with the agent's name");
    assert_eq!(
        (actor.client.as_str(), actor.version.as_deref()),
        ("test-client", Some("1.2.3"))
    );
    let progress = c
        .seen
        .progress
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert!(
        progress.iter().any(|p| p.contains("Update routes")),
        "{progress:?}"
    );
}

#[tokio::test]
async fn a_declined_approval_changes_nothing_even_if_the_agent_says_confirmed() {
    let c = connect(Mode::Ask, Some(false), false).await;
    let (plan, ..) = call(
        &c.client,
        "plan_change",
        json!({ "change": { "type": "removeRoute", "hostname": "app.xyz.com" } }),
    )
    .await;
    let (applied, ..) = call(
        &c.client,
        "apply_plan",
        json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"], "confirmed": true }),
    )
    .await;
    assert_eq!(applied["outcome"], "declined");
    assert_eq!(c.backend.lock().routes.len(), 1);
    let (shared, ..) = call(
        &c.client,
        "share_port",
        json!({ "target": "3000", "confirmed": true }),
    )
    .await;
    assert_eq!(shared["outcome"], "declined");
}

#[tokio::test]
async fn without_elicitation_it_asks_for_a_second_confirmed_call() {
    let c = connect(Mode::Ask, None, false).await;
    let (first, ..) = call(&c.client, "share_port", json!({ "target": "3000" })).await;
    assert_eq!(first["outcome"], "needsApproval");
    assert!(
        first["message"]
            .as_str()
            .unwrap()
            .contains("\"confirmed\": true")
    );
    let (second, ..) = call(
        &c.client,
        "share_port",
        json!({ "target": "3000", "confirmed": true }),
    )
    .await;
    assert_eq!(second["outcome"], "shared");
}

#[tokio::test]
async fn serves_resources_and_prompts() {
    let c = connect(Mode::Ask, None, false).await;
    let resources = c.client.list_all_resources().await.unwrap();
    assert!(resources.iter().any(|r| r.uri == "teitunnel://routes"));
    let templates = c.client.list_all_resource_templates().await.unwrap();
    assert!(
        templates
            .iter()
            .any(|t| t.uri_template == "teitunnel://route/{hostname}")
    );
    let routes = c
        .client
        .read_resource(ReadResourceRequestParams::new(
            "teitunnel://route/app.xyz.com",
        ))
        .await
        .unwrap();
    let text = serde_json::to_string(&routes.contents).unwrap();
    assert!(text.contains("http://localhost:3000"), "{text}");
    assert!(
        c.client
            .read_resource(ReadResourceRequestParams::new(
                "teitunnel://route/nope.xyz.com"
            ))
            .await
            .is_err()
    );
    let prompts = c.client.list_all_prompts().await.unwrap();
    assert_eq!(prompts.len(), 5);
    let mut args = serde_json::Map::new();
    args.insert("path".into(), "/webhooks/stripe".into());
    let prompt = c
        .client
        .get_prompt(GetPromptRequestParams::new("debug_webhook").with_arguments(args))
        .await
        .unwrap();
    let text = serde_json::to_string(&prompt.messages).unwrap();
    assert!(text.contains("wait_for_request") && text.contains("/webhooks/stripe"));
}

#[tokio::test]
// `resources/subscribe` is how clients on protocol 2025-11-25 subscribe.
#[allow(deprecated)]
async fn subscribers_hear_about_changes() {
    let c = connect(Mode::Full, None, true).await;
    c.client
        .subscribe(SubscribeRequestParams::new("teitunnel://routes"))
        .await
        .unwrap();
    let (plan, ..) = call(
        &c.client,
        "plan_change",
        json!({ "change": { "type": "addRoute", "hostname": "n.xyz.com", "origin": "4000" } }),
    )
    .await;
    let (applied, ..) = call(
        &c.client,
        "apply_plan",
        json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"] }),
    )
    .await;
    assert_eq!(applied["outcome"], "applied");
    for _ in 0..50 {
        if !c
            .seen
            .updated
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    let updated = c
        .seen
        .updated
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(updated, ["teitunnel://routes"]);
}

// Clients on protocol 2026-07-28: no handshake, per-request metadata, approvals through
// multi round-trip requests.

/// Records who was introduced to it; never answers (like the app not running).
#[derive(Default)]
struct Away {
    introduced: Mutex<Vec<String>>,
}

impl crate::registry::Approver for Away {
    fn approve<'a>(
        &'a self,
        _actor: &'a teitunnel_core::engine::Actor,
        _request: &'a crate::registry::ApprovalRequest,
    ) -> crate::backend::BoxFuture<'a, crate::registry::AppAnswer> {
        Box::pin(async { crate::registry::AppAnswer::Unavailable })
    }

    fn agent_connected<'a>(
        &'a self,
        actor: &'a teitunnel_core::engine::Actor,
    ) -> crate::backend::BoxFuture<'a, ()> {
        self.introduced
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(actor.client.clone());
        Box::pin(async {})
    }
}

async fn connect_modern(
    settings: crate::config::Settings,
    approve: Option<bool>,
    approver: Option<Arc<Away>>,
) -> Connected {
    use rmcp::service::{ClientLifecycleMode, ClientServiceExt as _};
    let backend = FakeBackend::new();
    let shared: SharedBackend = backend.clone();
    let mut builder = McpServer::builder(shared, settings);
    if let Some(approver) = approver {
        builder = builder.approver(approver);
    }
    let server = builder.build();
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let seen = Seen::default();
    let client = TestClient {
        approve,
        legacy: false,
        seen: seen.clone(),
    }
    .serve_with_lifecycle(
        client_io,
        ClientLifecycleMode::Discover {
            preferred_versions: vec![ProtocolVersion::V_2026_07_28],
        },
    )
    .await
    .expect("client discovers the server");
    Connected {
        client,
        backend,
        seen,
    }
}

fn ask_settings(approve_in_app: bool) -> crate::config::Settings {
    crate::config::Settings {
        mode: Mode::Ask,
        approve_in_app,
        ..crate::config::Settings::default()
    }
}

async fn plan(client: &RunningService<RoleClient, TestClient>) -> Value {
    let (plan, ..) = call(
        client,
        "plan_change",
        json!({ "change": { "type": "addRoute", "hostname": "new.xyz.com", "origin": "4000" } }),
    )
    .await;
    plan
}

#[tokio::test]
async fn discovery_says_who_the_server_is_and_how_long_to_cache() {
    let c = connect_modern(ask_settings(true), None, None).await;
    let info = c.client.peer_info().expect("server info");
    assert_eq!(info.protocol_version, ProtocolVersion::V_2026_07_28);
    let server = info.server_info.clone().expect("server info");
    assert_eq!(
        server.website_url.as_deref(),
        Some("https://teitunnel.teispace.com")
    );
    assert_eq!(
        server.icons.as_ref().map(|i| i[0].src.as_str()),
        Some("https://teitunnel.teispace.com/icon.png")
    );
    let tools = c.client.list_tools(None).await.unwrap();
    assert!(tools.ttl_ms.is_some_and(|t| t > 0));
    assert_eq!(tools.cache_scope, Some(rmcp::model::CacheScope::Private));
    let prompts = c.client.list_prompts(None).await.unwrap();
    assert!(prompts.ttl_ms.is_some());
    let resources = c.client.list_resources(None).await.unwrap();
    assert!(resources.ttl_ms.is_some());
    let templates = c.client.list_resource_templates(None).await.unwrap();
    assert!(templates.ttl_ms.is_some());
    let read = c
        .client
        .read_resource(ReadResourceRequestParams::new("teitunnel://routes"))
        .await
        .unwrap();
    assert_eq!(read.ttl_ms, Some(5000), "routes change: a short freshness");
    assert_eq!(read.cache_scope, Some(rmcp::model::CacheScope::Private));
}

#[tokio::test]
async fn the_person_approves_through_a_multi_round_trip() {
    let c = connect_modern(ask_settings(true), Some(true), None).await;
    let plan = plan(&c.client).await;
    assert!(plan["plan"]["approval"].as_str().unwrap().contains("asked"));
    let args =
        json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"] });
    let Value::Object(args) = args else {
        unreachable!()
    };
    // One round by hand: the server asks, with a sealed state, and changes nothing.
    let first = c
        .client
        .call_tool_once(CallToolRequestParams::new("apply_plan").with_arguments(args.clone()))
        .await
        .unwrap();
    let rmcp::model::CallToolResponse::InputRequired(input) = first else {
        panic!("the person is asked first: {first:?}")
    };
    let state = input.request_state.clone().expect("a request state");
    let question = serde_json::to_value(&input.input_requests).unwrap();
    assert_eq!(question["approve"]["method"], "elicitation/create");
    assert!(
        question["approve"]["params"]["message"]
            .as_str()
            .unwrap()
            .contains("Add new.xyz.com"),
        "{question}"
    );
    assert!(c.backend.lock().applied.is_empty());

    // A state that was tampered with is refused (a protocol error).
    let mut forged = state.clone();
    forged.insert(0, 'x');
    let mut yes = rmcp::model::InputResponses::new();
    yes.insert(
        "approve".into(),
        json!({ "action": "accept", "content": { "approve": true } }),
    );
    let refused = c
        .client
        .call_tool_once(
            CallToolRequestParams::new("apply_plan")
                .with_arguments(args.clone())
                .with_input_responses(yes.clone())
                .with_request_state(forged),
        )
        .await;
    assert!(refused.is_err(), "{refused:?}");
    // The same state for another call is refused too.
    let mut other = args.clone();
    other.insert("planId".into(), json!("another"));
    assert!(
        c.client
            .call_tool_once(
                CallToolRequestParams::new("apply_plan")
                    .with_arguments(other)
                    .with_input_responses(yes.clone())
                    .with_request_state(state.clone()),
            )
            .await
            .is_err()
    );
    assert!(c.backend.lock().applied.is_empty());

    // The client's own driver: it asks the person (the handler) and retries.
    let (applied, is_error, text) = call(
        &c.client,
        "apply_plan",
        json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"] }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert_eq!(applied["outcome"], "applied", "{applied}");
    let asked = c
        .seen
        .asked
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].starts_with("test-client asks Teitunnel to: Add new.xyz.com"));
    let actor = c.backend.lock().applied[0].1.clone().unwrap();
    assert_eq!(
        actor.client, "test-client",
        "who asked, from the request's _meta"
    );
}

#[tokio::test]
async fn a_no_through_a_multi_round_trip_changes_nothing() {
    let c = connect_modern(ask_settings(true), Some(false), None).await;
    let (shared, is_error, text) = call(
        &c.client,
        "share_port",
        json!({ "target": "3000", "confirmed": true }),
    )
    .await;
    assert!(!is_error, "{text}");
    assert_eq!(shared["outcome"], "declined");
    assert!(c.backend.lock().shares.is_empty());
}

#[tokio::test]
async fn without_anyone_to_ask_confirmed_isnt_enough() {
    // A client that can't ask, and no app: the agent can't approve its own change.
    let c = connect_modern(ask_settings(true), None, None).await;
    let (_, is_error, text) = call(
        &c.client,
        "share_port",
        json!({ "target": "3000", "confirmed": true }),
    )
    .await;
    assert!(is_error, "{text}");
    assert!(text.contains("needs the person's approval"), "{text}");
    assert!(c.backend.lock().shares.is_empty());

    // With the app as approver, the answer says to open it.
    let away = Arc::new(Away::default());
    let c = connect_modern(ask_settings(true), None, Some(Arc::clone(&away))).await;
    let (_, is_error, text) = call(
        &c.client,
        "share_port",
        json!({ "target": "3000", "confirmed": true }),
    )
    .await;
    assert!(is_error && text.contains("open Teitunnel"), "{text}");
    assert!(c.backend.lock().shares.is_empty());

    // Turned off, the conversation's confirmation counts again (the old behaviour).
    let c = connect_modern(ask_settings(false), None, None).await;
    let (shared, ..) = call(
        &c.client,
        "share_port",
        json!({ "target": "3000", "confirmed": true }),
    )
    .await;
    assert_eq!(shared["outcome"], "shared");
}

#[tokio::test]
async fn legacy_clients_cant_approve_their_own_changes_either() {
    let backend = FakeBackend::new();
    let shared: SharedBackend = backend.clone();
    let server = McpServer::builder(shared, ask_settings(true)).build();
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        if let Ok(running) = server.serve(server_io).await {
            let _ = running.waiting().await;
        }
    });
    let client = TestClient {
        approve: None,
        legacy: true,
        seen: Seen::default(),
    }
    .serve(client_io)
    .await
    .unwrap();
    let (_, is_error, text) = call(
        &client,
        "share_port",
        json!({ "target": "3000", "confirmed": true }),
    )
    .await;
    assert!(
        is_error && text.contains("needs the person's approval"),
        "{text}"
    );
    assert!(backend.lock().shares.is_empty());
}

#[tokio::test]
async fn an_agent_without_a_handshake_is_introduced_on_its_first_request() {
    let away = Arc::new(Away::default());
    let c = connect_modern(ask_settings(true), None, Some(Arc::clone(&away))).await;
    let _ = c.client.list_tools(None).await.unwrap();
    let _ = call(&c.client, "list_routes", json!({})).await;
    for _ in 0..50 {
        if !away
            .introduced
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let introduced = away
        .introduced
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(introduced, ["test-client"], "once, with its name");
}

#[tokio::test]
async fn outside_content_is_marked_and_fenced() {
    let c = connect_modern(ask_settings(true), None, None).await;
    let (logs, is_error, text) = call(&c.client, "logs_tail", json!({})).await;
    assert!(!is_error, "{text}");
    assert_eq!(logs["untrusted"], true);
    assert!(
        text.contains("<untrusted-data>") && text.trim_end().ends_with("</untrusted-data>"),
        "{text}"
    );
    let (routes, _, text) = call(&c.client, "list_routes", json!({})).await;
    assert!(routes.get("untrusted").is_none());
    assert!(!text.contains("untrusted-data"));
    let tools = c.client.list_all_tools().await.unwrap();
    let logs_tool = tools.iter().find(|t| t.name == "logs_tail").unwrap();
    assert!(
        logs_tool
            .output_schema
            .as_ref()
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.get("untrusted"))
            .is_some()
    );
}

#[tokio::test]
#[allow(deprecated)] // logging, deprecated in 2026-07-28, still works there
async fn logs_go_only_to_requests_that_ask_for_them() {
    let c = connect_modern(
        crate::config::Settings {
            mode: Mode::Full,
            ..crate::config::Settings::default()
        },
        None,
        None,
    )
    .await;
    let logs = || {
        c.seen
            .logs
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    };
    // No `io.modelcontextprotocol/logLevel` in the request: no log messages.
    let _ = call(&c.client, "share_port", json!({ "target": "3000" })).await;
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(logs().is_empty(), "{:?}", logs());
    let Value::Object(args) = json!({ "target": "3001" }) else {
        unreachable!()
    };
    let mut params = CallToolRequestParams::new("share_port").with_arguments(args);
    let mut meta = RequestMetaObject::default();
    meta.set_log_level(rmcp::model::LoggingLevel::Info);
    params.meta = Some(meta);
    let _ = c.client.call_tool(params).await.unwrap();
    for _ in 0..50 {
        if !logs().is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        logs().iter().any(|l| l.contains("share_port")),
        "{:?}",
        logs()
    );
}

#[test]
fn read_only_instructions_dont_offer_changes() {
    let instructions = crate::server::instructions(Mode::ReadOnly);
    for change in ["share_port", "apply_plan", "fix_issue", "confirmed"] {
        assert!(!instructions.contains(change), "{change}");
    }
    assert!(instructions.contains("mode: read-only"));
    let ask = crate::server::instructions(Mode::Ask);
    assert!(ask.contains("input_required") && ask.contains("untrusted"));
}

/// What every connection pays before its first call: the tool list. Kept in check so
/// descriptions stay worth their tokens.
#[tokio::test]
async fn the_tool_list_stays_small() {
    let c = connect(Mode::Ask, None, false).await;
    let tools = c.client.list_all_tools().await.unwrap();
    let size = serde_json::to_string(&tools).unwrap().len();
    let descriptions: usize = tools
        .iter()
        .map(|t| t.description.as_deref().map_or(0, str::len))
        .sum();
    // Shown with `--no-capture`.
    tracing::info!(
        "tools/list: {} tools, {size} bytes, {descriptions} bytes of descriptions",
        tools.len()
    );
    // 41 tools: about 133 KB, of which 23 KB descriptions and the rest input and output
    // schemas (plan_change's input alone is 8.7 KB: every kind of change).
    assert!(size < 140_000, "tools/list is {size} bytes");
    assert!(
        descriptions < 25_000,
        "{descriptions} bytes of descriptions"
    );
    for tool in &tools {
        let length = tool.description.as_deref().map_or(0, str::len);
        assert!(
            length < 2_100,
            "{} has a {length}-byte description",
            tool.name
        );
        let schema = serde_json::to_string(&tool.input_schema).unwrap();
        assert!(
            !schema.contains("$schema") && !schema.contains("\"default\":null"),
            "{} has a schema with noise: {schema}",
            tool.name
        );
    }
}
