//! Pausing: who serves the paused page, and pausing and resuming through a real Lens
//! and the engine (against the fake Cloudflare).

use super::*;
use crate::{
    domain_shares::DomainShare,
    engine::{
        Change, Local, RouteInput,
        fake::{CloudState, FakeCloud, FakeConnectors},
    },
    inspect::{
        TapSpec,
        tests::{origin, send},
    },
};

const ACCOUNT: &str = "acc";

fn ctx() -> Context<'static> {
    Context {
        account: ACCOUNT,
        machine_name: "Mac",
        tunnel: None,
    }
}

async fn share(store: &Store, hostname: &str, owner: &str) {
    Local::new(store.clone())
        .record_share(&DomainShare {
            account_id: ACCOUNT.into(),
            hostname: hostname.into(),
            origin: "http://localhost:3000".into(),
            owner: owner.into(),
            expires_at: None,
            created_at: 1,
            source: None,
            folder: false,
            paused: false,
            schedule: None,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn the_process_serving_the_route_is_asked() {
    let store = Store::open_in_memory().unwrap();
    // A share of the app's is the app's to pause.
    share(&store, "demo.xyz.com", APP_OWNER).await;
    let paused = request(&store, ACCOUNT, "Demo.xyz.com", false)
        .await
        .unwrap();
    assert_eq!(paused.owner, APP_OWNER);
    assert_eq!(paused.hostname, "demo.xyz.com");

    // A terminal that's gone can't.
    share(&store, "gone.xyz.com", "1-1").await;
    assert!(matches!(
        request(&store, ACCOUNT, "gone.xyz.com", false).await,
        Err(PauseError::OwnerGone(_))
    ));
    // A running terminal without a tap for it can't either.
    let me = runtime::this_process();
    share(&store, "plain.xyz.com", &me).await;
    assert!(matches!(
        request(&store, ACCOUNT, "plain.xyz.com", false).await,
        Err(PauseError::NotInspected(_))
    ));
    let scope = serde_json::to_string(&TapScope::route(ACCOUNT, "plain.xyz.com", None)).unwrap();
    let owner = me.clone();
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO lens_taps (id, scope, name, origin, owner, started_at)
                 VALUES ('t1', ?1, 'plain', 'http://localhost:3000', ?2, 1)",
                params![scope, owner],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        request(&store, ACCOUNT, "plain.xyz.com", false)
            .await
            .unwrap()
            .owner,
        me
    );

    // A route needs the app, `up` or `serve`.
    assert!(matches!(
        request(&store, ACCOUNT, "app.xyz.com", false).await,
        Err(PauseError::NoHost(_))
    ));
    assert!(claim_host(&store, APP_OWNER, true).await.unwrap());
    assert!(
        !claim_host(&store, &me, false).await.unwrap(),
        "the app holds it"
    );
    let by_schedule = request(&store, ACCOUNT, "app.xyz.com", true).await.unwrap();
    assert!(by_schedule.by_schedule);
    // Pausing by hand takes it over from the schedule.
    assert!(
        !request(&store, ACCOUNT, "app.xyz.com", false)
            .await
            .unwrap()
            .by_schedule
    );
    assert_eq!(list(&store, Some(ACCOUNT)).await.unwrap().len(), 3);
    assert!(
        request_resume(&store, ACCOUNT, "app.xyz.com")
            .await
            .unwrap()
    );
    assert!(
        !request_resume(&store, ACCOUNT, "app.xyz.com")
            .await
            .unwrap()
    );
    release_host(&store, APP_OWNER).await.unwrap();
    assert_eq!(host(&store).await.unwrap(), None);
}

async fn routed(origin: &str) -> (Engine, FakeCloud, FakeConnectors) {
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let cloud = FakeCloud::new(CloudState {
        zones: vec![crate::engine::ZoneRef {
            id: "z".into(),
            name: "xyz.com".into(),
        }],
        ..CloudState::default()
    });
    let conns = FakeConnectors::default();
    let change = Change::AddRoute {
        route: RouteInput {
            hostname: "app.xyz.com".into(),
            path: None,
            origin: origin.into(),
            access: None,
            options: None,
        },
    };
    let intent = engine.intent_for(&cloud, ctx(), &change).await.unwrap();
    let plan = engine.preview(&cloud, ctx(), &intent).await.unwrap();
    engine
        .apply(
            &cloud,
            &conns,
            ctx(),
            &intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .unwrap();
    (engine, cloud, conns)
}

fn service(cloud: &FakeCloud) -> String {
    let state = cloud.snapshot();
    state
        .tunnels
        .values()
        .next()
        .unwrap()
        .config
        .as_ref()
        .unwrap()
        .ingress[0]
        .service
        .clone()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_uninspected_route_goes_through_a_tap_while_paused() {
    let origin = origin().await;
    let (engine, cloud, conns) = routed(&origin).await;
    let store = engine.local().store().clone();
    let inspector = Inspector::new(Some(store.clone()), None, APP_OWNER);
    let enforcer = Enforcer::new();
    claim_host(&store, APP_OWNER, true).await.unwrap();

    request(&store, ACCOUNT, "app.xyz.com", false)
        .await
        .unwrap();
    let needs = enforcer.sync_taps(&store, &inspector).await.unwrap();
    let [Needs::Inspect(row)] = needs.as_slice() else {
        panic!("{needs:?}");
    };
    pause_here(&engine, &cloud, &conns, ctx(), &inspector, row)
        .await
        .unwrap();
    enforcer.applied_via_inspect(row);
    let tap = inspector.taps().pop().unwrap();
    assert_eq!(service(&cloud), tap.address, "the route points at the tap");
    assert!(tap.paused.is_some());
    assert_eq!(send(&tap.address, "GET", "/", &[]).await.0, 503);
    assert!(
        find(&store, ACCOUNT, "app.xyz.com")
            .await
            .unwrap()
            .unwrap()
            .via_inspect
    );
    // Nothing more to do while it stays paused.
    assert!(
        enforcer
            .sync_taps(&store, &inspector)
            .await
            .unwrap()
            .is_empty()
    );

    request_resume(&store, ACCOUNT, "app.xyz.com")
        .await
        .unwrap();
    let needs = enforcer.sync_taps(&store, &inspector).await.unwrap();
    assert_eq!(
        needs,
        [Needs::Revert {
            account_id: ACCOUNT.into(),
            hostname: "app.xyz.com".into()
        }]
    );
    resume_here(
        &engine,
        Some(&cloud),
        &conns,
        ctx(),
        &inspector,
        "app.xyz.com",
        true,
    )
    .await
    .unwrap();
    assert_eq!(service(&cloud), origin, "pointed back at the service");
    assert!(inspector.taps().is_empty());
    inspector.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_inspected_share_only_toggles_its_tap() {
    let origin = origin().await;
    let store = Store::open_in_memory().unwrap();
    let me = runtime::this_process();
    let inspector = Inspector::new(Some(store.clone()), None, &me);
    let mut spec = TapSpec::new(
        TapScope::route(ACCOUNT, "demo.xyz.com", None),
        "demo.xyz.com",
        &origin,
    );
    spec.public_url = Some("https://demo.xyz.com".into());
    let tap = inspector.start(spec).await.unwrap();
    share(&store, "demo.xyz.com", &me).await;
    // The taps table is written in the background.
    for _ in 0..100 {
        if runs_tap(&store, &me, &TapScope::route(ACCOUNT, "demo.xyz.com", None))
            .await
            .unwrap()
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let enforcer = Enforcer::new();

    request(&store, ACCOUNT, "demo.xyz.com", false)
        .await
        .unwrap();
    assert!(
        enforcer
            .sync_taps(&store, &inspector)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(send(&tap.address, "GET", "/", &[]).await.0, 503);

    request_resume(&store, ACCOUNT, "demo.xyz.com")
        .await
        .unwrap();
    assert!(
        enforcer
            .sync_taps(&store, &inspector)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(send(&tap.address, "GET", "/", &[]).await.0, 200);
    inspector.shutdown().await;
}

#[tokio::test]
async fn removing_a_hostnames_last_route_forgets_its_pause_and_schedule() {
    let (engine, cloud, conns) = routed("http://localhost:3000").await;
    let store = engine.local().store().clone();
    claim_host(&store, APP_OWNER, true).await.unwrap();
    request(&store, ACCOUNT, "app.xyz.com", false)
        .await
        .unwrap();
    let schedule = crate::schedule::Schedule::parse("mon-fri 09:00-18:00", Some("UTC")).unwrap();
    crate::schedule::set(&store, ACCOUNT, "app.xyz.com", Some(&schedule))
        .await
        .unwrap();

    let change = Change::RemoveRoute {
        hostname: "app.xyz.com".into(),
        path: None,
    };
    let intent = engine.intent_for(&cloud, ctx(), &change).await.unwrap();
    let plan = engine.preview(&cloud, ctx(), &intent).await.unwrap();
    let approval = Approval {
        fingerprint: &plan.fingerprint,
        confirmed: false,
    };
    engine
        .apply(&cloud, &conns, ctx(), &intent, approval, |_| {})
        .await
        .unwrap();
    assert!(
        find(&store, ACCOUNT, "app.xyz.com")
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        crate::schedule::list(&store, Some(ACCOUNT))
            .await
            .unwrap()
            .is_empty()
    );
}
