//! Executor tests against the fake Cloudflare: end-to-end flows, the staleness guard,
//! confirmation, and rollback with a failure injected at every step.

use cf_api::DnsRecord;
use serde_json::{Map, json};

use super::{
    executor::{Approval, Context, Engine, EngineError, Outcome, StepState},
    fake::{CloudState, FakeCloud, FakeConnectors},
    local::Local,
    types::{Intent, RouteSpec, ZoneRef},
};
use crate::{
    domain::{Hostname, RouteOrigin},
    store::Store,
};

const CTX: Context<'static> = Context {
    account: "acc",
    machine_name: "Mac",
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
        },
    }
}

fn remove(hostname: &str) -> Intent {
    Intent::RemoveRoute {
        hostname: Hostname::parse(hostname).unwrap(),
        path: None,
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
    assert_eq!(
        log[0].summary,
        "Remove every route and delete this Mac's tunnel"
    );
    assert_eq!(log[2].summary, "Add xyz.com → http://localhost:3000");
    assert!(log.iter().all(|e| e.outcome == "applied"));
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
                },
            },
        ),
        ("remove route", two_routes.clone(), remove("yx.com")),
        ("remove tunnel", two_routes, Intent::RemoveTunnel),
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
    assert!(leftovers[0].starts_with("Tunnel "), "{leftovers:?}");

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
