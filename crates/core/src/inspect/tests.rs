//! The inspector with a real Lens: taps, captures, masking, live host headers,
//! watched paths, idle stops, history across restarts, and route inspection through
//! the engine (against the fake Cloudflare).

use std::{sync::Arc, time::Duration};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::*;
use crate::{
    engine::{
        Engine, Local,
        fake::{CloudState, FakeCloud, FakeConnectors},
    },
    secrets::MemoryStore,
};

/// A tiny origin: answers every request with `200 hello` and the Host it got in
/// `x-host`.
pub(crate) async fn origin() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 16 * 1024];
                let mut head = Vec::new();
                loop {
                    let Ok(n) = socket.read(&mut buf).await else {
                        return;
                    };
                    if n == 0 {
                        return;
                    }
                    head.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&head).to_string();
                    let Some(end) = text.find("\r\n\r\n") else {
                        continue;
                    };
                    let length: usize = text[..end]
                        .lines()
                        .find_map(|l| {
                            l.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap_or(0))
                        })
                        .unwrap_or(0);
                    if head.len() < end + 4 + length {
                        continue;
                    }
                    let host = text[..end]
                        .lines()
                        .find_map(|l| {
                            let (name, value) = l.split_once(':')?;
                            name.eq_ignore_ascii_case("host")
                                .then(|| value.trim().to_owned())
                        })
                        .unwrap_or_default();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nx-host: {host}\r\nContent-Length: 5\r\n\r\nhello"
                    );
                    if socket.write_all(response.as_bytes()).await.is_err() {
                        return;
                    }
                    head.clear();
                }
            });
        }
    });
    format!("http://{addr}")
}

/// Sends a request to a tap as cloudflared would, returning the status and `x-host`.
pub(crate) async fn send(
    tap_url: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
) -> (u16, String) {
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let mut request = client
        .request(method.parse().unwrap(), format!("{tap_url}{path}"))
        .header("host", "demo.example.com")
        .header("cf-connecting-ip", "203.0.113.9");
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    if method == "POST" {
        request = request.body(r#"{"event":"paid","apiKey":"sk_live_0123456789abcdef"}"#);
    }
    let response = request.send().await.unwrap();
    let status = response.status().as_u16();
    let host = response
        .headers()
        .get("x-host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let _ = response.bytes().await;
    (status, host)
}

async fn settle(inspector: &Inspector, count: usize) {
    for _ in 0..200 {
        if inspector.list(&ExchangeQuery::default()).items.len() >= count
            && inspector
                .list(&ExchangeQuery::default())
                .items
                .iter()
                .all(|r| r.state != lens::ExchangeState::Pending)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("captures didn't arrive");
}

fn quick(id: &str) -> TapScope {
    TapScope::QuickShare {
        share_id: id.into(),
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn captures_requests_masked_and_changes_the_host_header_live() {
    let origin = origin().await;
    let secrets: Secrets = Arc::new(MemoryStore::default());
    let inspector = Inspector::new(None, Some(secrets), "app");
    let tap = inspector
        .start(TapSpec::new(quick("qs-test1"), "demo", &origin))
        .await
        .unwrap();
    assert_eq!(tap.id.as_str(), "qs-test1");
    assert_eq!(inspector.tap_for(&quick("qs-test1")), Some(tap.id.clone()));

    let (status, host) = send(
        &tap.address,
        "POST",
        "/hooks?token=abc123",
        &[("authorization", "Bearer secret-value-123")],
    )
    .await;
    assert_eq!((status, host.as_str()), (200, "demo.example.com"));
    settle(&inspector, 1).await;
    let page = inspector.list(&ExchangeQuery::default());
    let row = &page.items[0];
    assert_eq!((row.method.as_str(), row.status), ("POST", Some(200)));
    assert!(!row.path.contains("abc123"), "{}", row.path);

    let masked = inspector.detail(row.id, false).await.unwrap();
    let json = serde_json::to_string(&masked.view).unwrap();
    assert!(!json.contains("secret-value-123") && !json.contains("sk_live_0123456789abcdef"));
    assert!(masked.view.redacted && !masked.restored);
    let revealed = inspector.detail(row.id, true).await.unwrap();
    assert!(
        serde_json::to_string(&revealed.view)
            .unwrap()
            .contains("secret-value-123")
    );
    assert_eq!(revealed.view.client.ip.to_string(), "203.0.113.9");

    // The dev-server fix, without a restart.
    inspector
        .set_host_header(&tap.id, Some("localhost:5173".into()))
        .unwrap();
    let (_, host) = send(&tap.address, "GET", "/", &[]).await;
    assert_eq!(host, "localhost:5173");
    assert_eq!(
        inspector.view(&tap.id).unwrap().host_header.as_deref(),
        Some("localhost:5173")
    );
    inspector.set_host_header(&tap.id, None).unwrap();
    let (_, host) = send(&tap.address, "GET", "/", &[]).await;
    assert_eq!(host, "demo.example.com");

    // Exports are redacted unless asked not to be.
    settle(&inspector, 3).await;
    let curl = inspector
        .export(&[row.id], ExportFormat::Curl, true)
        .unwrap();
    assert!(curl.starts_with("curl") && !curl.contains("secret-value-123"));
    let har = inspector
        .export(
            &inspector
                .list(&ExchangeQuery::default())
                .items
                .iter()
                .map(|r| r.id)
                .collect::<Vec<_>>(),
            ExportFormat::Har,
            true,
        )
        .unwrap();
    let har: serde_json::Value = serde_json::from_str(&har).unwrap();
    assert_eq!(har["log"]["entries"].as_array().unwrap().len(), 3);

    // Replays are captured too.
    let replays = inspector
        .replay(
            row.id,
            ReplayInput {
                set_headers: vec![("x-debug".into(), "1".into())],
                times: Some(2),
                ..ReplayInput::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(replays.len(), 2);
    assert!(replays.iter().all(|r| r.replay_of == Some(row.id)));

    let metrics = inspector.metrics(&tap.id).unwrap();
    assert!(metrics.requests >= 3);
    inspector.clear(Some(&tap.id)).await.unwrap();
    assert!(inspector.list(&ExchangeQuery::default()).items.is_empty());
    inspector.stop(&tap.id).await;
    assert!(inspector.taps().is_empty());
    inspector.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn tap_settings_change_at_once() {
    let origin = origin().await;
    let inspector = Inspector::new(None, None, "app");
    let tap = inspector
        .start(TapSpec::new(quick("qs-test2"), "demo", &origin))
        .await
        .unwrap();
    let view = inspector
        .configure(
            &tap.id,
            &TapPatch {
                paused: Some(true),
                network_preset: Some(NetworkPreset::FourG),
                sse_keepalive_secs: Some(0),
                watched_paths: Some(vec!["/webhooks/*".into()]),
                idle_stop_minutes: Some(5),
                ..TapPatch::default()
            },
        )
        .unwrap();
    assert!(view.paused.is_some());
    assert!(view.network.latency.is_some());
    assert_eq!(view.sse_keepalive_secs, None);
    assert_eq!(view.watched_paths, ["/webhooks/*"]);
    assert_eq!(view.idle_stop_minutes, Some(5));
    let (status, _) = send(&tap.address, "GET", "/", &[]).await;
    assert_eq!(status, 503, "the paused page answers");
    assert!(
        inspector
            .configure(
                &tap.id,
                &TapPatch {
                    watched_paths: Some(vec!["re:(".into()]),
                    ..TapPatch::default()
                },
            )
            .is_err(),
        "an invalid pattern changes nothing"
    );

    let protected = inspector
        .protect(
            &tap.id,
            ProtectionInput {
                bearer: Some(true),
                ip_deny: Some(vec!["192.0.2.0/24".into()]),
                ..ProtectionInput::default()
            },
        )
        .await
        .unwrap();
    let token = protected.bearer_token.unwrap();
    assert_eq!(protected.protection.bearer_tokens, 1);
    assert_eq!(protected.protection.ip_deny, ["192.0.2.0/24"]);
    inspector
        .configure(
            &tap.id,
            &TapPatch {
                paused: Some(false),
                ..TapPatch::default()
            },
        )
        .unwrap();
    let (status, _) = send(&tap.address, "GET", "/", &[]).await;
    assert_eq!(status, 401, "no token");
    let (status, _) = send(
        &tap.address,
        "GET",
        "/",
        &[("authorization", &format!("Bearer {token}"))],
    )
    .await;
    assert_eq!(status, 200);
    inspector.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn reports_watched_paths() {
    let origin = origin().await;
    let inspector = Inspector::new(None, None, "app");
    let mut events = inspector.subscribe();
    let tap = inspector
        .start(TapSpec::new(quick("qs-test3"), "demo", &origin))
        .await
        .unwrap();
    inspector
        .configure(
            &tap.id,
            &TapPatch {
                watched_paths: Some(vec!["/webhooks/*".into()]),
                ..TapPatch::default()
            },
        )
        .unwrap();
    send(&tap.address, "GET", "/other", &[]).await;
    send(&tap.address, "POST", "/webhooks/stripe", &[]).await;
    let watched = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(InspectEvent::Watched { path, method, .. }) = events.recv().await {
                return (method, path);
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(watched, ("POST".to_owned(), "/webhooks/stripe".to_owned()));
    inspector.shutdown().await;
}

#[tokio::test(start_paused = true)]
async fn reports_idle_taps() {
    let origin = origin().await;
    let inspector = Inspector::new(None, None, "app");
    let mut events = inspector.subscribe();
    let tap = inspector
        .start(TapSpec::new(quick("qs-test4"), "demo", &origin))
        .await
        .unwrap();
    inspector
        .configure(
            &tap.id,
            &TapPatch {
                idle_stop_minutes: Some(10),
                ..TapPatch::default()
            },
        )
        .unwrap();
    tokio::time::sleep(Duration::from_secs(9 * 60)).await;
    assert!(
        !matches!(events.try_recv(), Ok(InspectEvent::Idle { .. })),
        "not yet"
    );
    tokio::time::sleep(Duration::from_secs(2 * 60)).await;
    let idle = loop {
        if let InspectEvent::Idle { tap, minutes, .. } = events.recv().await.unwrap() {
            break (tap, minutes);
        }
    };
    assert_eq!(idle, (tap.id.clone(), 10));
    inspector.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn history_survives_a_restart_masked() {
    let origin = origin().await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("teitunnel.db");
    let store = Store::open(&path).unwrap();
    let inspector = Inspector::new(Some(store.clone()), None, "app");
    inspector.load().await.unwrap();
    let tap = inspector
        .start(TapSpec::new(quick("qs-test5"), "demo", &origin))
        .await
        .unwrap();
    send(
        &tap.address,
        "POST",
        "/pay",
        &[("authorization", "Bearer secret-value-456")],
    )
    .await;
    settle(&inspector, 1).await;
    inspector.shutdown().await;

    let again = Inspector::new(Some(store.clone()), None, "app");
    again.load().await.unwrap();
    let rows = again.list(&ExchangeQuery::default()).items;
    assert_eq!(rows.len(), 1);
    let detail = again.detail(rows[0].id, true).await.unwrap();
    assert!(detail.restored);
    let json = serde_json::to_string(&detail.view).unwrap();
    assert!(!json.contains("secret-value-456"), "stored masked: {json}");
    assert!(
        again
            .known_taps()
            .iter()
            .any(|t| t.id.as_str() == "qs-test5" && !t.running)
    );
    // Another process reads the same history.
    let read = history(
        &store,
        HistoryQuery {
            limit: 10,
            ..HistoryQuery::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(read.len(), 1);
    // Replaying a restored capture needs its tap.
    assert!(matches!(
        again.replay(rows[0].id, ReplayInput::default()).await,
        Err(InspectError::TapGone)
    ));
}

#[tokio::test(flavor = "multi_thread")]
async fn verifies_webhooks_with_the_saved_secret() {
    let origin = origin().await;
    let secrets: Secrets = Arc::new(MemoryStore::default());
    let inspector = Inspector::new(None, Some(secrets.clone()), "app");
    let tap = inspector
        .start(TapSpec::new(quick("qs-test6"), "demo", &origin))
        .await
        .unwrap();
    // A GitHub delivery signed with "s3cret".
    let body = r#"{"event":"paid","apiKey":"sk_live_0123456789abcdef"}"#;
    let signature = {
        let mut headers = http::HeaderMap::new();
        webhook::resign(
            webhook::Provider::GitHub,
            &mut headers,
            body.as_bytes(),
            &webhook::WebhookSecret::new("s3cret"),
            0,
        )
        .unwrap();
        headers["x-hub-signature-256"].to_str().unwrap().to_owned()
    };
    send(
        &tap.address,
        "POST",
        "/gh",
        &[("x-hub-signature-256", &signature)],
    )
    .await;
    settle(&inspector, 1).await;
    let id = inspector.list(&ExchangeQuery::default()).items[0].id;
    let check = inspector.detail(id, false).await.unwrap().webhook.unwrap();
    assert_eq!(check.provider, WebhookSender::GitHub);
    assert!(!check.has_secret && check.verification.is_none());
    let scope = inspector.webhook_scope(&tap.id).unwrap();
    secrets::set_webhook_secret(
        &secrets,
        &scope,
        webhook::Provider::GitHub,
        Secret::new("s3cret".into()),
    )
    .await
    .unwrap();
    let check = inspector.detail(id, false).await.unwrap().webhook.unwrap();
    assert_eq!(check.verification, Some(WebhookVerdict::Valid));
    inspector.shutdown().await;
}

// ---- Route inspection through the engine -------------------------------------------

const ACCOUNT: &str = "acc";

fn ctx() -> crate::engine::Context<'static> {
    crate::engine::Context {
        account: ACCOUNT,
        machine_name: "Mac",
        tunnel: None,
    }
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
    let change = crate::engine::Change::AddRoute {
        route: crate::engine::RouteInput {
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
            crate::engine::Approval {
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
async fn inspecting_a_route_points_it_at_the_tap_and_back() {
    let origin = origin().await;
    let (engine, cloud, conns) = routed(&origin).await;
    let inspector = Inspector::new(
        Some(engine.local().store().clone()),
        None,
        crate::domain_shares::APP_OWNER,
    );
    let plan = routes::plan_on(
        &engine,
        &cloud,
        &conns,
        ctx(),
        &inspector,
        "app.xyz.com",
        None,
    )
    .await
    .unwrap();
    assert!(!plan.plan.steps.is_empty());
    let outcome = routes::apply_on(
        &engine,
        &cloud,
        &conns,
        ctx(),
        &inspector,
        "app.xyz.com",
        None,
        crate::engine::Approval {
            fingerprint: &plan.plan.fingerprint,
            confirmed: false,
        },
        |_| {},
    )
    .await
    .unwrap();
    assert!(
        matches!(outcome, crate::engine::Outcome::Applied { .. }),
        "{outcome:?}"
    );
    let tap = inspector.taps().pop().unwrap();
    assert_eq!(service(&cloud), tap.address);
    let remembered = routes::list(engine.local().store(), None).await.unwrap();
    assert_eq!(remembered.len(), 1);
    assert_eq!(remembered[0].original_origin, origin);
    assert_eq!(remembered[0].lens_url, tap.address);
    // Traffic through the route's tap is captured under the route.
    send(&tap.address, "GET", "/", &[]).await;
    settle(&inspector, 1).await;

    // Twice is refused.
    assert!(
        routes::plan_on(
            &engine,
            &cloud,
            &conns,
            ctx(),
            &inspector,
            "app.xyz.com",
            None
        )
        .await
        .is_err()
    );

    let off = routes::plan_off(&engine, &cloud, &conns, ctx(), "app.xyz.com", None)
        .await
        .unwrap()
        .unwrap();
    let outcome = routes::apply_off(
        &engine,
        &cloud,
        &conns,
        ctx(),
        Some(&inspector),
        "app.xyz.com",
        None,
        crate::engine::Approval {
            fingerprint: &off.plan.fingerprint,
            confirmed: false,
        },
        |_| {},
    )
    .await
    .unwrap();
    assert!(matches!(
        outcome,
        Some(crate::engine::Outcome::Applied { .. })
    ));
    assert_eq!(service(&cloud), origin);
    assert!(inspector.taps().is_empty());
    assert!(
        routes::list(engine.local().store(), None)
            .await
            .unwrap()
            .is_empty()
    );
    inspector.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_route_left_inspected_is_swept_back() {
    let origin = origin().await;
    let (engine, cloud, conns) = routed(&origin).await;
    // A previous run (crashed): the route points at a tap nobody runs.
    let crashed = Inspector::new(
        Some(engine.local().store().clone()),
        None,
        crate::domain_shares::APP_OWNER,
    );
    let plan = routes::plan_on(
        &engine,
        &cloud,
        &conns,
        ctx(),
        &crashed,
        "app.xyz.com",
        None,
    )
    .await
    .unwrap();
    routes::apply_on(
        &engine,
        &cloud,
        &conns,
        ctx(),
        &crashed,
        "app.xyz.com",
        None,
        crate::engine::Approval {
            fingerprint: &plan.plan.fingerprint,
            confirmed: false,
        },
        |_| {},
    )
    .await
    .unwrap();
    let lens_url = service(&cloud);
    crashed.shutdown().await;

    // The Doctor sees it.
    let port: u16 = lens_url.rsplit(':').next().unwrap().parse().unwrap();
    let orphans = routes::orphans(
        &routes::list(engine.local().store(), Some(ACCOUNT))
            .await
            .unwrap(),
        &[("app.xyz.com".to_owned(), None, lens_url.clone())],
        &std::collections::HashMap::from([(port, false)]),
    );
    assert_eq!(orphans.len(), 1);

    let remembered = routes::list(engine.local().store(), None).await.unwrap();
    for route in &remembered {
        routes::revert(&engine, &cloud, &conns, "Mac", None, route)
            .await
            .unwrap();
    }
    assert_eq!(service(&cloud), origin);
    assert!(
        routes::list(engine.local().store(), None)
            .await
            .unwrap()
            .is_empty()
    );
}
