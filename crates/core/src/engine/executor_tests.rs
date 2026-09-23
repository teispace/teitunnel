//! Executor tests against the fake Cloudflare: end-to-end flows, the staleness guard,
//! confirmation, and rollback with a failure injected at every step.

use cf_api::DnsRecord;
use serde_json::{Map, json};

use super::{
    access::AccessRule,
    activity::ActivityKind,
    executor::{Approval, Context, Engine, EngineError, Outcome, StepState},
    fake::{CloudState, FakeCloud, FakeConnectors},
    local::Local,
    types::{Intent, RouteSpec, ZoneRef},
};
use crate::{
    domain::{Hostname, PrivateNetwork, RouteOrigin},
    store::Store,
};

const CTX: Context<'static> = Context {
    account: "acc",
    machine_name: "Mac",
    tunnel: None,
};

fn engine() -> Engine {
    Engine::new(Local::new(Store::open_in_memory().unwrap()))
}

fn zones() -> CloudState {
    CloudState {
        zones: vec![
            ZoneRef {
                id: "z-xyz".into(),
                name: "xyz.com".into(),
            },
            ZoneRef {
                id: "z-yx".into(),
                name: "yx.com".into(),
            },
        ],
        ..CloudState::default()
    }
}

fn add(id: &str, hostname: &str, origin: &str) -> Intent {
    Intent::AddRoute {
        route: RouteSpec {
            id: id.into(),
            hostname: Hostname::parse(hostname).unwrap(),
            path: None,
            origin: RouteOrigin::parse(origin).unwrap(),
            options: Map::new(),
            access: None,
        },
    }
}

fn me() -> AccessRule {
    AccessRule {
        emails: vec!["me@xyz.com".into()],
        email_domains: Vec::new(),
    }
}

/// `intent`'s route, requiring a login.
fn protected(mut intent: Intent) -> Intent {
    if let Intent::AddRoute { route } | Intent::UpdateRoute { route, .. } = &mut intent {
        route.access = Some(me());
    }
    intent
}

/// An account with Zero Trust set up but no login method yet.
fn zero_trust() -> CloudState {
    CloudState {
        access_org: true,
        ..zones()
    }
}

fn remove(hostname: &str) -> Intent {
    Intent::RemoveRoute {
        hostname: Hostname::parse(hostname).unwrap(),
        path: None,
    }
}

fn share(network: &str) -> Intent {
    Intent::AddNetwork {
        network: PrivateNetwork::parse(network).unwrap(),
    }
}

fn foreign_a(id: &str, name: &str) -> DnsRecord {
    DnsRecord {
        id: id.into(),
        name: name.into(),
        kind: "A".into(),
        content: "192.0.2.10".into(),
        proxied: false,
        comment: Some("the old server".into()),
        ttl: 300,
    }
}

/// Previews and applies `intent` (confirming if needed); panics on engine errors.
async fn run(
    engine: &Engine,
    cloud: &FakeCloud,
    conns: &FakeConnectors,
    intent: &Intent,
) -> Outcome {
    let plan = engine.preview(cloud, CTX, intent).await.unwrap();
    engine
        .apply(
            cloud,
            conns,
            CTX,
            intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: true,
            },
            |_| {},
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn two_domains_from_zero_then_nothing_left() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());

    let outcome = run(&engine, &cloud, &conns, &add("r1", "xyz.com", "3000")).await;
    let Outcome::Applied {
        tunnel_id: Some(tunnel),
        verify,
        connector_error: None,
    } = outcome
    else {
        panic!("{outcome:?}")
    };
    assert_eq!(verify, ["xyz.com"]);
    run(
        &engine,
        &cloud,
        &conns,
        &add("r2", "yx.com", "localhost:5000"),
    )
    .await;

    let state = cloud.snapshot();
    let config = state.tunnels[&tunnel].config.clone().unwrap();
    let services: Vec<_> = config
        .ingress
        .iter()
        .map(|r| (r.hostname.as_deref(), r.service.as_str()))
        .collect();
    assert_eq!(
        services,
        [
            (Some("xyz.com"), "http://localhost:3000"),
            (Some("yx.com"), "http://localhost:5000"),
            (None, "http_status:404")
        ]
    );
    assert_eq!(state.record_count(), 2);
    let record = &state.records["z-yx"][0];
    assert_eq!(record.content, format!("{tunnel}.cfargotunnel.com"));
    assert!(record.proxied);
    assert_eq!(record.comment.as_deref(), Some("teitunnel:route=r2"));
    assert_eq!(engine.local().owned_records("acc").await.unwrap().len(), 2);
    let starts = conns
        .calls()
        .iter()
        .filter(|c| c.starts_with("start"))
        .count();
    assert_eq!(starts, 1, "the running connector is reused");

    // Applying again is a no-op.
    let again = engine
        .preview(&cloud, CTX, &add("r1", "xyz.com", "3000"))
        .await
        .unwrap();
    assert!(again.is_empty());

    run(&engine, &cloud, &conns, &Intent::RemoveTunnel).await;
    let state = cloud.snapshot();
    assert!(state.tunnels.is_empty(), "no tunnel left");
    assert_eq!(state.record_count(), 0, "no DNS records left");
    assert_eq!(engine.local().machine_tunnel("acc").await.unwrap(), None);
    assert!(
        engine
            .local()
            .owned_records("acc")
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        conns
            .calls()
            .ends_with(&[format!("stop {tunnel}"), format!("deleted {tunnel}")])
    );

    let log = engine.local().activity("acc", 10).await.unwrap();
    assert_eq!(log.len(), 3);
    // The wording names the platform ("this Mac", "this PC"…).
    assert_eq!(
        log[0].summary,
        crate::text::msg::plan::summary::remove_tunnel().english()
    );
    assert_eq!(log[2].summary, "Add xyz.com → http://localhost:3000");
    assert!(log.iter().all(|e| e.outcome == "applied"));

    // The structured record: kind, hostnames, step states and what changed.
    let first = log[2].record.as_ref().expect("recorded");
    assert_eq!(first.kind, ActivityKind::AddRoute);
    assert_eq!(first.hostnames, ["xyz.com"]);
    assert!(first.steps.iter().all(|s| s.state == StepState::Done));
    assert!(
        first
            .steps
            .iter()
            .all(|s| s.step.kind != crate::engine::views::StepKind::Verify)
    );
    let removal = log[0].record.as_ref().expect("recorded");
    assert_eq!(removal.kind, ActivityKind::RemoveTunnel);
    assert_eq!(removal.hostnames, ["xyz.com", "yx.com"]);
    insta::assert_yaml_snapshot!(
        "activity_remove_tunnel_changes",
        super::activity::english(&removal.changes)
    );
}

#[tokio::test]
async fn keeps_dashboard_settings_when_writing_the_config() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let tunnel = cloud.snapshot().tunnels.keys().next().unwrap().clone();
    {
        let mut state = cloud.state.lock().unwrap();
        let t = state.tunnels.get_mut(&tunnel).unwrap();
        let config = t.config.as_mut().unwrap();
        config
            .extra
            .insert("warp-routing".into(), json!({ "enabled": true }));
        config
            .origin_request
            .insert("connectTimeout".into(), json!(30));
        t.version += 1;
    }
    run(&engine, &cloud, &conns, &add("r2", "api.xyz.com", "8080")).await;
    let config = cloud.snapshot().tunnels[&tunnel].config.clone().unwrap();
    assert_eq!(config.ingress.len(), 3);
    assert_eq!(config.extra["warp-routing"]["enabled"], true);
    assert_eq!(config.origin_request["connectTimeout"], 30);
}

#[tokio::test]
async fn a_change_after_review_is_caught() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    let intent = add("r1", "app.xyz.com", "3000");
    let reviewed = engine.preview(&cloud, CTX, &intent).await.unwrap();
    cloud
        .state
        .lock()
        .unwrap()
        .records
        .insert("z-xyz".into(), vec![foreign_a("a1", "app.xyz.com")]);

    let err = engine
        .apply(
            &cloud,
            &conns,
            CTX,
            &intent,
            Approval {
                fingerprint: &reviewed.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .unwrap_err();
    let EngineError::Stale(fresh) = err else {
        panic!("{err:?}")
    };
    assert!(
        fresh.requires_confirmation,
        "the new plan flags the foreign record"
    );
    assert_eq!(cloud.mutations(), 0, "nothing was changed");

    // Reviewing the new plan without confirming isn't enough.
    let err = engine
        .apply(
            &cloud,
            &conns,
            CTX,
            &intent,
            Approval {
                fingerprint: &fresh.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::NeedsConfirmation));

    let outcome = engine
        .apply(
            &cloud,
            &conns,
            CTX,
            &intent,
            Approval {
                fingerprint: &fresh.fingerprint,
                confirmed: true,
            },
            |_| {},
        )
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let record = &cloud.snapshot().records["z-xyz"][0];
    assert_eq!((record.id.as_str(), record.kind.as_str()), ("a1", "CNAME"));
}

#[tokio::test]
async fn preview_reuses_a_recent_observation() {
    let (engine, cloud) = (engine(), FakeCloud::new(zones()));
    let intent = add("r1", "app.xyz.com", "3000");
    let first = engine.preview(&cloud, CTX, &intent).await.unwrap();
    cloud
        .state
        .lock()
        .unwrap()
        .records
        .insert("z-xyz".into(), vec![foreign_a("a1", "app.xyz.com")]);
    let cached = engine.preview(&cloud, CTX, &intent).await.unwrap();
    assert_eq!(cached, first, "served from the 5 s cache");
    engine.invalidate("acc");
    let fresh = engine.preview(&cloud, CTX, &intent).await.unwrap();
    assert!(fresh.requires_confirmation);
}

/// A setup and an intent whose plan has several mutations.
async fn scenarios() -> Vec<(&'static str, CloudState, Intent)> {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    run(&engine, &cloud, &conns, &add("r2", "yx.com", "5000")).await;
    let two_routes = cloud.snapshot();
    run(&engine, &cloud, &conns, &share("192.168.1.0/24")).await;
    let routes_and_network = cloud.snapshot();

    let (engine, cloud) = (self::engine(), FakeCloud::new(zero_trust()));
    run(
        &engine,
        &cloud,
        &conns,
        &protected(add("r1", "app.xyz.com", "3000")),
    )
    .await;
    run(&engine, &cloud, &conns, &add("r2", "yx.com", "5000")).await;
    let protected_routes = cloud.snapshot();

    let mut with_foreign = zones();
    with_foreign.records.insert(
        "z-xyz".into(),
        vec![foreign_a("a1", "app.xyz.com"), {
            let mut aaaa = foreign_a("a2", "app.xyz.com");
            aaaa.kind = "AAAA".into();
            aaaa.content = "2001:db8::1".into();
            aaaa
        }],
    );

    vec![
        ("first route", zones(), add("r1", "app.xyz.com", "3000")),
        (
            "replace foreign records",
            with_foreign,
            add("r1", "app.xyz.com", "3000"),
        ),
        (
            "rename",
            two_routes.clone(),
            Intent::UpdateRoute {
                hostname: Hostname::parse("app.xyz.com").unwrap(),
                path: None,
                route: RouteSpec {
                    id: "r3".into(),
                    hostname: Hostname::parse("web.xyz.com").unwrap(),
                    path: None,
                    origin: RouteOrigin::parse("3000").unwrap(),
                    options: Map::new(),
                    access: None,
                },
            },
        ),
        ("remove route", two_routes.clone(), remove("yx.com")),
        ("remove tunnel", two_routes, Intent::RemoveTunnel),
        (
            "first protected route",
            zero_trust(),
            protected(add("r1", "app.xyz.com", "3000")),
        ),
        (
            "rename protected",
            protected_routes.clone(),
            protected(Intent::UpdateRoute {
                hostname: Hostname::parse("app.xyz.com").unwrap(),
                path: None,
                route: RouteSpec {
                    id: "r3".into(),
                    hostname: Hostname::parse("web.xyz.com").unwrap(),
                    path: None,
                    origin: RouteOrigin::parse("3000").unwrap(),
                    options: Map::new(),
                    access: None,
                },
            }),
        ),
        (
            "remove protected route",
            protected_routes.clone(),
            remove("app.xyz.com"),
        ),
        (
            "remove tunnel with a login",
            protected_routes,
            Intent::RemoveTunnel,
        ),
        (
            "first private network",
            CloudState {
                default_vnet: Some("v-default".into()),
                ..zones()
            },
            share("10.0.0.0/24"),
        ),
        (
            "remove tunnel with a private network",
            routes_and_network,
            Intent::RemoveTunnel,
        ),
    ]
}

/// Copies the fake's machine tunnel into a fresh engine's local store.
async fn adopt(engine: &Engine, state: &CloudState) {
    if let Some((id, t)) = state.tunnels.iter().next() {
        engine
            .local()
            .set_machine_tunnel("acc", id, &t.name)
            .await
            .unwrap();
    }
    for (zone, records) in &state.records {
        for r in records.iter().filter(|r| {
            r.comment
                .as_deref()
                .is_some_and(|c| c.starts_with("teitunnel:"))
        }) {
            engine
                .local()
                .own_record("acc", zone, &r.id, &r.name, "r")
                .await
                .unwrap();
        }
    }
    for (id, app) in &state.access_apps {
        if app.name.starts_with("Teitunnel · ") {
            engine
                .local()
                .own_access_app("acc", id, &app.domain)
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn protects_a_route_with_a_login_and_takes_it_down_with_the_route() {
    let (engine, cloud, conns) = (
        engine(),
        FakeCloud::new(zero_trust()),
        FakeConnectors::default(),
    );
    let intent = protected(add("r1", "app.xyz.com", "3000"));
    let outcome = run(&engine, &cloud, &conns, &intent).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    assert_eq!(state.login_methods.len(), 1, "One-time PIN added");
    let (app_id, app) = state.access_apps.iter().next().expect("an application");
    assert_eq!(app.domain, "app.xyz.com");
    assert_eq!(AccessRule::from_new(app), Some(me()));
    assert_eq!(
        engine.local().owned_access_apps("acc").await.unwrap(),
        [(app_id.clone(), "app.xyz.com".to_owned())]
    );
    let log = engine.local().activity("acc", 1).await.unwrap();
    let steps = &log[0].record.as_ref().unwrap().steps;
    assert!(
        steps
            .iter()
            .any(|s| s.step.kind == super::views::StepKind::AccessApp)
    );

    // The overview shows who may sign in.
    let overview = engine.overview(&cloud, &conns, CTX).await.unwrap();
    assert_eq!(overview.routes[0].access, Some(me()));

    // Applying again changes nothing; editing without a login removes it.
    let again = engine.preview(&cloud, CTX, &intent).await.unwrap();
    assert!(again.steps.is_empty(), "{again:?}");
    let open = Intent::UpdateRoute {
        hostname: Hostname::parse("app.xyz.com").unwrap(),
        path: None,
        route: match add("r1", "app.xyz.com", "3000") {
            Intent::AddRoute { route } => route,
            _ => unreachable!(),
        },
    };
    engine.invalidate("acc");
    run(&engine, &cloud, &conns, &open).await;
    assert!(cloud.snapshot().access_apps.is_empty());
    assert!(
        engine
            .local()
            .owned_access_apps("acc")
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        cloud.snapshot().login_methods.len(),
        1,
        "a login method isn't removed once it worked"
    );
}

#[tokio::test]
async fn a_token_that_cant_read_access_still_manages_plain_routes() {
    let (engine, cloud, conns) = (
        engine(),
        FakeCloud::new(zero_trust()),
        FakeConnectors::default(),
    );
    run(
        &engine,
        &cloud,
        &conns,
        &protected(add("r1", "app.xyz.com", "3000")),
    )
    .await;
    cloud.state.lock().unwrap().access_forbidden = true;
    engine.invalidate("acc");
    let overview = engine.overview(&cloud, &conns, CTX).await.unwrap();
    assert_eq!(overview.routes[0].access, None);
    run(&engine, &cloud, &conns, &add("r2", "yx.com", "5000")).await;
    // Asking for a login needs the permission, and says so.
    let err = engine
        .preview(&cloud, CTX, &protected(add("r3", "web.xyz.com", "3000")))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            EngineError::Observe(super::observe::ObserveError::AccessPermission)
        ),
        "{err:?}"
    );
}

#[tokio::test]
async fn a_login_without_zero_trust_is_refused_before_anything_changes() {
    let (engine, cloud) = (engine(), FakeCloud::new(zones()));
    let err = engine
        .preview(&cloud, CTX, &protected(add("r1", "app.xyz.com", "3000")))
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            EngineError::Plan(super::planner::PlanError::ZeroTrustNotSetUp)
        ),
        "{err:?}"
    );
    assert_eq!(cloud.mutations(), 0);
}

#[tokio::test]
async fn a_failure_at_any_step_rolls_everything_back() {
    for (name, initial, intent) in scenarios().await {
        // Count the mutations a clean run makes.
        let probe = FakeCloud::new(initial.clone());
        let e = engine();
        adopt(&e, &initial).await;
        let outcome = run(&e, &probe, &FakeConnectors::default(), &intent).await;
        assert!(
            matches!(outcome, Outcome::Applied { .. }),
            "{name}: {outcome:?}"
        );
        let total = probe.mutations();
        assert!(total >= 2, "{name}: {total} mutations");

        for n in 0..total {
            let cloud = FakeCloud::new(initial.clone());
            let conns = FakeConnectors::default();
            let e = engine();
            adopt(&e, &initial).await;
            cloud.fail_once(n);
            let plan = e.preview(&cloud, CTX, &intent).await.unwrap();
            let mut states = Vec::new();
            let outcome = e
                .apply(
                    &cloud,
                    &conns,
                    CTX,
                    &intent,
                    Approval {
                        fingerprint: &plan.fingerprint,
                        confirmed: true,
                    },
                    |p| {
                        states.push(p);
                    },
                )
                .await
                .unwrap();
            assert!(
                matches!(outcome, Outcome::RolledBack { .. }),
                "{name}, failing mutation {n}: {outcome:?}"
            );
            assert_eq!(
                cloud.snapshot().normalized(),
                initial.normalized(),
                "{name}, failing mutation {n}: state restored"
            );
            assert!(
                states
                    .iter()
                    .any(|p| matches!(p.state, StepState::Failed { .. }))
            );
            let tunnel_after = e
                .local()
                .machine_tunnel("acc")
                .await
                .unwrap()
                .map(|t| t.tunnel_id);
            assert_eq!(
                tunnel_after,
                initial.tunnels.keys().next().cloned(),
                "{name}, failing mutation {n}: local tunnel restored"
            );
            let log = e.local().activity("acc", 1).await.unwrap();
            assert_eq!(log[0].outcome, "rolledBack");
            // Exactly one step failed; the ones before it were undone.
            let steps = &log[0].record.as_ref().expect("recorded").steps;
            let failed = steps
                .iter()
                .position(|s| matches!(s.state, StepState::Failed { .. }))
                .expect("a failed step");
            assert!(
                steps[..failed]
                    .iter()
                    .all(|s| matches!(s.state, StepState::Undone | StepState::Skipped)),
                "{name}, failing mutation {n}: {steps:?}"
            );
        }
    }
}

#[tokio::test]
async fn failed_undo_reports_what_was_left() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    let intent = add("r1", "app.xyz.com", "3000");
    let plan = engine.preview(&cloud, CTX, &intent).await.unwrap();
    // create tunnel (0), put config (1), create record (2) fails, and so does every undo.
    cloud.fail_from(2);
    let outcome = engine
        .apply(
            &cloud,
            &conns,
            CTX,
            &intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: true,
            },
            |_| {},
        )
        .await
        .unwrap();
    let Outcome::PartiallyApplied {
        failed_step,
        leftovers,
        ..
    } = outcome
    else {
        panic!("{outcome:?}")
    };
    assert_eq!(failed_step, 2);
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
    assert!(
        leftovers[0].english().starts_with("Tunnel "),
        "{leftovers:?}"
    );
    let log = engine.local().activity("acc", 1).await.unwrap();
    let states: Vec<_> = log[0]
        .record
        .as_ref()
        .expect("recorded")
        .steps
        .iter()
        .map(|s| s.state.clone())
        .collect();
    assert!(
        matches!(states[0], StepState::UndoFailed { .. }),
        "{states:?}"
    );
    assert!(matches!(states[2], StepState::Failed { .. }), "{states:?}");

    // The tunnel is still remembered, so the next attempt reuses it.
    cloud.reset_failures();
    engine.invalidate("acc");
    let outcome = run(&engine, &cloud, &conns, &intent).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(cloud.snapshot().tunnels.len(), 1);
}

#[tokio::test]
async fn a_tunnel_deleted_in_the_dashboard_is_recreated() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    cloud.state.lock().unwrap().tunnels.clear();
    engine.invalidate("acc");

    let plan = engine
        .preview(&cloud, CTX, &add("r2", "api.xyz.com", "8080"))
        .await
        .unwrap();
    assert!(matches!(
        plan.steps[0],
        super::types::Step::CreateTunnel { .. }
    ));
    run(&engine, &cloud, &conns, &add("r2", "api.xyz.com", "8080")).await;
    let state = cloud.snapshot();
    let (id, tunnel) = state.tunnels.iter().next().unwrap();
    assert_eq!(tunnel.name, "Mac");
    assert_eq!(
        engine
            .local()
            .machine_tunnel("acc")
            .await
            .unwrap()
            .unwrap()
            .tunnel_id,
        *id
    );
}

#[tokio::test]
async fn a_connector_that_wont_stop_keeps_the_tunnel() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let before = cloud.snapshot();
    conns
        .fail_stop
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let outcome = run(&engine, &cloud, &conns, &Intent::RemoveTunnel).await;
    assert!(matches!(outcome, Outcome::RolledBack { .. }), "{outcome:?}");
    assert_eq!(cloud.snapshot().normalized(), before.normalized());
    assert!(
        engine
            .local()
            .machine_tunnel("acc")
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn verifies_a_route_through_the_edge() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

    use super::verify::{Edge, Failure};

    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    let edge = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200))
        .mount(&edge)
        .await;
    let host = Hostname::parse("app.xyz.com").unwrap();
    let patience = std::time::Duration::ZERO;

    let missing = engine
        .verify(&cloud, CTX, &host, Edge::Test(*edge.address()), patience)
        .await
        .unwrap();
    assert_eq!(missing.failure, Some(Failure::NoRecord));

    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let ok = engine
        .verify(&cloud, CTX, &host, Edge::Test(*edge.address()), patience)
        .await
        .unwrap();
    assert!(ok.ok(), "{ok:?}");
    assert_eq!(ok.status, Some(200));

    // Someone repoints the record in the dashboard.
    cloud
        .state
        .lock()
        .unwrap()
        .records
        .get_mut("z-xyz")
        .unwrap()[0]
        .content = "elsewhere.example".into();
    let moved = engine
        .verify(&cloud, CTX, &host, Edge::Test(*edge.address()), patience)
        .await
        .unwrap();
    assert!(
        matches!(moved.failure, Some(Failure::RecordElsewhere { .. })),
        "{moved:?}"
    );
}

#[tokio::test]
async fn detects_outside_edits_and_resolves_them() {
    use cf_api::IngressRule;

    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    assert_eq!(engine.drift(&cloud, "acc", None).await.unwrap(), None);
    let tunnel = cloud.snapshot().tunnels.keys().next().unwrap().clone();

    let edit = |service: &str| {
        let mut state = cloud.state.lock().unwrap();
        let t = state.tunnels.get_mut(&tunnel).unwrap();
        let config = t.config.as_mut().unwrap();
        config.ingress.insert(
            0,
            IngressRule {
                hostname: Some("dash.xyz.com".into()),
                path: None,
                service: service.into(),
                origin_request: Map::new(),
                extra: Map::new(),
            },
        );
        t.version += 1;
    };

    // Keep theirs: the edit becomes the baseline.
    edit("http://localhost:9000");
    let drift = engine
        .drift(&cloud, "acc", None)
        .await
        .unwrap()
        .expect("drift");
    assert_eq!(drift.changes.len(), 1);
    assert_eq!(drift.changes[0].hostname, "dash.xyz.com");
    assert_eq!(drift.changes[0].before, None);
    engine.keep_theirs("acc", &drift).await.unwrap();
    assert_eq!(engine.drift(&cloud, "acc", None).await.unwrap(), None);

    // Restore mine: a plan puts Teitunnel's routes back.
    edit("http://localhost:9001");
    let drift = engine
        .drift(&cloud, "acc", None)
        .await
        .unwrap()
        .expect("drift");
    let restore = Intent::RestoreConfig {
        ingress: drift.ours.clone(),
    };
    let outcome = run(&engine, &cloud, &conns, &restore).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let ingress = cloud.snapshot().tunnels[&tunnel]
        .config
        .clone()
        .unwrap()
        .ingress;
    assert_eq!(ingress, drift.ours);
    assert_eq!(engine.drift(&cloud, "acc", None).await.unwrap(), None);

    // A change that touches no route (the catch-all) is adopted silently.
    {
        let mut state = cloud.state.lock().unwrap();
        let t = state.tunnels.get_mut(&tunnel).unwrap();
        t.config
            .as_mut()
            .unwrap()
            .ingress
            .last_mut()
            .unwrap()
            .service = "http_status:503".into();
        t.version += 1;
    }
    assert_eq!(engine.drift(&cloud, "acc", None).await.unwrap(), None);
}

#[tokio::test]
async fn changes_from_the_ui_become_intents() {
    use super::views::{Change, RouteInput};

    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    let input = |host: &str, origin: &str| RouteInput {
        hostname: host.into(),
        path: None,
        origin: origin.into(),
        access: None,
    };
    let add = engine
        .intent_for(
            &cloud,
            CTX,
            &Change::AddRoute {
                route: input("app.xyz.com", "3000"),
            },
        )
        .await
        .unwrap();
    run(&engine, &cloud, &conns, &add).await;

    // An option set in the dashboard survives an edit made in Teitunnel.
    let tunnel = cloud.snapshot().tunnels.keys().next().unwrap().clone();
    {
        let mut state = cloud.state.lock().unwrap();
        let t = state.tunnels.get_mut(&tunnel).unwrap();
        t.config.as_mut().unwrap().ingress[0]
            .origin_request
            .insert("noTLSVerify".into(), json!(true));
        t.version += 1;
    }
    engine.invalidate("acc");
    let edit = Change::UpdateRoute {
        hostname: "app.xyz.com".into(),
        path: None,
        route: input("app.xyz.com", "4000"),
    };
    let intent = engine.intent_for(&cloud, CTX, &edit).await.unwrap();
    // The dashboard edit is drift; keep it, then apply the change.
    let drift = engine.drift(&cloud, "acc", None).await.unwrap().unwrap();
    engine.keep_theirs("acc", &drift).await.unwrap();
    run(&engine, &cloud, &conns, &intent).await;
    let rule = &cloud.snapshot().tunnels[&tunnel]
        .config
        .clone()
        .unwrap()
        .ingress[0];
    assert_eq!(rule.service, "http://localhost:4000");
    assert_eq!(rule.origin_request["noTLSVerify"], true);

    let overview = engine.overview(&cloud, &conns, CTX).await.unwrap();
    assert_eq!(overview.routes.len(), 1);
    assert_eq!(overview.routes[0].dns, super::views::DnsState::Ok);
    assert!(overview.tunnel.unwrap().connector.is_some());

    let bad = engine
        .intent_for(
            &cloud,
            CTX,
            &Change::AddRoute {
                route: input("nodot", "3000"),
            },
        )
        .await
        .unwrap_err();
    assert!(
        matches!(bad, EngineError::Input(ref e) if e.field == "hostname"),
        "{bad:?}"
    );
    let nothing = engine
        .intent_for(&cloud, CTX, &Change::RestoreConfig)
        .await
        .unwrap_err();
    assert!(matches!(nothing, EngineError::NothingToRestore));
}

#[tokio::test]
async fn lists_tunnels_with_this_macs_first() {
    let mut state = zones();
    state.tunnels.insert(
        "a-other".into(),
        super::fake::FakeTunnel {
            name: "Build server".into(),
            version: 1,
            config: None,
        },
    );
    let (engine, cloud, conns) = (engine(), FakeCloud::new(state), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let tunnels = engine.tunnels(&cloud, &conns, "acc").await.unwrap();
    assert_eq!(tunnels.len(), 2);
    assert!(tunnels[0].this_mac);
    assert_eq!(tunnels[0].routes, Some(1));
    assert!(tunnels[0].connector.is_some());
    assert_eq!(tunnels[1].name, "Build server");
    assert!(!tunnels[1].this_mac);
    assert_eq!(tunnels[1].connector, None);
}

#[tokio::test]
async fn deletes_single_records_with_confirmation_for_foreign_ones() {
    use super::views::Change;

    let mut state = zones();
    state.records.insert(
        "z-xyz".into(),
        vec![
            {
                let mut r = foreign_a("mine", "old.xyz.com");
                r.comment = Some("teitunnel:route=abc".into());
                r
            },
            foreign_a("theirs", "blog.xyz.com"),
        ],
    );
    let (engine, cloud, conns) = (engine(), FakeCloud::new(state), FakeConnectors::default());
    let delete = |hostname: &str, id: &str| Change::DeleteRecord {
        zone_id: "z-xyz".into(),
        hostname: hostname.into(),
        record_id: id.into(),
    };

    let owned = engine
        .intent_for(&cloud, CTX, &delete("old.xyz.com", "mine"))
        .await
        .unwrap();
    let plan = engine.preview(&cloud, CTX, &owned).await.unwrap();
    assert!(!plan.requires_confirmation);
    run(&engine, &cloud, &conns, &owned).await;

    let theirs = engine
        .intent_for(&cloud, CTX, &delete("blog.xyz.com", "theirs"))
        .await
        .unwrap();
    let plan = engine.preview(&cloud, CTX, &theirs).await.unwrap();
    assert!(
        plan.requires_confirmation,
        "deleting someone else's record needs a yes"
    );
    run(&engine, &cloud, &conns, &theirs).await;
    assert_eq!(cloud.snapshot().record_count(), 0);

    let gone = engine
        .intent_for(&cloud, CTX, &delete("blog.xyz.com", "theirs"))
        .await
        .unwrap();
    let err = engine.preview(&cloud, CTX, &gone).await.unwrap_err();
    assert!(matches!(err, EngineError::Plan(_)), "{err:?}");
}

/// "Fix all safe issues" repairs owned DNS and deletes owned orphans, and never touches
/// a record Teitunnel didn't create, whatever the mix.
#[tokio::test]
async fn fixing_safe_issues_never_touches_foreign_records() {
    use crate::doctor::{BinaryFact, Facts, diagnose, fix_safe, gather};

    for seed in 0..8u32 {
        let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
        run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
        run(&engine, &cloud, &conns, &add("r2", "yx.com", "5000")).await;
        let tunnel = cloud.snapshot().tunnels.keys().next().unwrap().clone();
        {
            let mut state = cloud.state.lock().unwrap();
            let target = format!("{tunnel}.cfargotunnel.com");
            let xyz = state.records.get_mut("z-xyz").unwrap();
            if seed & 1 == 1 {
                xyz.clear(); // dns.missing for app.xyz.com (owned fix)
            }
            let mut orphan = foreign_a("orph", "old.xyz.com");
            orphan.kind = "CNAME".into();
            orphan.content = target.clone();
            orphan.comment = Some("teitunnel:route=x".into());
            xyz.push(orphan);
            let mut theirs = foreign_a("theirs", "legacy.xyz.com");
            theirs.kind = "CNAME".into();
            theirs.content = "dead-tunnel.cfargotunnel.com".into();
            xyz.push(theirs);
            if seed & 2 == 2 {
                // A foreign A record blocks yx.com: fixing it needs a yes, so it's skipped.
                let yx = state.records.get_mut("z-yx").unwrap();
                yx.clear();
                yx.push(foreign_a("blocker", "yx.com"));
            }
            if seed & 4 == 4 {
                let yx = state.records.get_mut("z-yx").unwrap();
                if let Some(r) = yx.iter_mut().find(|r| r.kind == "CNAME") {
                    r.proxied = false;
                }
            }
        }
        engine.invalidate("acc");
        let facts = gather(&engine, &cloud, &conns, CTX, Vec::new(), Some(true))
            .await
            .unwrap();
        let issues = diagnose(&Facts {
            binary: BinaryFact::Ok,
            accounts: vec![facts],
            foreign: Vec::new(),
        });
        let before = cloud.snapshot();
        let report = fix_safe(&engine, &cloud, &conns, CTX, &issues).await;
        let after = cloud.snapshot();

        let foreign_ids = ["theirs", "blocker"];
        for id in foreign_ids {
            let was = before.records.values().flatten().find(|r| r.id == id);
            let now = after.records.values().flatten().find(|r| r.id == id);
            assert_eq!(was, now, "seed {seed}: foreign record {id} was touched");
        }
        assert!(
            !after.records.values().flatten().any(|r| r.id == "orph"),
            "seed {seed}: the owned orphan is deleted"
        );
        assert!(report.fixed >= 1, "seed {seed}: {report:?}");
        assert!(report.failed.is_empty(), "seed {seed}: {report:?}");
    }
}

#[tokio::test]
async fn a_login_left_behind_by_an_outside_edit_is_found_and_removed() {
    use crate::doctor::{BinaryFact, Facts, diagnose, fix_safe, gather};

    let (engine, cloud, conns) = (
        engine(),
        FakeCloud::new(zero_trust()),
        FakeConnectors::default(),
    );
    run(
        &engine,
        &cloud,
        &conns,
        &protected(add("r1", "app.xyz.com", "3000")),
    )
    .await;
    run(&engine, &cloud, &conns, &add("r2", "yx.com", "5000")).await;
    // Someone else's application, for a hostname with no route: never touched.
    cloud.state.lock().unwrap().access_apps.insert(
        "theirs".into(),
        super::access::app_definition("old.xyz.com", &me()),
    );
    let facts = gather(&engine, &cloud, &conns, CTX, Vec::new(), Some(true))
        .await
        .unwrap();
    assert!(
        facts.orphan_logins.is_empty(),
        "a routed login isn't an orphan"
    );

    // The route is removed in the dashboard; its login stays.
    {
        let mut state = cloud.state.lock().unwrap();
        let tunnel = state.tunnels.values_mut().next().unwrap();
        let config = tunnel.config.as_mut().unwrap();
        config
            .ingress
            .retain(|r| r.hostname.as_deref() != Some("app.xyz.com"));
    }
    engine.invalidate("acc");
    let facts = gather(&engine, &cloud, &conns, CTX, Vec::new(), Some(true))
        .await
        .unwrap();
    assert_eq!(facts.orphan_logins, ["app.xyz.com"]);
    let issues = diagnose(&Facts {
        binary: BinaryFact::Ok,
        accounts: vec![facts],
        foreign: Vec::new(),
    });
    let report = fix_safe(&engine, &cloud, &conns, CTX, &issues).await;
    assert!(report.failed.is_empty(), "{report:?}");
    let apps = cloud.snapshot().access_apps;
    assert_eq!(apps.keys().collect::<Vec<_>>(), ["theirs"]);
    assert!(
        engine
            .local()
            .owned_access_apps("acc")
            .await
            .unwrap()
            .is_empty()
    );
    let log = engine.local().activity("acc", 1).await.unwrap();
    assert_eq!(log[0].summary, "Remove the login from app.xyz.com");
}

#[tokio::test]
async fn imports_routes_from_an_old_tunnel() {
    use super::views::{Change, RouteInput};

    // The old, locally-managed tunnel's DNS record for app.xyz.com (not ours).
    let mut state = zones();
    let mut old = foreign_a("old", "app.xyz.com");
    old.kind = "CNAME".into();
    old.content = "old-tunnel.cfargotunnel.com".into();
    old.proxied = true;
    state.records.insert("z-xyz".into(), vec![old]);
    let (engine, cloud, conns) = (engine(), FakeCloud::new(state), FakeConnectors::default());
    let input = |host: &str, path: Option<&str>, origin: &str| RouteInput {
        hostname: host.into(),
        path: path.map(str::to_owned),
        origin: origin.into(),
        access: None,
    };
    let change = Change::ImportRoutes {
        routes: vec![
            input("app.xyz.com", None, "http://localhost:3000"),
            input("app.xyz.com", Some("^/api/"), "http://localhost:8080"),
            input("yx.com", None, "http://localhost:5000"),
        ],
    };
    let intent = engine.intent_for(&cloud, CTX, &change).await.unwrap();
    let plan = engine.preview(&cloud, CTX, &intent).await.unwrap();
    assert!(
        plan.requires_confirmation,
        "the old tunnel's record isn't ours"
    );
    let verifies = plan
        .steps
        .iter()
        .filter(|s| matches!(s, super::types::Step::Verify { .. }))
        .count();
    assert_eq!(verifies, 2, "one check per hostname");
    run(&engine, &cloud, &conns, &intent).await;

    let state = cloud.snapshot();
    let tunnel = state.tunnels.keys().next().unwrap();
    let ingress = &state.tunnels[tunnel].config.as_ref().unwrap().ingress;
    assert_eq!(ingress.len(), 4, "three routes and the catch-all");
    assert_eq!(
        ingress[0].path.as_deref(),
        Some("^/api/"),
        "longer paths first"
    );
    assert_eq!(state.record_count(), 2);
    assert!(
        state.records["z-xyz"][0]
            .content
            .starts_with(tunnel.as_str())
    );

    // Importing the same routes again changes nothing.
    engine.invalidate("acc");
    let again = engine.intent_for(&cloud, CTX, &change).await.unwrap();
    assert!(
        engine
            .preview(&cloud, CTX, &again)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn exports_the_routes_and_records_as_they_are() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    assert!(
        engine
            .export_input(&cloud, CTX, None)
            .await
            .unwrap()
            .is_none(),
        "nothing to export without a tunnel"
    );
    run(&engine, &cloud, &conns, &add("r1", "xyz.com", "3000")).await;
    run(&engine, &cloud, &conns, &add("r2", "yx.com", "5000")).await;
    engine.invalidate("acc");

    let input = engine
        .export_input(&cloud, CTX, Some("2026.9.1".into()))
        .await
        .unwrap()
        .expect("a tunnel");
    let hostnames: Vec<_> = input
        .ingress
        .iter()
        .filter_map(|r| r.hostname.as_deref())
        .collect();
    assert_eq!(hostnames, ["xyz.com", "yx.com"]);
    let records: Vec<_> = input
        .records
        .iter()
        .map(|r| (r.hostname.as_str(), r.comment.as_deref()))
        .collect();
    assert_eq!(
        records,
        [
            ("xyz.com", Some("teitunnel:route=r1")),
            ("yx.com", Some("teitunnel:route=r2"))
        ]
    );
    let terraform = crate::export::render(&input, crate::export::ExportFormat::Terraform);
    assert_eq!(terraform.contents.matches("import {").count(), 4);
}

#[tokio::test]
async fn shares_a_private_network_and_stops_sharing_it() {
    let (engine, cloud, conns) = (
        engine(),
        FakeCloud::new(CloudState {
            default_vnet: Some("v-default".into()),
            ..zones()
        }),
        FakeConnectors::default(),
    );
    let outcome = run(&engine, &cloud, &conns, &share("192.168.1.7/24")).await;
    let Outcome::Applied {
        tunnel_id: Some(tunnel),
        verify,
        connector_error: None,
    } = outcome
    else {
        panic!("{outcome:?}")
    };
    assert!(verify.is_empty(), "there's no hostname to check");
    assert!(
        conns.calls().iter().any(|c| c.starts_with("start")),
        "the connector carries the traffic"
    );
    let state = cloud.snapshot();
    let route = state.network_routes.values().next().unwrap();
    assert_eq!(
        (
            route.network.as_str(),
            route.tunnel_id.as_str(),
            route.comment.as_str(),
            route.virtual_network_id.as_deref()
        ),
        (
            "192.168.1.0/24",
            tunnel.as_str(),
            "Added by Teitunnel",
            Some("v-default")
        )
    );

    engine.invalidate("acc");
    let overview = engine.overview(&cloud, &conns, CTX).await.unwrap();
    let networks = overview.networks.unwrap();
    assert_eq!(networks.len(), 1);
    assert_eq!(networks[0].network, "192.168.1.0/24");
    assert!(networks[0].private && networks[0].owned);

    assert!(
        engine
            .preview(&cloud, CTX, &share("192.168.1.0/24"))
            .await
            .unwrap()
            .is_empty()
    );
    let stop = Intent::RemoveNetwork {
        network: PrivateNetwork::parse("192.168.1.0/24").unwrap(),
    };
    run(&engine, &cloud, &conns, &stop).await;
    assert!(cloud.snapshot().network_routes.is_empty());
    assert_eq!(cloud.snapshot().tunnels.len(), 1, "the tunnel stays");

    let log = engine.local().activity("acc", 10).await.unwrap();
    assert_eq!(
        log[0].summary,
        "Stop sharing private network 192.168.1.0/24"
    );
    assert_eq!(
        log[0].record.as_ref().unwrap().kind,
        ActivityKind::RemoveNetwork
    );
    assert_eq!(log[1].summary, "Share private network 192.168.1.0/24");
}

#[tokio::test]
async fn a_token_that_cant_read_networks_still_manages_routes() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    cloud.state.lock().unwrap().networks_forbidden = true;
    engine.invalidate("acc");
    let overview = engine.overview(&cloud, &conns, CTX).await.unwrap();
    assert_eq!(overview.routes.len(), 1);
    assert_eq!(overview.networks, None);
    let err = engine
        .preview(&cloud, CTX, &share("10.0.0.0/24"))
        .await
        .unwrap_err();
    assert!(matches!(err, EngineError::Observe(_)), "{err:?}");
    run(&engine, &cloud, &conns, &Intent::RemoveTunnel).await;
    assert!(cloud.snapshot().tunnels.is_empty());
}

/// Previews and applies `intent` on one of this Mac's tunnels (`None`: the default).
async fn run_on(
    engine: &Engine,
    cloud: &FakeCloud,
    conns: &FakeConnectors,
    tunnel: Option<&str>,
    intent: &Intent,
) -> Result<Outcome, EngineError> {
    let ctx = Context { tunnel, ..CTX };
    let plan = engine.preview(cloud, ctx, intent).await?;
    engine
        .apply(
            cloud,
            conns,
            ctx,
            intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: true,
            },
            |_| {},
        )
        .await
}

#[tokio::test]
async fn several_tunnels_on_one_machine() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let default = engine.local().machine_tunnel("acc").await.unwrap().unwrap();

    // A second tunnel: its own name, not the default, no connector until it has routes.
    let create = Intent::CreateTunnel {
        name: "staging".into(),
    };
    let outcome = run_on(&engine, &cloud, &conns, None, &create)
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let tunnels = engine.local().tunnels("acc").await.unwrap();
    assert_eq!(tunnels.len(), 2);
    let staging = tunnels.iter().find(|t| !t.is_default).unwrap().clone();
    assert_eq!(staging.name, "staging");
    assert_eq!(
        engine.local().machine_tunnel("acc").await.unwrap().unwrap(),
        default,
        "the default tunnel is unchanged"
    );

    // The name is the account's, case-insensitively; blank names are refused.
    let taken = Intent::CreateTunnel {
        name: "STAGING".into(),
    };
    assert!(matches!(
        run_on(&engine, &cloud, &conns, None, &taken).await,
        Err(EngineError::Plan(
            super::planner::PlanError::TunnelNameTaken(_)
        ))
    ));
    let blank = Intent::CreateTunnel { name: "  ".into() };
    assert!(matches!(
        run_on(&engine, &cloud, &conns, None, &blank).await,
        Err(EngineError::Plan(
            super::planner::PlanError::InvalidTunnelName
        ))
    ));

    // A route on the second tunnel lands there, and its DNS points there.
    let on_staging = Some(staging.tunnel_id.as_str());
    let outcome = run_on(
        &engine,
        &cloud,
        &conns,
        on_staging,
        &add("r2", "beta.xyz.com", "4000"),
    )
    .await
    .unwrap();
    let Outcome::Applied {
        tunnel_id: Some(id),
        ..
    } = outcome
    else {
        panic!("{outcome:?}");
    };
    assert_eq!(id, staging.tunnel_id);
    let state = cloud.snapshot();
    let rules = |id: &str| -> Vec<String> {
        state.tunnels[id]
            .config
            .as_ref()
            .unwrap()
            .ingress
            .iter()
            .filter_map(|r| r.hostname.clone())
            .collect()
    };
    assert_eq!(rules(&default.tunnel_id), ["app.xyz.com"]);
    assert_eq!(rules(&staging.tunnel_id), ["beta.xyz.com"]);
    let beta = &state.records["z-xyz"]
        .iter()
        .find(|r| r.name == "beta.xyz.com")
        .unwrap()
        .content;
    assert_eq!(*beta, super::types::tunnel_target(&staging.tunnel_id));

    // A hostname is routed once per machine: not again on the other tunnel.
    let again = run_on(
        &engine,
        &cloud,
        &conns,
        None,
        &add("r3", "beta.xyz.com", "5000"),
    )
    .await;
    assert!(
        matches!(
            &again,
            Err(EngineError::Plan(super::planner::PlanError::RoutedElsewhere { tunnel, .. }))
                if tunnel == "staging"
        ),
        "{again:?}"
    );

    // The overview has both tunnels (default first) and knows which carries each route.
    let overview = engine.overview(&cloud, &conns, CTX).await.unwrap();
    assert_eq!(
        overview
            .tunnels
            .iter()
            .map(|t| (t.name.as_str(), t.is_default))
            .collect::<Vec<_>>(),
        [("Mac", true), ("staging", false)]
    );
    let carriers: Vec<(&str, Option<&str>)> = overview
        .routes
        .iter()
        .map(|r| (r.hostname.as_str(), r.tunnel_id.as_deref()))
        .collect();
    assert_eq!(
        carriers,
        [
            ("app.xyz.com", Some(default.tunnel_id.as_str())),
            ("beta.xyz.com", on_staging)
        ]
    );
    assert!(
        overview
            .routes
            .iter()
            .all(|r| r.dns == super::views::DnsState::Ok),
        "{:?}",
        overview.routes
    );

    // Removing the second tunnel leaves the default one and its route alone.
    run_on(&engine, &cloud, &conns, on_staging, &Intent::RemoveTunnel)
        .await
        .unwrap();
    let left = engine.local().tunnels("acc").await.unwrap();
    assert_eq!(left, vec![default.clone()]);
    let state = cloud.snapshot();
    assert!(!state.tunnels.contains_key(&staging.tunnel_id));
    assert!(
        state.records["z-xyz"]
            .iter()
            .all(|r| r.name != "beta.xyz.com")
    );
    assert_eq!(rules_of(&state, &default.tunnel_id), ["app.xyz.com"]);

    // A tunnel that isn't this machine's can't be targeted.
    assert!(matches!(
        run_on(
            &engine,
            &cloud,
            &conns,
            Some("nope"),
            &add("r4", "x.xyz.com", "1")
        )
        .await,
        Err(EngineError::Observe(
            super::observe::ObserveError::UnknownTunnel
        ))
    ));
}

fn rules_of(state: &CloudState, id: &str) -> Vec<String> {
    state.tunnels[id]
        .config
        .as_ref()
        .unwrap()
        .ingress
        .iter()
        .filter_map(|r| r.hostname.clone())
        .collect()
}

#[tokio::test]
async fn the_doctor_checks_and_fixes_each_tunnel_on_its_own() {
    use crate::doctor::{BinaryFact, Facts, diagnose, fix_safe, gather};
    let (engine, cloud, conns) = (
        engine(),
        FakeCloud::new(zero_trust()),
        FakeConnectors::default(),
    );
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let create = Intent::CreateTunnel {
        name: "staging".into(),
    };
    run_on(&engine, &cloud, &conns, None, &create)
        .await
        .unwrap();
    let staging = engine
        .local()
        .tunnels("acc")
        .await
        .unwrap()
        .into_iter()
        .find(|t| !t.is_default)
        .unwrap();
    let on = Some(staging.tunnel_id.as_str());
    run_on(
        &engine,
        &cloud,
        &conns,
        on,
        &protected(add("r2", "beta.xyz.com", "4000")),
    )
    .await
    .unwrap();

    // Checking the default tunnel: the second tunnel's login belongs to a route.
    let facts = gather(&engine, &cloud, &conns, CTX, Vec::new(), Some(true))
        .await
        .unwrap();
    assert!(facts.orphan_logins.is_empty(), "{:?}", facts.orphan_logins);

    // The second tunnel's DNS record goes missing: its issue names that tunnel, and the
    // safe fix puts the record back pointing there.
    cloud
        .state
        .lock()
        .unwrap()
        .records
        .get_mut("z-xyz")
        .unwrap()
        .retain(|r| r.name != "beta.xyz.com");
    engine.invalidate("acc");
    let ctx = Context { tunnel: on, ..CTX };
    let facts = gather(&engine, &cloud, &conns, ctx, Vec::new(), Some(true))
        .await
        .unwrap();
    let issues = diagnose(&Facts {
        binary: BinaryFact::Ok,
        accounts: vec![facts],
        foreign: Vec::new(),
    });
    let missing = issues
        .iter()
        .find(|i| i.check == "dns.missing")
        .expect("a missing record");
    assert_eq!(missing.tunnel_id.as_deref(), on);
    let report = fix_safe(&engine, &cloud, &conns, CTX, &issues).await;
    assert_eq!(report.fixed, 1, "{report:?}");
    let restored = cloud.snapshot().records["z-xyz"]
        .iter()
        .find(|r| r.name == "beta.xyz.com")
        .map(|r| r.content.clone());
    assert_eq!(
        restored,
        Some(super::types::tunnel_target(&staging.tunnel_id))
    );
    // And the default tunnel still carries only its own route.
    let default = engine.local().machine_tunnel("acc").await.unwrap().unwrap();
    assert_eq!(
        rules_of(&cloud.snapshot(), &default.tunnel_id),
        ["app.xyz.com"]
    );
}

/// Another machine's tunnel in the account that routes `hostname` too.
fn other_machine(cloud: &FakeCloud, hostname: &str) -> String {
    let mut state = cloud.state.lock().unwrap();
    state.load_balancing = true;
    let id = "00000000-0000-4000-8000-0000000000b2".to_owned();
    state.tunnels.insert(
        id.clone(),
        super::fake::FakeTunnel {
            name: "server".into(),
            version: 1,
            config: Some(cf_api::TunnelConfig {
                ingress: vec![
                    cf_api::IngressRule {
                        hostname: Some(hostname.into()),
                        path: None,
                        service: "http://localhost:3000".into(),
                        origin_request: Map::new(),
                        extra: Map::new(),
                    },
                    cf_api::IngressRule {
                        hostname: None,
                        path: None,
                        service: "http_status:404".into(),
                        origin_request: Map::new(),
                        extra: Map::new(),
                    },
                ],
                origin_request: Map::new(),
                extra: Map::new(),
            }),
        },
    );
    id
}

fn balance(hostname: &str) -> Intent {
    Intent::BalanceRoute {
        hostname: Hostname::parse(hostname).unwrap(),
    }
}

fn unbalance(hostname: &str) -> Intent {
    Intent::UnbalanceRoute {
        hostname: Hostname::parse(hostname).unwrap(),
    }
}

#[tokio::test]
async fn load_balances_a_route_across_machines_and_back() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let ours = engine.local().machine_tunnel("acc").await.unwrap().unwrap();
    let theirs = other_machine(&cloud, "app.xyz.com");

    let plan = engine
        .preview(&cloud, CTX, &balance("app.xyz.com"))
        .await
        .unwrap();
    let kinds: Vec<&str> = plan
        .steps
        .iter()
        .map(|s| match s {
            super::types::Step::CreateLbMonitor { .. } => "monitor+",
            super::types::Step::CreateLbPool { .. } => "pool+",
            super::types::Step::CreateLoadBalancer { .. } => "lb+",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["monitor+", "pool+", "lb+"]);
    assert!(
        plan.warnings.is_empty(),
        "two machines: no single-endpoint warning"
    );

    run(&engine, &cloud, &conns, &balance("app.xyz.com")).await;
    let state = cloud.snapshot();
    let (_, lb) = state
        .load_balancers
        .values()
        .next()
        .expect("a load balancer");
    assert_eq!(lb.name, "app.xyz.com");
    assert!(lb.proxied);
    let pool = state.lb_pools.values().next().expect("a pool");
    let mut addresses: Vec<&str> = pool.origins.iter().map(|o| o.address.as_str()).collect();
    addresses.sort_unstable();
    let mut expected = vec![
        super::types::tunnel_target(&ours.tunnel_id),
        super::types::tunnel_target(&theirs),
    ];
    expected.sort();
    assert_eq!(addresses, expected);
    assert!(
        pool.origins
            .iter()
            .all(|o| o.header["Host"] == ["app.xyz.com"])
    );
    assert_eq!(
        pool.monitor.as_deref(),
        state.lb_monitors.keys().next().map(String::as_str)
    );
    assert!(
        engine
            .local()
            .balanced("acc")
            .await
            .unwrap()
            .contains("app.xyz.com")
    );

    // Applying again changes nothing.
    let again = engine
        .preview(&cloud, CTX, &balance("app.xyz.com"))
        .await
        .unwrap();
    assert!(again.steps.is_empty(), "{:?}", again.steps);

    // Stopping removes all three, and only them.
    run(&engine, &cloud, &conns, &unbalance("app.xyz.com")).await;
    let state = cloud.snapshot();
    assert!(
        state.load_balancers.is_empty()
            && state.lb_pools.is_empty()
            && state.lb_monitors.is_empty()
    );
    assert!(
        !engine
            .local()
            .balanced("acc")
            .await
            .unwrap()
            .contains("app.xyz.com")
    );
    assert!(matches!(
        engine.preview(&cloud, CTX, &unbalance("app.xyz.com")).await,
        Err(EngineError::Plan(super::planner::PlanError::NotBalanced(_)))
    ));
}

#[tokio::test]
async fn load_balancing_rolls_back_completely() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    other_machine(&cloud, "app.xyz.com");
    let before = cloud.snapshot();

    // The load balancer fails: the pool and monitor made before it go.
    let intent = balance("app.xyz.com");
    let plan = engine.preview(&cloud, CTX, &intent).await.unwrap();
    cloud.reset_failures();
    cloud.fail_once(2);
    let outcome = engine
        .apply(
            &cloud,
            &conns,
            CTX,
            &intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::RolledBack { .. }), "{outcome:?}");
    assert_eq!(cloud.snapshot().normalized(), before.normalized());
    let state = cloud.snapshot();
    assert!(state.lb_pools.is_empty() && state.lb_monitors.is_empty());

    // Stopping fails at the last step: the load balancer and pool come back, the pool
    // pointing at the monitor and the load balancer at the recreated pool.
    cloud.reset_failures();
    run(&engine, &cloud, &conns, &balance("app.xyz.com")).await;
    let intent = unbalance("app.xyz.com");
    let plan = engine.preview(&cloud, CTX, &intent).await.unwrap();
    cloud.reset_failures();
    cloud.fail_once(2);
    let outcome = engine
        .apply(
            &cloud,
            &conns,
            CTX,
            &intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::RolledBack { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    let (_, lb) = state
        .load_balancers
        .values()
        .next()
        .expect("the load balancer is back");
    let pool = state.lb_pools.values().next().expect("the pool is back");
    assert_eq!(lb.default_pools, std::slice::from_ref(&pool.id));
    assert_eq!(lb.fallback_pool, pool.id);
    assert!(
        state
            .lb_monitors
            .contains_key(pool.monitor.as_deref().unwrap())
    );
    assert!(
        engine
            .local()
            .balanced("acc")
            .await
            .unwrap()
            .contains("app.xyz.com")
    );
}

#[tokio::test]
async fn a_route_leaving_a_balanced_hostname_leaves_its_pool() {
    let (engine, cloud, conns) = (engine(), FakeCloud::new(zones()), FakeConnectors::default());
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let ours = engine.local().machine_tunnel("acc").await.unwrap().unwrap();
    let theirs = other_machine(&cloud, "app.xyz.com");
    run(&engine, &cloud, &conns, &balance("app.xyz.com")).await;

    run(&engine, &cloud, &conns, &remove("app.xyz.com")).await;
    let state = cloud.snapshot();
    let pool = state
        .lb_pools
        .values()
        .next()
        .expect("the pool stays for the other machine");
    let addresses: Vec<&str> = pool.origins.iter().map(|o| o.address.as_str()).collect();
    assert_eq!(addresses, [super::types::tunnel_target(&theirs)]);
    assert!(!addresses.contains(&super::types::tunnel_target(&ours.tunnel_id).as_str()));
    assert_eq!(state.load_balancers.len(), 1);

    // Adding it back joins the pool instead of taking the DNS record over.
    run(&engine, &cloud, &conns, &add("r1", "app.xyz.com", "3000")).await;
    let state = cloud.snapshot();
    assert_eq!(state.lb_pools.values().next().unwrap().origins.len(), 2);
}
