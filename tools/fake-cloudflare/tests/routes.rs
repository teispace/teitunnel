//! The routes engine end to end over HTTP, against the fake Cloudflare API: two routes
//! in two zones, verified, then everything removed with nothing left behind.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{net::SocketAddr, process::Stdio, time::Duration};

use cf_api::{ApiToken, Client};
use teitunnel_core::{
    Secret,
    domain::Hostname,
    engine::{
        AccessRule, Approval, Change, Connectors, Context, Edge, Engine, Local, Outcome, RouteInput,
    },
    runtime::ConnectorState,
    store::Store,
};
use tokio::io::{AsyncBufReadExt, BufReader};

const CTX: Context<'static> = Context {
    account: "e2e-account",
    machine_name: "E2E Mac",
    tunnel: None,
};

/// Pretends to run connectors; records the token it was given.
#[derive(Debug, Default)]
struct Recorder(std::sync::Mutex<Vec<String>>);

impl Connectors for Recorder {
    fn state(&self, tunnel_id: &str) -> Option<ConnectorState> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .any(|c| c == &format!("start {tunnel_id}"))
            .then_some(ConnectorState::Healthy { connections: 4 })
    }
    async fn start(
        &self,
        _: &str,
        tunnel_id: &str,
        token: Secret<String>,
    ) -> Result<(), teitunnel_core::text::Text> {
        assert!(token.expose().starts_with("e2e-run-token-"));
        self.0.lock().unwrap().push(format!("start {tunnel_id}"));
        Ok(())
    }
    async fn stop(&self, tunnel_id: &str) -> Result<(), teitunnel_core::text::Text> {
        self.0
            .lock()
            .unwrap()
            .retain(|c| c != &format!("start {tunnel_id}"));
        Ok(())
    }
    async fn deleted(&self, _: &str) {}
}

async fn spawn_fake() -> (tokio::process::Child, SocketAddr) {
    let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_fake-cloudflare"))
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let line = tokio::time::timeout(Duration::from_secs(10), lines.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let addr = line.trim_start_matches("listening on ").parse().unwrap();
    (child, addr)
}

async fn apply(engine: &Engine, api: &Client, conns: &Recorder, change: Change) -> Outcome {
    apply_on(engine, api, conns, None, change).await
}

async fn apply_on(
    engine: &Engine,
    api: &Client,
    conns: &Recorder,
    tunnel: Option<&str>,
    change: Change,
) -> Outcome {
    let ctx = Context { tunnel, ..CTX };
    let intent = engine.intent_for(api, ctx, &change).await.unwrap();
    let plan = engine.preview(api, ctx, &intent).await.unwrap();
    let approval = Approval {
        fingerprint: &plan.fingerprint,
        confirmed: false,
    };
    engine
        .apply(api, conns, ctx, &intent, approval, |_| {})
        .await
        .unwrap()
}

fn add(hostname: &str, origin: &str) -> Change {
    Change::AddRoute {
        route: RouteInput {
            hostname: hostname.into(),
            path: None,
            origin: origin.into(),
            access: None,
            options: None,
        },
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn two_domains_verified_then_nothing_left() {
    let (_fake, addr) = spawn_fake().await;
    let api = Client::with_base(&format!("http://{addr}"), ApiToken::new("e2e")).unwrap();
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let conns = Recorder::default();

    for (host, origin) in [("xyz.com", "3000"), ("app.yx.com", "5000")] {
        let outcome = apply(&engine, &api, &conns, add(host, origin)).await;
        assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
        let checked = engine
            .verify(
                &api,
                CTX,
                &Hostname::parse(host).unwrap(),
                Edge::Test(addr),
                Duration::ZERO,
            )
            .await
            .unwrap();
        assert!(checked.ok(), "{checked:?}");
    }

    let tunnels = api.tunnels("e2e-account").await.unwrap();
    assert_eq!(tunnels.len(), 1);
    assert_eq!(tunnels[0].name, "E2E Mac");
    let config = api
        .tunnel_config("e2e-account", &tunnels[0].id)
        .await
        .unwrap();
    assert_eq!(
        config.config.unwrap().ingress.len(),
        3,
        "two routes and the catch-all"
    );
    let overview = engine.overview(&api, &conns, CTX).await.unwrap();
    assert_eq!(overview.routes.len(), 2);

    let outcome = apply(&engine, &api, &conns, Change::RemoveTunnel).await;
    assert!(
        matches!(
            outcome,
            Outcome::Applied {
                tunnel_id: None,
                ..
            }
        ),
        "{outcome:?}"
    );
    assert!(
        api.tunnels("e2e-account").await.unwrap().is_empty(),
        "no tunnel left"
    );
    for zone in ["z-xyz", "z-yx"] {
        let left = api
            .dns_records_with_comment(zone, "teitunnel")
            .await
            .unwrap();
        assert!(left.is_empty(), "records left in {zone}: {left:?}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_protected_route_asks_for_a_login_until_its_removed() {
    let (_fake, addr) = spawn_fake().await;
    let api = Client::with_base(&format!("http://{addr}"), ApiToken::new("e2e")).unwrap();
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let conns = Recorder::default();
    let host = Hostname::parse("app.xyz.com").unwrap();

    let mut change = add("app.xyz.com", "3000");
    if let Change::AddRoute { route } = &mut change {
        route.access = Some(AccessRule {
            emails: vec!["me@xyz.com".into()],
            email_domains: Vec::new(),
            bypass: vec!["/webhooks".into()],
        });
    }
    let outcome = apply(&engine, &api, &conns, change).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(
        api.identity_providers("e2e-account").await.unwrap()[0].kind,
        "onetimepin"
    );
    let apps = api
        .access_apps_for("e2e-account", "app.xyz.com")
        .await
        .unwrap();
    assert_eq!(apps.len(), 1);
    // Webhooks skip it through an application of their own that lets everyone in.
    let open = api
        .access_apps_for("e2e-account", "app.xyz.com/webhooks")
        .await
        .unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(open[0].policies[0].decision, "bypass");
    let checked = engine
        .verify(&api, CTX, &host, Edge::Test(addr), Duration::ZERO)
        .await
        .unwrap();
    assert!(checked.ok() && checked.protected, "{checked:?}");

    let remove = Change::RemoveRoute {
        hostname: "app.xyz.com".into(),
        path: None,
    };
    let outcome = apply(&engine, &api, &conns, remove).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert!(
        api.access_apps_for("e2e-account", "app.xyz.com")
            .await
            .unwrap()
            .is_empty(),
        "the login went with the route"
    );
    assert!(
        api.access_apps_for("e2e-account", "app.xyz.com/webhooks")
            .await
            .unwrap()
            .is_empty(),
        "and so did the path that skipped it"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_second_tunnel_carries_its_own_routes() {
    let (_fake, addr) = spawn_fake().await;
    let api = Client::with_base(&format!("http://{addr}"), ApiToken::new("e2e")).unwrap();
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let conns = Recorder::default();

    let outcome = apply(&engine, &api, &conns, add("xyz.com", "3000")).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let create = Change::CreateTunnel {
        name: "staging".into(),
    };
    let outcome = apply(&engine, &api, &conns, create).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let staging = engine
        .local()
        .tunnels("e2e-account")
        .await
        .unwrap()
        .into_iter()
        .find(|t| !t.is_default)
        .unwrap();

    let on = Some(staging.tunnel_id.as_str());
    let outcome = apply_on(&engine, &api, &conns, on, add("app.yx.com", "5000")).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    // Verify finds the tunnel carrying the hostname by itself.
    let checked = engine
        .verify(
            &api,
            CTX,
            &Hostname::parse("app.yx.com").unwrap(),
            Edge::Test(addr),
            Duration::ZERO,
        )
        .await
        .unwrap();
    assert!(checked.ok(), "{checked:?}");

    let names: Vec<String> = api
        .tunnels("e2e-account")
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"staging".to_owned()));
    let overview = engine.overview(&api, &conns, CTX).await.unwrap();
    assert_eq!(overview.tunnels.len(), 2);
    assert_eq!(overview.routes.len(), 2);

    // Deleting the second tunnel leaves the first and its route.
    let outcome = apply_on(&engine, &api, &conns, on, Change::RemoveTunnel).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(api.tunnels("e2e-account").await.unwrap().len(), 1);
    let overview = engine.overview(&api, &conns, CTX).await.unwrap();
    assert_eq!(
        overview
            .routes
            .iter()
            .map(|r| r.hostname.as_str())
            .collect::<Vec<_>>(),
        ["xyz.com"]
    );
}

/// Which Worker the edge runs for `host` + `path` (the fake's `x-fake-worker`).
async fn worker_in_front(addr: SocketAddr, host: &str, path: &str) -> Option<String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
    let request = format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
    socket.write_all(request.as_bytes()).await.unwrap();
    let mut response = String::new();
    socket.read_to_string(&mut response).await.unwrap();
    response
        .lines()
        .take_while(|l| !l.is_empty())
        .find_map(|l| l.strip_prefix("x-fake-worker: "))
        .map(str::to_owned)
}

async fn front(
    engine: &Engine,
    api: &Client,
    conns: &Recorder,
    secrets: &teitunnel_core::secrets::Secrets,
    change: &teitunnel_core::fronts::FrontChange,
) -> Outcome {
    use teitunnel_core::fronts;
    let plan = fronts::preview(engine, api, Some(secrets), CTX, change)
        .await
        .unwrap();
    let approval = Approval {
        fingerprint: &plan.fingerprint,
        confirmed: false,
    };
    fronts::apply(
        engine,
        api,
        conns,
        Some(secrets),
        CTX,
        change,
        approval,
        |_| {},
    )
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_offline_page_and_an_inbox_go_with_their_route() {
    use teitunnel_core::{
        engine::front::{InboxSettings, InboxVerify, OfflinePage},
        fronts::{self, FrontChange},
        secrets::{MemoryStore, Secrets},
    };
    let (_fake, addr) = spawn_fake().await;
    let api = Client::with_base(&format!("http://{addr}"), ApiToken::new("e2e")).unwrap();
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let conns = Recorder::default();
    let secrets: Secrets = std::sync::Arc::new(MemoryStore::default());

    let outcome = apply(&engine, &api, &conns, add("app.xyz.com", "3000")).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");

    let offline = FrontChange::Offline {
        hostname: "app.xyz.com".into(),
        page: Some(OfflinePage {
            title: "Back soon".into(),
            message: "Out for lunch.".into(),
            when_app_down: false,
        }),
    };
    let outcome = front(&engine, &api, &conns, &secrets, &offline).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let page = worker_in_front(addr, "app.xyz.com", "/").await.unwrap();
    assert!(page.starts_with("tt-offline-"), "{page}");

    // A verifying inbox takes the signing secret saved for the hostname.
    fronts::set_inbox_secret(
        &secrets,
        "app.xyz.com",
        InboxVerify::Github,
        Secret::new("gh-secret".into()),
    )
    .await
    .unwrap();
    let inbox = FrontChange::Inbox {
        hostname: "app.xyz.com".into(),
        path: "/hooks/".into(),
        inbox: Some(InboxSettings {
            verify: Some(InboxVerify::Github),
            ..InboxSettings::default()
        }),
    };
    let outcome = front(&engine, &api, &conns, &secrets, &inbox).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let hooks = worker_in_front(addr, "app.xyz.com", "/hooks/push")
        .await
        .unwrap();
    assert!(hooks.starts_with("tt-inbox-"), "{hooks}");
    // The most specific route wins: other paths still get the offline page.
    assert_eq!(
        worker_in_front(addr, "app.xyz.com", "/about").await,
        Some(page.clone())
    );
    assert_eq!(api.worker_routes("z-xyz").await.unwrap().len(), 2);
    assert_eq!(
        api.d1_databases(
            "e2e-account",
            teitunnel_core::comments::remote::DATABASE_NAME
        )
        .await
        .unwrap()
        .len(),
        1,
        "the inbox keeps webhooks in the account's database"
    );
    assert_eq!(fronts::list(&engine, None).await.unwrap().len(), 2);
    // The same change again has nothing to do.
    let again = fronts::preview(&engine, &api, Some(&secrets), CTX, &inbox)
        .await
        .unwrap();
    assert!(again.steps.is_empty(), "{again:?}");

    // Removing the route takes its Workers and their routes with it.
    let remove = Change::RemoveRoute {
        hostname: "app.xyz.com".into(),
        path: None,
    };
    let outcome = apply(&engine, &api, &conns, remove).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert!(api.worker_routes("z-xyz").await.unwrap().is_empty());
    for script in [&page, &hooks] {
        assert!(
            api.worker_deployments("e2e-account", script)
                .await
                .unwrap()
                .is_none(),
            "{script} left behind"
        );
    }
    assert!(fronts::list(&engine, None).await.unwrap().is_empty());
    assert_eq!(worker_in_front(addr, "app.xyz.com", "/").await, None);

    // Deleting the tunnel with the route still on it does the same.
    let outcome = apply(&engine, &api, &conns, add("app.xyz.com", "3000")).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let outcome = front(&engine, &api, &conns, &secrets, &offline).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let outcome = apply(&engine, &api, &conns, Change::RemoveTunnel).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert!(api.worker_routes("z-xyz").await.unwrap().is_empty());
    assert!(fronts::list(&engine, None).await.unwrap().is_empty());
}
