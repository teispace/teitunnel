//! The edge protection tools against the in-memory backend: listing, planning through
//! the shared plans (applied with apply_plan), approvals, and a token's secret handed
//! out once.

use std::sync::Arc;

use serde_json::{Value, json};
use teitunnel_core::{
    engine::{
        Change,
        edge::{BotMode, EdgeProtection, HeaderOp, HeaderRule},
    },
    protection::ProtectionChange,
};

use super::{
    CoreTools, ProtectionTools,
    tests::{FakeBackend, actor},
};
use crate::{
    backend::SharedBackend,
    config::{Mode, Settings},
    plans::Plans,
    registry::{ToolClass, ToolContext, ToolProvider},
    traffic::NoTraffic,
};

struct Harness {
    backend: Arc<FakeBackend>,
    core: CoreTools,
    protection: ProtectionTools,
}

fn harness() -> Harness {
    let backend = FakeBackend::new();
    let shared: SharedBackend = backend.clone();
    let plans = Arc::new(Plans::default());
    Harness {
        core: CoreTools::new(shared.clone(), Arc::new(NoTraffic), Arc::clone(&plans)),
        protection: ProtectionTools::new(shared, plans),
        backend,
    }
}

impl Harness {
    async fn call(&self, mode: Mode, name: &str, args: Value) -> Result<Value, String> {
        let ctx = ToolContext::detached(
            Settings {
                mode,
                allow_secrets: false,
            },
            actor(),
        );
        let Value::Object(args) = args else {
            return Err("arguments must be an object".into());
        };
        let provider: &dyn ToolProvider =
            if self.protection.tools().iter().any(|t| t.tool.name == name) {
                &self.protection
            } else {
                &self.core
            };
        provider
            .call(name, args, &ctx)
            .await
            .map(|out| crate::redaction::value(out.structured, false))
            .map_err(|e| e.message)
    }
}

#[test]
fn tools_are_listed_with_their_classes() {
    let h = harness();
    let specs = h.protection.tools();
    let class = |name: &str| specs.iter().find(|s| s.tool.name == name).map(|s| s.class);
    assert_eq!(class("get_protection"), Some(ToolClass::Read));
    assert_eq!(
        class("protect_hostname"),
        Some(ToolClass::Read),
        "it only plans"
    );
    assert_eq!(class("service_token_list"), Some(ToolClass::Read));
    assert_eq!(class("service_token_create"), Some(ToolClass::Change));
    assert_eq!(class("service_token_revoke"), Some(ToolClass::Destructive));
    let create = specs
        .iter()
        .find(|s| s.tool.name == "service_token_create")
        .unwrap();
    let hints = create.tool.annotations.as_ref().unwrap();
    assert_eq!(hints.destructive_hint, Some(false));
    for spec in &specs {
        assert!(spec.tool.output_schema.is_some(), "{}", spec.tool.name);
    }
}

#[tokio::test]
async fn protect_hostname_changes_only_what_it_names_and_applies_through_apply_plan() {
    let h = harness();
    h.backend.lock().protection = EdgeProtection {
        ai_crawlers: true,
        ..EdgeProtection::default()
    };
    let got = h
        .call(
            Mode::Full,
            "get_protection",
            json!({ "hostname": "app.xyz.com" }),
        )
        .await
        .unwrap();
    assert_eq!(got["blockAiCrawlers"], true);
    assert_eq!(got["plan"], "pro");

    let plan = h
        .call(
            Mode::Full,
            "protect_hostname",
            json!({
                "hostname": "app.xyz.com",
                "bots": "challenge",
                "responseHeaders": [{ "name": "X-Robots-Tag", "op": "set", "value": "noindex" }]
            }),
        )
        .await
        .unwrap();
    let id = plan["plan"]["planId"].as_str().unwrap().to_owned();
    let fingerprint = plan["plan"]["fingerprint"].as_str().unwrap().to_owned();
    assert!(
        h.backend.lock().applied.is_empty(),
        "planning changes nothing"
    );
    let applied = h
        .call(
            Mode::Full,
            "apply_plan",
            json!({ "planId": id, "fingerprint": fingerprint, "verify": false }),
        )
        .await
        .unwrap();
    assert_eq!(applied["outcome"], "applied", "{applied}");
    {
        let state = h.backend.lock();
        let (Change::ProtectHostname { protection, .. }, Some(by)) = &state.applied[0] else {
            panic!("{:?}", state.applied)
        };
        assert_eq!(by.client, "test-agent");
        assert_eq!(protection.bots, BotMode::Challenge);
        assert!(protection.ai_crawlers, "kept, since it wasn't named");
        assert_eq!(
            protection.response_headers,
            [HeaderRule {
                name: "X-Robots-Tag".into(),
                op: HeaderOp::Set,
                value: Some("noindex".into()),
            }]
        );
    }

    let off = h
        .call(
            Mode::Full,
            "protect_hostname",
            json!({ "hostname": "app.xyz.com", "off": true }),
        )
        .await
        .unwrap();
    assert!(
        off["plan"]["summary"]
            .as_str()
            .unwrap()
            .starts_with("Remove edge protection")
    );
    let bad = h
        .call(
            Mode::Full,
            "protect_hostname",
            json!({ "hostname": "app.xyz.com", "requestHeaders": [{ "name": "X", "op": "append" }] }),
        )
        .await;
    assert!(bad.unwrap_err().contains("append"));
}

#[tokio::test]
async fn a_new_tokens_secret_comes_back_once_after_approval() {
    let h = harness();
    // Nobody can be asked: the tool explains, and nothing is created.
    let asked = h
        .call(
            Mode::Ask,
            "service_token_create",
            json!({ "hostname": "api.xyz.com", "name": "CI" }),
        )
        .await
        .unwrap();
    assert_eq!(asked["outcome"], "needsApproval");
    assert!(asked["credentials"].is_null());
    assert!(h.backend.lock().tokens.is_empty());

    let created = h
        .call(
            Mode::Ask,
            "service_token_create",
            json!({ "hostname": "api.xyz.com", "name": "CI", "confirmed": true }),
        )
        .await
        .unwrap();
    assert_eq!(created["outcome"], "created", "{created}");
    assert_eq!(created["credentials"]["clientId"], "tok1.access");
    assert_eq!(created["credentials"]["clientSecret"], "0123456789abcdef");
    assert_eq!(created["credentials"]["sensitive"], true);

    let listed = h
        .call(
            Mode::ReadOnly,
            "service_token_list",
            json!({ "hostname": "api.xyz.com" }),
        )
        .await
        .unwrap();
    assert_eq!(listed["tokens"][0]["name"], "CI");
    assert!(
        !listed.to_string().contains("0123456789abcdef"),
        "listing never shows a secret"
    );

    let revoked = h
        .call(
            Mode::Full,
            "service_token_revoke",
            json!({ "hostname": "api.xyz.com", "token": "ci" }),
        )
        .await
        .unwrap();
    assert_eq!(revoked["outcome"], "revoked", "{revoked}");
    assert!(matches!(
        h.backend.lock().protection_applied.last(),
        Some(ProtectionChange::RevokeToken { token_id, .. }) if token_id == "tok1"
    ));
    let missing = h
        .call(
            Mode::Full,
            "service_token_revoke",
            json!({ "hostname": "api.xyz.com", "token": "CI" }),
        )
        .await;
    assert!(missing.unwrap_err().contains("no service token"));
}
