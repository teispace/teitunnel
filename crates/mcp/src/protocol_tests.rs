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
    // Teitunnel's 26 and the 5 edge protection tools.
    assert_eq!(tools.len(), 31);
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
    for hidden in [
        "share_port",
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
    let (_, is_error, text) = call(&c.client, "no_such_tool", json!({})).await;
    assert!(is_error && text.contains("no tool"), "{text}");
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
    assert_eq!(prompts.len(), 4);
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
