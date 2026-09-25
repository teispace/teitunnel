//! Edge protection and service tokens against the fake Cloudflare: end to end, foreign
//! rules untouched, the rate limit shared across hostnames, secrets handed out once,
//! and the scenarios the failure-at-every-step harness rolls back.

use super::{
    edge::{BotMode, EdgeHeaderOp, EdgeProtection, HeaderRule, LimitAction, RateLimitSpec},
    executor::{Approval, Context, Engine, EngineError, Outcome},
    fake::{CloudState, FakeCloud, FakeConnectors},
    local::Local,
    observe::ObserveError,
    types::{Intent, ZoneRef},
};
use crate::{domain::Hostname, store::Store};

const CTX: Context<'static> = Context {
    account: "acc",
    machine_name: "Mac",
    tunnel: None,
};

fn engine() -> Engine {
    Engine::new(Local::new(Store::open_in_memory().unwrap()))
}

fn host(h: &str) -> Hostname {
    Hostname::parse(h).unwrap()
}

/// A zone on `plan` with someone else's custom rule.
pub(super) fn zone(plan: &str) -> CloudState {
    let mut state = CloudState {
        zones: vec![ZoneRef {
            id: "z-xyz".into(),
            name: "xyz.com".into(),
        }],
        access_org: true,
        ..CloudState::default()
    };
    state.zone_plans.insert("z-xyz".into(), plan.into());
    state.rulesets.insert(
        ("z-xyz".into(), cf_api::PHASE_CUSTOM.into()),
        (
            "rs-custom".into(),
            vec![cf_api::Rule {
                id: "theirs".into(),
                action: "block".into(),
                expression: "(ip.src eq 192.0.2.1)".into(),
                description: "Their own rule".into(),
                enabled: true,
                action_parameters: None,
                ratelimit: None,
            }],
        ),
    );
    state
}

pub(super) fn everything(rate_limit: bool) -> EdgeProtection {
    EdgeProtection {
        bots: BotMode::Challenge,
        ai_crawlers: true,
        rate_limit: rate_limit.then_some(RateLimitSpec {
            requests: 30,
            period: 60,
            action: LimitAction::Block,
        }),
        request_headers: vec![HeaderRule {
            name: "X-Env".into(),
            op: EdgeHeaderOp::Set,
            value: Some("preview".into()),
        }],
        response_headers: vec![HeaderRule {
            name: "X-Robots-Tag".into(),
            op: EdgeHeaderOp::Set,
            value: Some("noindex".into()),
        }],
    }
}

pub(super) fn protect(hostname: &str, protection: EdgeProtection) -> Intent {
    Intent::ProtectHostname {
        hostname: host(hostname),
        protection,
    }
}

pub(super) fn create_token(hostname: &str, label: &str) -> Intent {
    Intent::CreateServiceToken {
        hostname: host(hostname),
        label: label.into(),
    }
}

async fn apply(
    engine: &Engine,
    cloud: &FakeCloud,
    intent: &Intent,
) -> (Outcome, Vec<super::edge::IssuedToken>) {
    let plan = engine.preview(cloud, CTX, intent).await.unwrap();
    engine
        .apply_issuing(
            cloud,
            &FakeConnectors::default(),
            CTX,
            intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .unwrap()
}

fn custom_rules(cloud: &FakeCloud) -> Vec<String> {
    cloud
        .snapshot()
        .rulesets
        .get(&("z-xyz".into(), cf_api::PHASE_CUSTOM.into()))
        .map(|(_, rules)| rules.iter().map(|r| r.description.clone()).collect())
        .unwrap_or_default()
}

#[tokio::test]
async fn protects_a_hostname_and_takes_it_all_back_leaving_theirs() {
    let (engine, cloud) = (engine(), FakeCloud::new(zone("pro")));
    let before = cloud.snapshot();
    let (outcome, issued) = apply(&engine, &cloud, &protect("app.xyz.com", everything(true))).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert!(issued.is_empty());
    let rules = custom_rules(&cloud);
    assert_eq!(rules[0], "Their own rule");
    assert_eq!(rules.len(), 3, "{rules:?}");
    assert_eq!(
        engine.local().owned_edge_rules("acc").await.unwrap().len(),
        5
    );
    let log = engine.local().activity("acc", 1).await.unwrap();
    assert_eq!(log[0].summary, "Protect app.xyz.com at Cloudflare's edge");

    // Nothing to do the second time.
    let plan = engine
        .preview(&cloud, CTX, &protect("app.xyz.com", everything(true)))
        .await
        .unwrap();
    assert!(plan.steps.is_empty(), "{:?}", plan.steps);

    let (outcome, _) = apply(
        &engine,
        &cloud,
        &protect("app.xyz.com", EdgeProtection::default()),
    )
    .await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(cloud.snapshot().normalized(), before.normalized());
    assert!(
        engine
            .local()
            .owned_edge_rules("acc")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn hostnames_share_the_zones_rate_limit() {
    let (engine, cloud) = (engine(), FakeCloud::new(zone("pro")));
    let limited = EdgeProtection {
        rate_limit: everything(true).rate_limit,
        ..EdgeProtection::default()
    };
    apply(&engine, &cloud, &protect("a.xyz.com", limited.clone())).await;
    apply(&engine, &cloud, &protect("b.xyz.com", limited)).await;
    let state = cloud.snapshot();
    let (_, rules) = &state.rulesets[&("z-xyz".to_owned(), cf_api::PHASE_RATE_LIMIT.to_owned())];
    assert_eq!(rules.len(), 1, "one rule for both");
    assert_eq!(
        rules[0].expression,
        r#"(http.host in {"a.xyz.com" "b.xyz.com"})"#
    );
}

#[tokio::test]
async fn a_missing_permission_is_reported_as_such() {
    let mut state = zone("free");
    state.rulesets_forbidden = true;
    state.service_tokens_forbidden = true;
    let (engine, cloud) = (engine(), FakeCloud::new(state));
    let err = engine
        .preview(&cloud, CTX, &protect("app.xyz.com", everything(false)))
        .await
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Observe(ObserveError::EdgePermission)),
        "{err:?}"
    );
    let err = engine
        .preview(&cloud, CTX, &create_token("app.xyz.com", "CI"))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            EngineError::Observe(ObserveError::ServiceTokenPermission)
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_new_token_is_handed_out_once_and_never_recorded() {
    let (engine, cloud) = (engine(), FakeCloud::new(zone("free")));
    let (outcome, issued) = apply(&engine, &cloud, &create_token("api.xyz.com", "CI")).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let [token] = issued.as_slice() else {
        panic!("{issued:?}")
    };
    assert_eq!(
        token.client_secret.expose(),
        &format!("secret-for-{}", token.token_id)
    );
    assert!(!format!("{token:?}").contains("secret-for"), "redacted");
    // Only machines pass the new login, and only this token.
    let state = cloud.snapshot();
    let app = state.access_apps.values().next().unwrap();
    assert_eq!(app.policies.len(), 1);
    assert_eq!(app.policies[0].decision, "non_identity");
    // The secret is nowhere in the activity log or the local store.
    let log = engine.local().activity("acc", 1).await.unwrap();
    assert!(!serde_json::to_string(&log).unwrap().contains("secret-for"));
    let stored = engine.local().owned_service_tokens("acc").await.unwrap();
    assert_eq!(stored[0].client_id, token.client_id);

    // Rotating hands out a new secret; revoking removes the login again.
    let rotate = Intent::RotateServiceToken {
        hostname: host("api.xyz.com"),
        token_id: token.token_id.clone(),
    };
    let (_, rotated) = apply(&engine, &cloud, &rotate).await;
    assert_ne!(rotated[0].client_secret, token.client_secret);
    let revoke = Intent::RevokeServiceToken {
        hostname: host("api.xyz.com"),
        token_id: token.token_id.clone(),
    };
    let (outcome, _) = apply(&engine, &cloud, &revoke).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    assert!(state.service_tokens.is_empty());
    assert!(
        state.access_apps.is_empty(),
        "the machine-only login went with it"
    );
}

#[tokio::test]
async fn a_rolled_back_token_hands_out_nothing() {
    let (engine, cloud) = (engine(), FakeCloud::new(zone("free")));
    // The token is created (0), then letting it through the login fails (1).
    cloud.fail_once(1);
    let (outcome, issued) = apply(&engine, &cloud, &create_token("api.xyz.com", "CI")).await;
    assert!(matches!(outcome, Outcome::RolledBack { .. }), "{outcome:?}");
    assert!(issued.is_empty());
    assert!(cloud.snapshot().service_tokens.is_empty());
}

/// Scenarios for the failure-at-every-step harness (`executor_tests`).
pub(super) async fn edge_scenarios() -> Vec<(&'static str, CloudState, Intent)> {
    let engine = engine();
    let cloud = FakeCloud::new(zone("pro"));
    apply(&engine, &cloud, &protect("app.xyz.com", everything(true))).await;
    let protected = cloud.snapshot();
    let limited = EdgeProtection {
        rate_limit: everything(true).rate_limit,
        ..EdgeProtection::default()
    };

    let cloud = FakeCloud::new(zone("free"));
    apply(&engine, &cloud, &create_token("api.xyz.com", "CI")).await;
    let with_token = cloud.snapshot();
    let token_id = with_token
        .service_tokens
        .keys()
        .next()
        .cloned()
        .unwrap_or_default();

    vec![
        (
            "first edge protection, with a shared rate limit",
            zone("pro"),
            protect("app.xyz.com", everything(true)),
        ),
        (
            "turning edge protection off",
            protected.clone(),
            protect("app.xyz.com", EdgeProtection::default()),
        ),
        (
            "another hostname joins the rate limit, with bots blocked",
            protected,
            protect(
                "web.xyz.com",
                EdgeProtection {
                    bots: BotMode::Block,
                    ..limited
                },
            ),
        ),
        (
            "a service token for a hostname without a login",
            zone("free"),
            create_token("api.xyz.com", "CI"),
        ),
        (
            "revoking a service token",
            with_token,
            Intent::RevokeServiceToken {
                hostname: host("api.xyz.com"),
                token_id,
            },
        ),
    ]
}

async fn change(engine: &Engine, cloud: &FakeCloud, change: super::Change) {
    let intent = engine.intent_for(cloud, CTX, &change).await.unwrap();
    let (outcome, _) = apply(engine, cloud, &intent).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
}

#[tokio::test]
async fn removing_the_last_route_takes_its_edge_rules_with_it() {
    use super::{Change, RouteInput};
    let (engine, cloud) = (engine(), FakeCloud::new(zone("pro")));
    let route = RouteInput {
        hostname: "app.xyz.com".into(),
        path: None,
        origin: "3000".into(),
        access: None,
        options: None,
    };
    change(&engine, &cloud, Change::AddRoute { route }).await;
    let (outcome, _) = apply(&engine, &cloud, &protect("app.xyz.com", everything(true))).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert!(
        !engine
            .local()
            .owned_edge_rules("acc")
            .await
            .unwrap()
            .is_empty()
    );

    let removal = Change::RemoveRoute {
        hostname: "app.xyz.com".into(),
        path: None,
    };
    change(&engine, &cloud, removal).await;
    assert_eq!(custom_rules(&cloud), ["Their own rule"], "theirs stays");
    assert!(
        engine
            .local()
            .owned_edge_rules("acc")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn removing_the_last_route_revokes_the_service_tokens_made_for_it() {
    use super::{Change, RouteInput};
    let (engine, cloud) = (engine(), FakeCloud::new(zone("free")));
    let route = RouteInput {
        hostname: "api.xyz.com".into(),
        path: None,
        origin: "3000".into(),
        access: None,
        options: None,
    };
    change(&engine, &cloud, Change::AddRoute { route }).await;
    let (outcome, issued) = apply(&engine, &cloud, &create_token("api.xyz.com", "CI")).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(issued.len(), 1);

    let removal = Change::RemoveRoute {
        hostname: "api.xyz.com".into(),
        path: None,
    };
    change(&engine, &cloud, removal).await;
    let state = cloud.snapshot();
    assert!(
        state.service_tokens.is_empty(),
        "{:?}",
        state.service_tokens
    );
    assert!(state.access_apps.is_empty(), "the login went too");
    assert!(
        engine
            .local()
            .owned_service_tokens("acc")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn deleting_the_tunnel_revokes_the_service_tokens_of_its_hostnames() {
    use super::{Change, RouteInput};
    let (engine, cloud) = (engine(), FakeCloud::new(zone("free")));
    let route = RouteInput {
        hostname: "api.xyz.com".into(),
        path: None,
        origin: "3000".into(),
        access: None,
        options: None,
    };
    change(&engine, &cloud, Change::AddRoute { route }).await;
    apply(&engine, &cloud, &create_token("api.xyz.com", "CI")).await;
    let (outcome, _) = apply(&engine, &cloud, &Intent::RemoveTunnel).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    assert!(state.tunnels.is_empty());
    assert!(
        state.service_tokens.is_empty(),
        "{:?}",
        state.service_tokens
    );
    assert!(
        engine
            .local()
            .owned_service_tokens("acc")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn cleaning_up_a_hostname_left_behind_removes_its_rules_only_once_nothing_serves_it() {
    use super::{Change, EngineError, PlanError, RouteInput};
    let (engine, cloud) = (engine(), FakeCloud::new(zone("pro")));
    let route = RouteInput {
        hostname: "app.xyz.com".into(),
        path: None,
        origin: "3000".into(),
        access: None,
        options: None,
    };
    change(&engine, &cloud, Change::AddRoute { route }).await;
    apply(&engine, &cloud, &protect("app.xyz.com", everything(true))).await;
    let clean_up = Change::CleanUpHostname {
        hostname: "app.xyz.com".into(),
    };
    // Still routed: refused.
    let intent = engine.intent_for(&cloud, CTX, &clean_up).await.unwrap();
    assert!(matches!(
        engine.preview(&cloud, CTX, &intent).await,
        Err(EngineError::Plan(PlanError::HostnameRouted(_)))
    ));

    // Someone deleted its DNS record in the dashboard: deleting the tunnel no longer
    // knows the hostname was served from here, and leaves its edge rules behind.
    cloud.edit(|state| state.records.clear());
    apply(&engine, &cloud, &Intent::RemoveTunnel).await;
    assert!(
        !engine
            .local()
            .owned_edge_rules("acc")
            .await
            .unwrap()
            .is_empty()
    );

    change(&engine, &cloud, clean_up).await;
    assert_eq!(custom_rules(&cloud), ["Their own rule"]);
    assert!(
        engine
            .local()
            .owned_edge_rules("acc")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn deleting_the_tunnel_takes_its_hostnames_workers_and_edge_rules() {
    use super::{Change, RouteInput, front::OfflinePage};
    let (engine, cloud) = (engine(), FakeCloud::new(zone("pro")));
    for hostname in ["app.xyz.com", "api.xyz.com"] {
        let route = RouteInput {
            hostname: hostname.into(),
            path: None,
            origin: "3000".into(),
            access: None,
            options: None,
        };
        change(&engine, &cloud, Change::AddRoute { route }).await;
    }
    apply(&engine, &cloud, &protect("app.xyz.com", everything(true))).await;
    let offline = Intent::SetOfflinePage {
        hostname: host("api.xyz.com"),
        page: Some(OfflinePage::default()),
    };
    let (outcome, _) = apply(&engine, &cloud, &offline).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(cloud.snapshot().worker_routes.len(), 1);

    let (outcome, _) = apply(&engine, &cloud, &Intent::RemoveTunnel).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    assert!(state.tunnels.is_empty());
    assert!(state.worker_routes.is_empty(), "{:?}", state.worker_routes);
    assert!(state.workers.is_empty(), "{:?}", state.workers.keys());
    assert_eq!(custom_rules(&cloud), ["Their own rule"], "theirs stays");
    let local = engine.local();
    assert!(local.owned_edge_rules("acc").await.unwrap().is_empty());
    assert!(local.fronts(Some("acc"), None).await.unwrap().is_empty());
}
