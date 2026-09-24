//! Workers in front of a route (offline page, webhook inbox) and Snapshot comments'
//! database: planner scenarios (insta snapshots of the preview), foreign routes left
//! alone, idempotency, and execution against the fake Cloudflare. The
//! failure-at-every-step scenarios are in `executor_tests` (via [`front_scenarios`]).

use serde_json::Value;

use super::{
    executor::{Approval, Context, Engine, Outcome},
    fake::{CloudState, FakeCloud, FakeConnectors},
    front::{
        DatabaseState, FrontConfig, FrontKind, FrontState, InboxSettings, InboxVerify,
        ObservedFront, OfflinePage, script_for,
    },
    local::Local,
    planner::{PlanError, plan},
    simulate::apply,
    sites::{SiteComments, SiteSettings},
    types::{Intent, ObservedRecord, Plan, RouteSpec, Snapshot, Step, Warning, ZoneRef},
};
use crate::{
    Secret,
    domain::{Hostname, RouteOrigin},
    store::Store,
};

const CTX: Context<'static> = Context {
    account: "acc",
    machine_name: "Mac",
    tunnel: None,
};

fn host(h: &str) -> Hostname {
    Hostname::parse(h).unwrap()
}

fn page(title: &str) -> OfflinePage {
    OfflinePage {
        title: title.into(),
        message: "I'm away; back tomorrow.".into(),
        when_app_down: false,
    }
}

pub(super) fn offline(hostname: &str, title: Option<&str>) -> Intent {
    Intent::SetOfflinePage {
        hostname: host(hostname),
        page: title.map(page),
    }
}

pub(super) fn inbox(hostname: &str, path: &str, on: bool) -> Intent {
    Intent::SetInbox {
        hostname: host(hostname),
        path: path.into(),
        inbox: on.then(InboxSettings::default),
        secret: None,
    }
}

fn proxied(name: &str) -> ObservedRecord {
    ObservedRecord {
        zone_id: "z-xyz".into(),
        record: cf_api::DnsRecord {
            id: format!("rec-{name}"),
            name: name.into(),
            kind: "CNAME".into(),
            content: "t1.cfargotunnel.com".into(),
            proxied: true,
            comment: Some("teitunnel:route=r1".into()),
            ttl: 1,
        },
        owned: true,
    }
}

fn state(fronts: Vec<ObservedFront>, foreign: Vec<cf_api::WorkerRoute>) -> FrontState {
    FrontState {
        hostname: "app.xyz.com".into(),
        zone_id: "z-xyz".into(),
        fronts,
        foreign,
    }
}

fn observed(front: Option<FrontState>, database: Option<Option<&str>>) -> Snapshot {
    Snapshot {
        account_id: "acc".into(),
        machine_name: "Mac".into(),
        zones: vec![ZoneRef {
            id: "z-xyz".into(),
            name: "xyz.com".into(),
        }],
        tunnel: None,
        tunnel_names: Vec::new(),
        elsewhere: Vec::new(),
        records: vec![proxied("app.xyz.com")],
        access: None,
        networks: None,
        balance: None,
        site: None,
        held: Vec::new(),
        owner: "me@Mac".into(),
        now: 0,
        edge: None,
        service_tokens: None,
        database: database.map(|id| DatabaseState {
            id: id.map(str::to_owned),
        }),
        front,
    }
}

fn ours(config: FrontConfig, routed: bool) -> ObservedFront {
    let script = script_for(config.kind(), "app.xyz.com", config.path());
    ObservedFront {
        route: routed.then(|| cf_api::WorkerRoute {
            id: "wr1".into(),
            pattern: super::front::pattern_for("app.xyz.com", &config),
            script: Some(script.clone()),
            request_limit_fail_open: Some(true),
        }),
        config,
        script,
        exists: true,
    }
}

fn view(plan: &Plan) -> (Vec<String>, Vec<Warning>) {
    (
        plan.steps
            .iter()
            .map(|s| s.describe(&plan.tunnel_name).english())
            .collect(),
        plan.warnings.clone(),
    )
}

fn assert_idempotent(intent: &Intent, before: &Snapshot) -> Plan {
    let first = plan(intent, before).unwrap();
    let after = apply(before, &first);
    let again = plan(intent, &after).unwrap();
    assert!(again.steps.is_empty(), "{:?}", again.steps);
    first
}

#[test]
fn offline_page_on_a_route() {
    let before = observed(Some(state(Vec::new(), Vec::new())), None);
    let first = assert_idempotent(&offline("app.xyz.com", Some("Back soon")), &before);
    insta::assert_yaml_snapshot!(view(&first));
    // Worker before its route, so the route never points at nothing.
    assert!(matches!(first.steps[0], Step::PutFrontWorker { .. }));
    assert!(matches!(
        &first.steps[1],
        Step::CreateWorkerRoute { pattern, .. } if pattern == "app.xyz.com/*"
    ));
    assert!(!first.requires_confirmation);
}

#[test]
fn changing_and_removing_the_offline_page() {
    let before = observed(
        Some(state(
            vec![ours(FrontConfig::Offline { page: page("Old") }, true)],
            Vec::new(),
        )),
        None,
    );
    let change = plan(&offline("app.xyz.com", Some("New")), &before).unwrap();
    assert!(matches!(
        change.steps.as_slice(),
        [Step::PutFrontWorker {
            previous: Some(_),
            ..
        }]
    ));
    assert!(change.warnings.is_empty());
    let off = plan(&offline("app.xyz.com", None), &before).unwrap();
    let after = apply(&before, &off);
    assert!(after.front.as_ref().unwrap().fronts.is_empty());
    insta::assert_yaml_snapshot!(view(&off));
    assert!(matches!(off.steps[0], Step::DeleteWorkerRoute { .. }));
    assert!(matches!(off.steps[1], Step::DeleteFrontWorker { .. }));
    // Nothing to remove.
    let empty = observed(Some(state(Vec::new(), Vec::new())), None);
    assert_eq!(
        plan(&offline("app.xyz.com", None), &empty).unwrap_err(),
        PlanError::NoFront("app.xyz.com".into())
    );
}

#[test]
fn someone_elses_worker_route_is_never_taken() {
    let foreign = cf_api::WorkerRoute {
        id: "theirs".into(),
        pattern: "app.xyz.com/*".into(),
        script: Some("their-worker".into()),
        request_limit_fail_open: None,
    };
    let before = observed(Some(state(Vec::new(), vec![foreign])), None);
    assert_eq!(
        plan(&offline("app.xyz.com", Some("Hi")), &before).unwrap_err(),
        PlanError::WorkerRouteTaken {
            pattern: "app.xyz.com/*".into(),
            worker: "their-worker".into()
        }
    );
    // A different pattern is fine: an inbox on a path next to their Worker.
    let with_theirs = plan(&inbox("app.xyz.com", "/hooks/", true), &{
        let mut s = before.clone();
        s.database = Some(DatabaseState {
            id: Some("db1".into()),
        });
        s
    })
    .unwrap();
    assert!(
        with_theirs
            .steps
            .iter()
            .all(|s| !matches!(s, Step::DeleteWorkerRoute { .. }))
    );
}

#[test]
fn a_front_worker_needs_a_proxied_hostname() {
    let mut before = observed(Some(state(Vec::new(), Vec::new())), None);
    before.records.clear();
    assert_eq!(
        plan(&offline("app.xyz.com", Some("Hi")), &before).unwrap_err(),
        PlanError::FrontNeedsRoute("app.xyz.com".into())
    );
    assert!(matches!(
        plan(&offline("app.nowhere.org", Some("Hi")), &before).unwrap_err(),
        PlanError::NoZone(_)
    ));
    let bad = Intent::SetOfflinePage {
        hostname: host("app.xyz.com"),
        page: Some(OfflinePage {
            title: " ".into(),
            ..page("x")
        }),
    };
    assert!(matches!(
        plan(&bad, &observed(Some(state(Vec::new(), Vec::new())), None)).unwrap_err(),
        PlanError::Front(_)
    ));
}

#[test]
fn an_inbox_creates_the_database_first() {
    let before = observed(Some(state(Vec::new(), Vec::new())), Some(None));
    let first = assert_idempotent(&inbox("app.xyz.com", "/webhooks/", true), &before);
    insta::assert_yaml_snapshot!(view(&first));
    assert!(matches!(first.steps[0], Step::CreateDatabase { .. }));
    assert!(matches!(
        &first.steps[1],
        Step::PutFrontWorker {
            database: Some(super::front::DatabaseRef::Created),
            ..
        }
    ));
    let existing = observed(Some(state(Vec::new(), Vec::new())), Some(Some("db1")));
    let plan2 = plan(&inbox("app.xyz.com", "/webhooks/", true), &existing).unwrap();
    assert!(matches!(
        &plan2.steps[0],
        Step::PutFrontWorker { database: Some(super::front::DatabaseRef::Existing(id)), .. } if id == "db1"
    ));
}

#[test]
fn a_verifying_inbox_needs_its_secret_once() {
    let before = observed(Some(state(Vec::new(), Vec::new())), Some(Some("db1")));
    let verify = |secret: Option<&str>| Intent::SetInbox {
        hostname: host("app.xyz.com"),
        path: "/hooks/".into(),
        inbox: Some(InboxSettings {
            verify: Some(InboxVerify::Github),
            ..InboxSettings::default()
        }),
        secret: secret.map(|s| Secret::new(s.to_owned())),
    };
    assert_eq!(
        plan(&verify(None), &before).unwrap_err(),
        PlanError::InboxNeedsSecret
    );
    let with_secret = plan(&verify(Some("s3cret")), &before).unwrap();
    // The secret never shows in the plan as reviewed or recorded.
    assert!(
        !serde_json::to_string(&with_secret.steps)
            .unwrap()
            .contains("s3cret")
    );
    let applied = apply(&before, &with_secret);
    // Once the Worker has it, changing other settings keeps it.
    assert!(plan(&verify(None), &applied).unwrap().steps.is_empty());
}

#[test]
fn removing_the_last_route_takes_its_workers_along() {
    use super::types::ObservedTunnel;
    let mut before = observed(
        Some(state(
            vec![
                ours(FrontConfig::Offline { page: page("Hi") }, true),
                ours(
                    FrontConfig::Inbox {
                        path: "/hooks/".into(),
                        settings: InboxSettings::default(),
                    },
                    false,
                ),
            ],
            Vec::new(),
        )),
        None,
    );
    before.tunnel = Some(ObservedTunnel {
        id: "t1".into(),
        name: "Mac".into(),
        config_version: 1,
        ingress: vec![
            cf_api::IngressRule {
                hostname: Some("app.xyz.com".into()),
                path: None,
                service: "http://localhost:3000".into(),
                origin_request: serde_json::Map::new(),
                extra: serde_json::Map::new(),
            },
            cf_api::IngressRule {
                hostname: None,
                path: None,
                service: "http_status:404".into(),
                origin_request: serde_json::Map::new(),
                extra: serde_json::Map::new(),
            },
        ],
    });
    let removed = plan(
        &Intent::RemoveRoute {
            hostname: host("app.xyz.com"),
            path: None,
        },
        &before,
    )
    .unwrap();
    let kinds: Vec<&str> = removed
        .steps
        .iter()
        .map(|s| match s {
            Step::PutConfig { .. } => "config",
            Step::DeleteRecord { .. } => "dns-",
            Step::DeleteWorkerRoute { .. } => "route-",
            Step::DeleteFrontWorker { .. } => "worker-",
            _ => "other",
        })
        .collect();
    assert_eq!(kinds, ["config", "dns-", "route-", "worker-", "worker-"]);
}

#[test]
fn snapshot_comments_bind_the_database() {
    use super::snapshot_tests::{content, site};
    let settings = SiteSettings::default().with_comments(SiteComments {
        database: super::front::DatabaseRef::Created,
        identity: false,
    });
    assert!(settings.worker_first());
    assert_eq!(
        settings.overlay.as_deref(),
        Some(crate::comments::OVERLAY_PATH)
    );
    let metadata = super::sites::metadata(
        &settings,
        &content(&[("index.html", "hi")]),
        "jwt",
        "m",
        "teitunnel-demo",
        Some("db1"),
    );
    let bindings = metadata["bindings"].as_array().unwrap();
    let named = |name: &str| bindings.iter().find(|b| b["name"] == name).cloned();
    assert_eq!(named("DB").unwrap()["id"], "db1");
    assert_eq!(named("COMMENTS_SITE").unwrap()["text"], "teitunnel-demo");
    assert!(named("ACCESS_IDENTITY").is_none());
    assert_eq!(
        named("OVERLAY_SRC").unwrap()["text"],
        "/__teitunnel/comments/overlay.js"
    );
    // The overlay's source rides along with the Worker's code.
    let module = &super::sites::modules()[0].content;
    assert!(module.contains("const OVERLAY_JS = "));
    let _ = site;
}

// ---------- execution against the fake Cloudflare ----------

pub(super) fn engine() -> Engine {
    Engine::new(Local::new(Store::open_in_memory().unwrap())).with_owner("me@Mac")
}

pub(super) fn route(id: &str, hostname: &str) -> Intent {
    Intent::AddRoute {
        route: RouteSpec {
            id: id.into(),
            hostname: host(hostname),
            path: None,
            origin: RouteOrigin::parse("3000").unwrap(),
            options: serde_json::Map::new(),
            access: None,
        },
    }
}

pub(super) async fn run(engine: &Engine, cloud: &FakeCloud, intent: &Intent) -> Outcome {
    let plan = engine.preview(cloud, CTX, intent).await.unwrap();
    engine
        .apply(
            cloud,
            &FakeConnectors::default(),
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

pub(super) fn zones() -> CloudState {
    CloudState {
        zones: vec![ZoneRef {
            id: "z-xyz".into(),
            name: "xyz.com".into(),
        }],
        workers_subdomain: Some("acme".into()),
        access_org: true,
        ..CloudState::default()
    }
}

fn binding(cloud: &FakeCloud, script: &str, name: &str) -> Option<Value> {
    let state = cloud.snapshot();
    let worker = state.workers.get(script)?;
    worker.live()?.metadata["bindings"]
        .as_array()?
        .iter()
        .find(|b| b["name"] == name)
        .cloned()
}

#[tokio::test]
async fn an_offline_page_goes_up_and_comes_down() {
    let (engine, cloud) = (engine(), FakeCloud::new(zones()));
    assert!(matches!(
        run(&engine, &cloud, &route("r1", "app.xyz.com")).await,
        Outcome::Applied { .. }
    ));
    let outcome = run(&engine, &cloud, &offline("app.xyz.com", Some("Back soon"))).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let script = script_for(FrontKind::Offline, "app.xyz.com", "");
    let page: Value = serde_json::from_str(
        binding(&cloud, &script, "PAGE").unwrap()["text"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(page["title"], "Back soon");
    let state = cloud.snapshot();
    let (zone, route) = state.worker_routes.values().next().unwrap();
    assert_eq!(zone, "z-xyz");
    assert_eq!(route.pattern, "app.xyz.com/*");
    assert_eq!(route.request_limit_fail_open, Some(true));
    let index = engine.local().fronts(Some("acc"), None).await.unwrap();
    assert_eq!(index.len(), 1);
    assert_eq!(index[0].1.route_id.as_deref(), Some(route.id.as_str()));

    // A changed page replaces the Worker; the route stays.
    run(
        &engine,
        &cloud,
        &offline("app.xyz.com", Some("Gone fishing")),
    )
    .await;
    assert!(
        binding(&cloud, &script, "PAGE").unwrap()["text"]
            .as_str()
            .unwrap()
            .contains("Gone fishing")
    );
    assert_eq!(cloud.snapshot().worker_routes.len(), 1);

    run(&engine, &cloud, &offline("app.xyz.com", None)).await;
    let state = cloud.snapshot();
    assert!(state.worker_routes.is_empty());
    assert!(!state.workers.contains_key(&script));
    assert!(
        engine
            .local()
            .fronts(Some("acc"), None)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn an_inbox_brings_its_database_with_tables() {
    let (engine, cloud) = (engine(), FakeCloud::new(zones()));
    run(&engine, &cloud, &route("r1", "app.xyz.com")).await;
    let outcome = run(&engine, &cloud, &inbox("app.xyz.com", "/webhooks/", true)).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    let (db_id, db) = state.d1.iter().next().expect("a database");
    assert_eq!(db.name, crate::comments::remote::DATABASE_NAME);
    let tables = db
        .run(&[cf_api::D1Statement::new(
            "SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name",
            vec![],
        )])
        .unwrap();
    let names: Vec<&str> = tables[0]
        .results
        .iter()
        .filter_map(|r| r["name"].as_str())
        .collect();
    assert!(names.contains(&"teitunnel_comments") && names.contains(&"teitunnel_inbox"));
    let script = script_for(FrontKind::Inbox, "app.xyz.com", "/webhooks/");
    assert_eq!(
        binding(&cloud, &script, "DB").unwrap()["id"],
        db_id.as_str()
    );
    assert_eq!(
        engine
            .local()
            .cloud_database("acc")
            .await
            .unwrap()
            .as_deref(),
        Some(db_id.as_str())
    );
    // A second inbox reuses the database.
    run(&engine, &cloud, &inbox("app.xyz.com", "/stripe/", true)).await;
    assert_eq!(cloud.snapshot().d1.len(), 1);
    // Removing the route removes both inboxes (the database stays).
    run(
        &engine,
        &cloud,
        &Intent::RemoveRoute {
            hostname: host("app.xyz.com"),
            path: None,
        },
    )
    .await;
    let state = cloud.snapshot();
    assert!(state.worker_routes.is_empty());
    assert!(state.workers.keys().all(|w| !w.starts_with("tt-")));
    assert_eq!(state.d1.len(), 1);
}

#[tokio::test]
async fn a_snapshot_with_comments_gets_the_database() {
    use super::snapshot_tests::{content, site};
    let (engine, cloud) = (engine(), FakeCloud::new(zones()));
    let intent = Intent::PublishSnapshot {
        site: site("demo", None),
        settings: SiteSettings::default().with_comments(SiteComments {
            database: super::front::DatabaseRef::Created,
            identity: false,
        }),
        content: content(&[("index.html", "<p>hi</p>")]),
    };
    let outcome = run(&engine, &cloud, &intent).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    let db_id = state.d1.keys().next().expect("a database").clone();
    assert_eq!(
        binding(&cloud, "teitunnel-demo", "DB").unwrap()["id"],
        db_id.as_str()
    );
    assert_eq!(
        binding(&cloud, "teitunnel-demo", "COMMENTS_SITE").unwrap()["text"],
        "teitunnel-demo"
    );
    let live = state.workers["teitunnel-demo"].live().unwrap().clone();
    assert_eq!(live.metadata["assets"]["config"]["run_worker_first"], true);
}

/// Failure-at-every-step scenarios for `a_failure_at_any_step_rolls_everything_back`.
pub(super) async fn front_scenarios() -> Vec<(&'static str, CloudState, Intent)> {
    let (engine, cloud) = (engine(), FakeCloud::new(zones()));
    run(&engine, &cloud, &route("r1", "app.xyz.com")).await;
    let routed = cloud.snapshot();
    run(&engine, &cloud, &offline("app.xyz.com", Some("Back soon"))).await;
    let with_page = cloud.snapshot();
    run(&engine, &cloud, &inbox("app.xyz.com", "/hooks/", true)).await;
    let with_both = cloud.snapshot();
    vec![
        (
            "offline page",
            routed.clone(),
            offline("app.xyz.com", Some("Back soon")),
        ),
        (
            "remove the offline page",
            with_page,
            offline("app.xyz.com", None),
        ),
        (
            "inbox with a new database",
            routed,
            inbox("app.xyz.com", "/hooks/", true),
        ),
        (
            "remove a route with an offline page and an inbox",
            with_both,
            Intent::RemoveRoute {
                hostname: host("app.xyz.com"),
                path: None,
            },
        ),
        (
            "snapshot with comments",
            zones(),
            Intent::PublishSnapshot {
                site: super::snapshot_tests::site("demo", None),
                settings: SiteSettings::default().with_comments(SiteComments {
                    database: super::front::DatabaseRef::Created,
                    identity: false,
                }),
                content: super::snapshot_tests::content(&[("index.html", "hi")]),
            },
        ),
    ]
}

/// Rebuilds the local index of front Workers and the database from the fake's state
/// (what another machine, or a restored backup, would have).
pub(super) async fn adopt_fronts(engine: &Engine, state: &CloudState) {
    if let Some((id, db)) = state.d1.iter().next() {
        engine
            .local()
            .set_cloud_database("acc", Some((id, &db.name)))
            .await
            .unwrap();
    }
    for (zone, route) in state.worker_routes.values() {
        let Some(script) = route.script.as_deref().filter(|s| s.starts_with("tt-")) else {
            continue;
        };
        let Some(live) = state.workers.get(script).and_then(|w| w.live()) else {
            continue;
        };
        let bindings = live.metadata["bindings"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let text = |name: &str| {
            bindings
                .iter()
                .find(|b| b["name"] == name)
                .and_then(|b| b["text"].as_str())
                .and_then(|t| serde_json::from_str::<Value>(t).ok())
        };
        let config = if let Some(page) = text("PAGE") {
            FrontConfig::Offline {
                page: OfflinePage {
                    title: page["title"].as_str().unwrap_or_default().into(),
                    message: page["message"].as_str().unwrap_or_default().into(),
                    when_app_down: page["whenAppDown"].as_bool().unwrap_or_default(),
                },
            }
        } else if let Some(inbox) = text("INBOX") {
            FrontConfig::Inbox {
                path: inbox["path"].as_str().unwrap_or_default().into(),
                settings: InboxSettings {
                    max_items: u32::try_from(inbox["maxItems"].as_u64().unwrap_or(500))
                        .unwrap_or(500),
                    retention_days: u32::try_from(inbox["retentionDays"].as_u64().unwrap_or(7))
                        .unwrap_or(7),
                    verify: None,
                },
            }
        } else {
            continue;
        };
        let hostname = route.pattern.split('/').next().unwrap_or_default();
        engine
            .local()
            .save_front("acc", hostname, zone, script, &config)
            .await
            .unwrap();
        engine
            .local()
            .set_front_route(
                "acc",
                hostname,
                config.kind(),
                config.path(),
                Some(&route.id),
            )
            .await
            .unwrap();
    }
}
