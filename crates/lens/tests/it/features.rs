//! Stubs, injection, reserved paths, paused page, header rules, replay, webhooks.

use std::{
    io::Write,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
};

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body_util::BodyExt;
use lens::{
    ExchangeState, Filter, HandlerFuture, HeaderOp, HeaderRules, Injection, PathPattern,
    PausedPage, ReplayOptions, ReplayTarget, RequestEdits, ReservedHandler, ReservedRequest,
    Resign, Responder, StubMode, StubRule, Upstream,
    webhook::{self, Provider, Verification, WebhookSecret},
};

use crate::support::*;

#[tokio::test]
async fn fallback_stub_answers_while_the_origin_is_down() {
    let url = closed_port().await;
    let (lens, tap) = lens_with(Upstream::origin(&url).unwrap(), |config| {
        config.stubs = vec![StubRule {
            method: Some("POST".into()),
            headers: vec![("content-type".into(), "application/json".into())],
            ..StubRule::new(
                PathPattern::parse("/webhooks/*").unwrap(),
                202,
                r#"{"queued":true}"#,
            )
        }];
    })
    .await;
    let reply = fetch(
        tap.addr,
        request(Method::POST, "/webhooks/stripe")
            .body(body(r#"{"id":"evt_1"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::ACCEPTED);
    assert_eq!(reply.text(), r#"{"queued":true}"#);
    assert_eq!(reply.headers["x-teitunnel-stub"], "1");
    let exchange = exchange_for(&lens, "/webhooks/stripe").await;
    assert_eq!(
        exchange.responder,
        Responder::Stub {
            rule: 0,
            fallback: true
        }
    );
    assert_eq!(
        &exchange.request.body.data[..],
        br#"{"id":"evt_1"}"#,
        "the webhook is recorded"
    );
    // Other paths still fail normally.
    assert_eq!(
        get(tap.addr, "/other").await.status,
        StatusCode::BAD_GATEWAY
    );
    assert_eq!(lens.metrics(&tap.id).unwrap().stubbed, 1);
}

#[tokio::test]
async fn fallback_stub_stays_out_of_the_way_while_the_origin_is_up() {
    let hits = Arc::new(AtomicUsize::new(0));
    let origin = {
        let hits = Arc::clone(&hits);
        origin(move |request: Request<hyper::body::Incoming>| {
            let hits = Arc::clone(&hits);
            async move {
                hits.fetch_add(1, SeqCst);
                let body = request.into_body().collect().await.unwrap().to_bytes();
                text_response(200, body)
            }
        })
        .await
    };
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.capture.max_body_bytes = 8;
        config.stubs = vec![StubRule::new(
            PathPattern::parse("/hooks/*").unwrap(),
            200,
            "stub",
        )];
    })
    .await;
    // A body larger than the capture cap still streams to the origin.
    let reply = fetch(
        tap.addr,
        request(Method::POST, "/hooks/a")
            .body(body("0123456789abcdef"))
            .unwrap(),
    )
    .await;
    assert_eq!(reply.text(), "0123456789abcdef");
    let reply = fetch(
        tap.addr,
        request(Method::POST, "/hooks/b")
            .body(body("small"))
            .unwrap(),
    )
    .await;
    assert_eq!(reply.text(), "small");
    assert_eq!(hits.load(SeqCst), 2);
}

#[tokio::test]
async fn always_stubs_never_reach_the_origin() {
    let hits = Arc::new(AtomicUsize::new(0));
    let origin = {
        let hits = Arc::clone(&hits);
        origin(move |_| {
            let hits = Arc::clone(&hits);
            async move {
                hits.fetch_add(1, SeqCst);
                text_response(200, "origin")
            }
        })
        .await
    };
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.stubs = vec![StubRule {
            mode: StubMode::Always,
            ..StubRule::new(PathPattern::parse("re:^/mock/").unwrap(), 418, "teapot")
        }];
    })
    .await;
    let reply = fetch(
        tap.addr,
        request(Method::PUT, "/mock/x")
            .body(body("payload"))
            .unwrap(),
    )
    .await;
    assert_eq!(
        (reply.status.as_u16(), reply.text().as_str()),
        (418, "teapot")
    );
    assert_eq!(get(tap.addr, "/real").await.text(), "origin");
    assert_eq!(hits.load(SeqCst), 1);
    let exchange = exchange_for(&lens, "/mock/x").await;
    assert_eq!(&exchange.request.body.data[..], b"payload");
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

const PAGE: &[u8] = b"<!doctype html><html><body><h1>Hello</h1></body></html>";
const SNIPPET: &str = "<script src=\"/__teitunnel/overlay.js\" defer></script>";

#[tokio::test]
async fn injection_strips_accept_encoding_and_injects() {
    let seen_encoding = Arc::new(Mutex::new(None::<String>));
    let origin = {
        let seen = Arc::clone(&seen_encoding);
        origin(move |request: Request<hyper::body::Incoming>| {
            let seen = Arc::clone(&seen);
            async move {
                let accept = request
                    .headers()
                    .get("accept-encoding")
                    .map(|v| v.to_str().unwrap().to_owned());
                *seen.lock().unwrap() = Some(accept.clone().unwrap_or_default());
                let gzip_ok = accept.is_some_and(|v| v.contains("gzip"));
                let mut response = if gzip_ok {
                    let mut r = Response::new(lens::full(gzip(PAGE)));
                    r.headers_mut()
                        .insert("content-encoding", "gzip".parse().unwrap());
                    r
                } else {
                    Response::new(lens::full(PAGE))
                };
                response
                    .headers_mut()
                    .insert("content-type", "text/html; charset=utf-8".parse().unwrap());
                response
            }
        })
        .await
    };
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.injection = Some(Injection::new(SNIPPET));
    })
    .await;
    let navigation = |path: &str| {
        request(Method::GET, path)
            .header("accept", "text/html,application/xhtml+xml")
            .header("accept-encoding", "gzip, br")
            .header("sec-fetch-dest", "document")
            .body(lens::empty())
            .unwrap()
    };
    let reply = fetch(tap.addr, navigation("/page")).await;
    assert_eq!(
        seen_encoding.lock().unwrap().as_deref(),
        Some(""),
        "Accept-Encoding stripped"
    );
    assert_eq!(
        reply.text(),
        format!("<!doctype html><html><body><h1>Hello</h1>{SNIPPET}</body></html>")
    );
    // The capture holds what the origin sent.
    let exchange = exchange_for(&lens, "/page").await;
    assert_eq!(&exchange.response.as_ref().unwrap().body.data[..], PAGE);

    // API calls (fetch/XHR) keep compression and get no snippet.
    let api = fetch(
        tap.addr,
        request(Method::GET, "/api")
            .header("accept-encoding", "gzip")
            .header("sec-fetch-dest", "empty")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(api.headers["content-encoding"], "gzip");
    assert_eq!(&api.body[..], &gzip(PAGE)[..]);
}

#[tokio::test]
async fn injection_decompresses_when_the_origin_insists_on_gzip() {
    let origin = origin(|_| async {
        let compressed = gzip(PAGE);
        let mut response = Response::new(lens::full(compressed.clone()));
        let headers = response.headers_mut();
        headers.insert("content-type", "text/html".parse().unwrap());
        headers.insert("content-encoding", "gzip".parse().unwrap());
        headers.insert("content-length", compressed.len().into());
        response
    })
    .await;
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.injection = Some(Injection::new(SNIPPET));
    })
    .await;
    let reply = fetch(
        tap.addr,
        request(Method::GET, "/")
            .header("accept", "text/html")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert!(!reply.headers.contains_key("content-encoding"));
    assert_eq!(
        reply.headers["content-length"],
        reply.body.len().to_string().as_str()
    );
    assert!(reply.text().contains(&format!("{SNIPPET}</body>")));
}

#[derive(Debug)]
struct Echo;

impl ReservedHandler for Echo {
    fn handle(&self, request: ReservedRequest) -> HandlerFuture {
        Box::pin(async move {
            let text = format!(
                "{} {} {}",
                request.tap,
                request.uri.path(),
                request.body.len()
            );
            text_response(200, text)
        })
    }
}

#[tokio::test]
async fn reserved_paths_are_answered_by_lens_behind_the_gates() {
    let origin = origin(|_| async { text_response(200, "origin") }).await;
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.reserved = Some(Arc::new(Echo));
        config.gates.basic = Some(lens::BasicAuth::new("u", "p").unwrap());
    })
    .await;
    assert_eq!(
        get(tap.addr, "/__teitunnel/comments").await.status,
        StatusCode::UNAUTHORIZED
    );
    let auth = format!(
        "Basic {}",
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "u:p")
    );
    let reply = fetch(
        tap.addr,
        request(Method::POST, "/__teitunnel/comments")
            .header("authorization", auth)
            .body(body("abc"))
            .unwrap(),
    )
    .await;
    assert_eq!(reply.text(), format!("{} /__teitunnel/comments 3", tap.id));
}

#[tokio::test]
async fn paused_page_and_resume() {
    let origin = origin(|_| async { text_response(200, "live") }).await;
    let (lens, tap) = lens_for(&origin.url).await;
    lens.update_tap(&tap.id, |config| {
        config.paused = Some(PausedPage {
            title: "Back soon <3".into(),
            message: "Deploying.".into(),
            retry_after_secs: 30,
        });
    })
    .unwrap();
    let paused = get(tap.addr, "/").await;
    assert_eq!(paused.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(paused.headers["retry-after"], "30");
    assert!(paused.text().contains("Back soon &lt;3"));
    lens.update_tap(&tap.id, |config| config.paused = None)
        .unwrap();
    assert_eq!(get(tap.addr, "/").await.text(), "live");
    let first = next_exchange(&lens, Filter::default()).await;
    assert_eq!(first.responder, Responder::Paused);
}

#[tokio::test]
async fn header_rules_and_cors_helper() {
    let origin = origin(|request: Request<hyper::body::Incoming>| async move {
        let custom = request
            .headers()
            .get("x-added")
            .map_or("-", |v| v.to_str().unwrap_or("?"))
            .to_owned();
        let mut response = text_response(200, custom);
        response
            .headers_mut()
            .insert("x-powered-by", "Express".parse().unwrap());
        response
    })
    .await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.headers = HeaderRules {
            request: vec![HeaderOp::Set {
                name: "x-added".into(),
                value: "yes".into(),
            }],
            response: vec![HeaderOp::Remove {
                name: "x-powered-by".into(),
            }],
            cors: true,
        };
    })
    .await;
    let reply = fetch(
        tap.addr,
        request(Method::GET, "/cors")
            .header("origin", "https://ui.test")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(reply.text(), "yes");
    assert!(!reply.headers.contains_key("x-powered-by"));
    assert_eq!(
        reply.headers["access-control-allow-origin"],
        "https://ui.test"
    );
    let preflight = fetch(
        tap.addr,
        request(Method::OPTIONS, "/cors")
            .header("origin", "https://ui.test")
            .header("access-control-request-method", "DELETE")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(preflight.status, StatusCode::NO_CONTENT);
    assert_eq!(preflight.headers["access-control-allow-methods"], "DELETE");
    let exchange = next_exchange(
        &lens,
        Filter {
            methods: vec!["OPTIONS".into()],
            ..Filter::default()
        },
    )
    .await;
    assert_eq!(exchange.responder, Responder::Lens);
}

#[tokio::test]
async fn replay_as_is_edited_repeated_and_elsewhere() {
    let other = origin(|_| async { text_response(200, "other") }).await;
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let origin = {
        let log = Arc::clone(&log);
        origin(move |request: Request<hyper::body::Incoming>| {
            let log = Arc::clone(&log);
            async move {
                let (parts, body) = request.into_parts();
                let body = body.collect().await.unwrap().to_bytes();
                let flag = parts
                    .headers
                    .get("x-flag")
                    .map_or("-", |v| v.to_str().unwrap_or("?"))
                    .to_owned();
                log.lock().unwrap().push(format!(
                    "{} {} {flag} {}",
                    parts.method,
                    parts.uri,
                    String::from_utf8_lossy(&body)
                ));
                text_response(201, "ok")
            }
        })
        .await
    };
    let (lens, tap) = lens_for(&origin.url).await;
    fetch(
        tap.addr,
        request(Method::POST, "/orders?x=1")
            .body(body("original"))
            .unwrap(),
    )
    .await;
    let original = exchange_for(&lens, "/orders").await;

    let same = lens
        .replay(original.id, &ReplayOptions::default())
        .await
        .unwrap();
    assert_eq!(same.len(), 1);
    assert_eq!(same[0].replay_of, Some(original.id));
    assert_eq!(same[0].status(), Some(StatusCode::CREATED));
    assert_eq!(same[0].state, ExchangeState::Complete);

    let edited = lens
        .replay(
            original.id,
            &ReplayOptions {
                edits: RequestEdits {
                    method: Some("PUT".into()),
                    path_and_query: Some("/orders/7".into()),
                    set_headers: vec![("x-flag".into(), "edited".into())],
                    body: Some(Bytes::from_static(b"changed")),
                    ..RequestEdits::default()
                },
                times: 3,
                ..ReplayOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.len(), 3);
    let entries = log.lock().unwrap().clone();
    assert_eq!(entries[0], "POST /orders?x=1 - original");
    assert_eq!(entries[1], "POST /orders?x=1 - original");
    assert_eq!(&entries[2..], ["PUT /orders/7 edited changed"; 3]);

    let elsewhere = lens
        .replay(
            original.id,
            &ReplayOptions {
                target: ReplayTarget::Upstream(Upstream::origin(&other.url).unwrap()),
                ..ReplayOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        &elsewhere[0].response.as_ref().unwrap().body.data[..],
        b"other"
    );
    // Replays are captured like anything else.
    let replays = lens.list(
        &lens::Query {
            filter: Filter {
                path: Some("/orders".into()),
                ..Filter::default()
            },
            ..lens::Query::default()
        },
        &lens::Redaction::masked(),
    );
    assert_eq!(
        replays
            .items
            .iter()
            .filter(|e| e.replay_of.is_some())
            .count(),
        5
    );

    // A replay to a dead upstream is captured as failed.
    let dead = lens
        .replay(
            original.id,
            &ReplayOptions {
                target: ReplayTarget::Upstream(Upstream::origin(&closed_port().await).unwrap()),
                ..ReplayOptions::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(dead[0].state, ExchangeState::Failed);
}

#[tokio::test]
async fn replayed_webhooks_can_be_resigned() {
    let secret = WebhookSecret::new("whsec_local_test_secret");
    let verdicts = Arc::new(Mutex::new(Vec::<Verification>::new()));
    let origin = {
        let verdicts = Arc::clone(&verdicts);
        let secret = secret.clone();
        origin(move |request: Request<hyper::body::Incoming>| {
            let verdicts = Arc::clone(&verdicts);
            let secret = secret.clone();
            async move {
                let (parts, body) = request.into_parts();
                let body = body.collect().await.unwrap().to_bytes();
                let verdict = webhook::verify(
                    Provider::Stripe,
                    &webhook::VerifyInput {
                        headers: &parts.headers,
                        body: &body,
                        body_complete: true,
                        url: "",
                        now: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        tolerance: None,
                    },
                    &secret,
                );
                verdicts.lock().unwrap().push(verdict);
                text_response(200, "ok")
            }
        })
        .await
    };
    let (lens, tap) = lens_for(&origin.url).await;
    // An old delivery, signed an hour ago.
    let body_text = r#"{"type":"invoice.paid"}"#;
    let mut headers = http::HeaderMap::new();
    let old = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 3_600;
    webhook::resign(
        Provider::Stripe,
        &mut headers,
        body_text.as_bytes(),
        &secret,
        old,
    )
    .unwrap();
    fetch(
        tap.addr,
        request(Method::POST, "/webhooks/stripe")
            .header("stripe-signature", headers["stripe-signature"].clone())
            .body(body(body_text))
            .unwrap(),
    )
    .await;
    let original = exchange_for(&lens, "/webhooks/stripe").await;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    assert!(matches!(
        webhook::verify_exchange(&original, &secret, None, now),
        Verification::Expired { .. }
    ));
    lens.replay(original.id, &ReplayOptions::default())
        .await
        .unwrap();
    lens.replay(
        original.id,
        &ReplayOptions {
            resign: Some(Resign {
                provider: Provider::Stripe,
                secret: secret.clone(),
            }),
            ..ReplayOptions::default()
        },
    )
    .await
    .unwrap();
    let verdicts = verdicts.lock().unwrap().clone();
    assert!(
        matches!(verdicts[0], Verification::Expired { .. }),
        "{verdicts:?}"
    );
    assert!(
        matches!(verdicts[1], Verification::Expired { .. }),
        "as-is replay keeps the old signature"
    );
    assert_eq!(
        verdicts[2],
        Verification::Valid,
        "re-signed replay verifies"
    );
}

#[tokio::test]
async fn truncated_bodies_cant_be_replayed_as_is() {
    let origin = origin(|_| async { text_response(200, "ok") }).await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.capture.max_body_bytes = 4;
    })
    .await;
    fetch(
        tap.addr,
        request(Method::POST, "/big")
            .body(body("0123456789"))
            .unwrap(),
    )
    .await;
    let original = exchange_for(&lens, "/big").await;
    assert!(matches!(
        lens.replay(original.id, &ReplayOptions::default()).await,
        Err(lens::LensError::BodyTruncated {
            captured: 4,
            total: 10
        })
    ));
}
