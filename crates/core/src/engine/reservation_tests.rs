//! Names shared by a team (M12-11): owners in DNS comments, leases, conflicts and
//! take-overs, end to end through the engine against the fake Cloudflare, plus the
//! planner's cases on hand-made snapshots.

use cf_api::DnsRecord;
use serde_json::Map;

use super::{
    executor::{Approval, Context, Engine, EngineError, Outcome},
    fake::{CloudState, FakeCloud, FakeConnectors},
    local::Local,
    ownership::{HoldKind, LEASE_ADDRESS, Ownership, parse_until},
    planner::{PlanError, plan},
    types::{Intent, ObservedRecord, RouteSpec, Snapshot, Step, Warning, ZoneRef},
};
use crate::{
    domain::{Hostname, RouteOrigin},
    store::Store,
};

const CTX: Context<'static> = Context {
    account: "acc",
    machine_name: "Mac",
    tunnel: None,
};

fn engine(owner: &str) -> Engine {
    Engine::new(Local::new(Store::open_in_memory().unwrap())).with_owner(owner)
}

fn cloud() -> FakeCloud {
    FakeCloud::new(CloudState {
        zones: vec![ZoneRef {
            id: "z".into(),
            name: "xyz.com".into(),
        }],
        ..CloudState::default()
    })
}

fn host(name: &str) -> Hostname {
    Hostname::parse(name).unwrap()
}

fn add(hostname: &str) -> Intent {
    Intent::AddRoute {
        route: RouteSpec {
            id: "r1".into(),
            hostname: host(hostname),
            path: None,
            origin: RouteOrigin::parse("3000").unwrap(),
            options: Map::new(),
            access: None,
        },
    }
}

fn reserve(hostname: &str, until: Option<&str>) -> Intent {
    Intent::Reserve {
        hostname: host(hostname),
        until: until.map(|u| parse_until(u).unwrap()),
    }
}

fn release(hostname: &str) -> Intent {
    Intent::Release {
        hostname: host(hostname),
    }
}

fn remove(hostname: &str) -> Intent {
    Intent::RemoveRoute {
        hostname: host(hostname),
        path: None,
    }
}

/// Previews and applies `intent`, confirming only when `confirm` says so.
async fn apply(
    engine: &Engine,
    cloud: &FakeCloud,
    intent: &Intent,
    confirm: bool,
) -> Result<Outcome, EngineError> {
    let plan = engine.preview(cloud, CTX, intent).await?;
    engine
        .apply(
            cloud,
            &FakeConnectors::default(),
            CTX,
            intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: confirm,
            },
            |_| {},
        )
        .await
}

fn records(cloud: &FakeCloud, name: &str) -> Vec<DnsRecord> {
    cloud
        .snapshot()
        .records
        .values()
        .flatten()
        .filter(|r| r.name == name)
        .cloned()
        .collect()
}

fn only(cloud: &FakeCloud, name: &str) -> (DnsRecord, Ownership) {
    let found = records(cloud, name);
    assert_eq!(found.len(), 1, "{found:?}");
    let record = found[0].clone();
    let ownership = Ownership::parse(record.comment.as_deref().unwrap()).unwrap();
    (record, ownership)
}

#[tokio::test]
async fn a_reservation_is_a_placeholder_that_a_route_keeps_and_gives_back() {
    let alice = engine("alice@Alice-MacBook");
    let cloud = cloud();

    apply(
        &alice,
        &cloud,
        &reserve("demo.xyz.com", Some("2099-12-31")),
        false,
    )
    .await
    .unwrap();
    let (record, lease) = only(&cloud, "demo.xyz.com");
    assert_eq!(
        (
            record.kind.as_str(),
            record.content.as_str(),
            record.proxied
        ),
        ("AAAA", LEASE_ADDRESS, true)
    );
    assert_eq!(lease.owner.as_deref(), Some("alice@Alice-MacBook"));
    assert_eq!(lease.until, parse_until("2099-12-31"));

    // Routing her own reserved name needs no confirmation and keeps the lease.
    apply(&alice, &cloud, &add("demo.xyz.com"), false)
        .await
        .unwrap();
    let (record, route) = only(&cloud, "demo.xyz.com");
    assert_eq!(record.kind, "CNAME");
    assert_eq!(route.route_id(), Some("r1"));
    assert!(route.lease);
    assert_eq!(route.until, parse_until("2099-12-31"));

    // Removing the route puts the placeholder back.
    alice.invalidate("acc");
    apply(&alice, &cloud, &remove("demo.xyz.com"), false)
        .await
        .unwrap();
    let (record, lease) = only(&cloud, "demo.xyz.com");
    assert_eq!(record.content, LEASE_ADDRESS);
    assert_eq!(lease.owner.as_deref(), Some("alice@Alice-MacBook"));

    // Releasing frees the name.
    alice.invalidate("acc");
    apply(&alice, &cloud, &release("demo.xyz.com"), false)
        .await
        .unwrap();
    assert!(records(&cloud, "demo.xyz.com").is_empty());
    let err = alice
        .preview(&cloud, CTX, &release("demo.xyz.com"))
        .await
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Plan(PlanError::NotReserved(_))),
        "{err:?}"
    );
}

#[tokio::test]
async fn someone_elses_name_is_refused_unless_taken_over() {
    let (alice, bob, cloud) = (engine("alice@mac"), engine("bob@pc"), cloud());
    apply(
        &alice,
        &cloud,
        &reserve("demo.xyz.com", Some("2099-12-31")),
        false,
    )
    .await
    .unwrap();

    let plan = bob
        .preview(&cloud, CTX, &add("demo.xyz.com"))
        .await
        .unwrap();
    assert!(plan.requires_confirmation);
    assert_eq!(
        plan.warnings,
        [Warning::HeldBy {
            hostname: "demo.xyz.com".into(),
            owner: Some("alice@mac".into()),
            until: parse_until("2099-12-31"),
            kind: HoldKind::Reservation,
        }]
    );
    let refused = apply(&bob, &cloud, &add("demo.xyz.com"), false).await;
    assert!(
        matches!(refused, Err(EngineError::NeedsConfirmation)),
        "{refused:?}"
    );
    assert_eq!(only(&cloud, "demo.xyz.com").0.content, LEASE_ADDRESS);

    // Taken over: Bob's route, not carrying Alice's lease.
    apply(&bob, &cloud, &add("demo.xyz.com"), true)
        .await
        .unwrap();
    let (record, route) = only(&cloud, "demo.xyz.com");
    assert_eq!(record.kind, "CNAME");
    assert_eq!(route.owner.as_deref(), Some("bob@pc"));
    assert!(!route.lease);

    // Now Alice sees Bob's route as his.
    alice.invalidate("acc");
    let plan = alice
        .preview(&cloud, CTX, &reserve("demo.xyz.com", None))
        .await
        .unwrap();
    assert!(plan.requires_confirmation);
    assert!(matches!(
        &plan.warnings[..],
        [Warning::HeldBy { kind: HoldKind::Route, owner: Some(o), .. }] if o == "bob@pc"
    ));
}

#[tokio::test]
async fn an_ended_lease_is_free() {
    let (alice, bob, cloud) = (engine("alice@mac"), engine("bob@pc"), cloud());
    cloud.state.lock().unwrap().records.insert(
        "z".into(),
        vec![DnsRecord {
            id: "old".into(),
            name: "demo.xyz.com".into(),
            kind: "AAAA".into(),
            content: LEASE_ADDRESS.into(),
            proxied: true,
            comment: Some(Ownership::lease("alice@mac", parse_until("2020-01-01")).render()),
            ttl: 1,
        }],
    );
    let plan = bob
        .preview(&cloud, CTX, &add("demo.xyz.com"))
        .await
        .unwrap();
    assert!(!plan.requires_confirmation, "{plan:?}");
    assert!(plan.warnings.is_empty());
    // Reserving over it replaces it without asking, too.
    let plan = alice
        .preview(&cloud, CTX, &reserve("demo.xyz.com", None))
        .await
        .unwrap();
    assert!(!plan.requires_confirmation);
    assert!(matches!(
        &plan.steps[..],
        [
            Step::DeleteRecord { .. },
            Step::CreateReservation { until: None, .. }
        ]
    ));
}

#[tokio::test]
async fn a_record_teitunnel_didnt_write_is_never_reserved() {
    let (alice, cloud) = (engine("alice@mac"), cloud());
    cloud.state.lock().unwrap().records.insert(
        "z".into(),
        vec![DnsRecord {
            id: "www".into(),
            name: "www.xyz.com".into(),
            kind: "A".into(),
            content: "192.0.2.1".into(),
            proxied: false,
            comment: None,
            ttl: 300,
        }],
    );
    let err = alice
        .preview(&cloud, CTX, &reserve("www.xyz.com", None))
        .await
        .unwrap_err();
    assert!(
        matches!(err, EngineError::Plan(PlanError::HostnameInUse(_))),
        "{err:?}"
    );
}

#[tokio::test]
async fn reserving_a_routed_name_and_renewing_change_only_the_comment() {
    let (alice, cloud) = (engine("alice@mac"), cloud());
    apply(&alice, &cloud, &add("demo.xyz.com"), false)
        .await
        .unwrap();
    let before = only(&cloud, "demo.xyz.com").0;
    alice.invalidate("acc");
    apply(
        &alice,
        &cloud,
        &reserve("demo.xyz.com", Some("2099-01-01")),
        false,
    )
    .await
    .unwrap();
    let (after, lease) = only(&cloud, "demo.xyz.com");
    assert_eq!((after.id, after.content), (before.id, before.content));
    assert!(lease.lease);
    assert_eq!(lease.until, parse_until("2099-01-01"));

    // The same again: nothing to do. A new date: the comment only.
    alice.invalidate("acc");
    let same = alice
        .preview(&cloud, CTX, &reserve("demo.xyz.com", Some("2099-01-01")))
        .await
        .unwrap();
    assert!(same.is_empty(), "{same:?}");
    let renew = alice
        .preview(&cloud, CTX, &reserve("demo.xyz.com", None))
        .await
        .unwrap();
    assert!(matches!(
        &renew.steps[..],
        [Step::SetLease {
            lease: true,
            until: None,
            ..
        }]
    ));

    // Releasing keeps the route.
    apply(&alice, &cloud, &release("demo.xyz.com"), false)
        .await
        .unwrap();
    let (record, route) = only(&cloud, "demo.xyz.com");
    assert_eq!(record.kind, "CNAME");
    assert!(!route.lease);
    assert_eq!(route.route_id(), Some("r1"));
}

#[tokio::test]
async fn a_failed_reservation_change_is_undone() {
    let (alice, cloud) = (engine("alice@mac"), cloud());
    apply(&alice, &cloud, &add("demo.xyz.com"), false)
        .await
        .unwrap();
    // Take-over of Bob's placeholder: delete (0) then create (1); fail each in turn.
    let theirs = DnsRecord {
        id: "bob".into(),
        name: "bob.xyz.com".into(),
        kind: "AAAA".into(),
        content: LEASE_ADDRESS.into(),
        proxied: true,
        comment: Some(Ownership::lease("bob@pc", None).render()),
        ttl: 1,
    };
    cloud
        .state
        .lock()
        .unwrap()
        .records
        .get_mut("z")
        .unwrap()
        .push(theirs);
    for (intent, confirm) in [
        (reserve("bob.xyz.com", Some("2099-01-01")), true),
        (reserve("demo.xyz.com", Some("2099-01-01")), false),
        (remove("demo.xyz.com"), false),
    ] {
        let initial = cloud.snapshot();
        let start = cloud.mutations();
        for n in 0..2 {
            alice.invalidate("acc");
            cloud.fail_once(start + n);
            let outcome = apply(&alice, &cloud, &intent, confirm).await.unwrap();
            if matches!(outcome, Outcome::Applied { .. }) {
                // Fewer mutations than `n`: nothing failed, so undo it for the next round.
                *cloud.state.lock().unwrap() = initial.clone();
                continue;
            }
            assert!(
                matches!(outcome, Outcome::RolledBack { .. }),
                "{intent:?}, failing mutation {n}: {outcome:?}"
            );
            assert_eq!(
                cloud.snapshot().normalized(),
                initial.normalized(),
                "{intent:?}, failing mutation {n}: state restored"
            );
        }
        *cloud.state.lock().unwrap() = initial;
    }
}

fn snapshot(records: Vec<ObservedRecord>, held: Vec<super::ownership::Hold>) -> Snapshot {
    Snapshot {
        account_id: "acc".into(),
        machine_name: "Mac".into(),
        zones: vec![ZoneRef {
            id: "z".into(),
            name: "xyz.com".into(),
        }],
        tunnel: None,
        tunnel_names: Vec::new(),
        elsewhere: Vec::new(),
        records,
        access: None,
        networks: None,
        balance: None,
        site: None,
        held,
        owner: "me@Mac".into(),
        now: 1_000,
        edge: Vec::new(),
        service_tokens: None,
        database: None,
        front: Vec::new(),
    }
}

#[test]
fn a_snapshot_asks_before_replacing_someone_elses_placeholder() {
    use super::sites::{SiteAddress, SiteContent, SiteSettings, SiteSpec, SiteState};
    let placeholder = ObservedRecord {
        zone_id: "z".into(),
        owned: true,
        record: DnsRecord {
            id: "p".into(),
            name: "docs.xyz.com".into(),
            kind: "AAAA".into(),
            content: LEASE_ADDRESS.into(),
            proxied: true,
            comment: Some(Ownership::lease("bob@pc", None).render()),
            ttl: 1,
        },
    };
    let hold = super::ownership::Hold {
        hostname: "docs.xyz.com".into(),
        owner: Some("bob@pc".into()),
        until: None,
        kind: HoldKind::Reservation,
    };
    let mut observed = snapshot(vec![placeholder], vec![hold]);
    observed.site = Some(SiteState {
        script: "teitunnel-docs".into(),
        exists: false,
        active_version: None,
        workers_dev: false,
        subdomain: Some("me".into()),
        domains: Vec::new(),
        hostname_taken_by: None,
    });
    let intent = Intent::PublishSnapshot {
        site: SiteSpec {
            id: "s".into(),
            name: "docs".into(),
            script: "teitunnel-docs".into(),
            address: SiteAddress::Domain {
                hostname: host("docs.xyz.com"),
            },
            access: None,
        },
        settings: SiteSettings {
            spa: false,
            password: super::sites::Password::Off,
            overlay: None,
            comments: None,
        },
        content: SiteContent {
            root: None,
            files: Vec::new(),
            headers: None,
            redirects: None,
        },
    };
    let planned = plan(&intent, &observed).unwrap();
    assert!(planned.requires_confirmation);
    assert!(
        planned
            .steps
            .iter()
            .any(|s| matches!(s, Step::DeleteRecord { record, .. } if record.id == "p"))
    );
    // Without the hold (mine, or ended), it simply makes way.
    observed.held.clear();
    let planned = plan(&intent, &observed).unwrap();
    assert!(!planned.requires_confirmation);
}
