//! Forwarding and capture: plain requests, bodies, errors, routing, metrics, h2.

use std::time::Duration;

use bytes::Bytes;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use lens::{
    CaptureConfig, ErrorKind, ExchangeKind, ExchangeState, Filter, HostHeader, Lens, LensEvent,
    LensOptions, ListenOptions, Query, Redaction, Responder, Routing, TapConfig, TapId, Upstream,
    WaitOptions,
};

use crate::support::*;

/// An origin that echoes the request line, headers it saw, and the body.
async fn echo_origin() -> Origin {
    origin(|request: Request<hyper::body::Incoming>| async move {
        let (parts, body) = request.into_parts();
        let body = body.collect().await.unwrap().to_bytes();
        let mut seen = format!("{} {}\n", parts.method, parts.uri);
        for (name, value) in &parts.headers {
            seen.push_str(&format!("{name}: {}\n", value.to_str().unwrap_or("?")));
        }
        seen.push('\n');
        let mut out = seen.into_bytes();
        out.extend_from_slice(&body);
        let mut response = text_response(200, out);
        response
            .headers_mut()
            .insert("set-cookie", "origin=1; Path=/".parse().unwrap());
        response
    })
    .await
}

#[tokio::test]
async fn forwards_and_captures_a_request() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.trust_cf_connecting_ip = true;
    })
    .await;
    let reply = fetch(
        tap.addr,
        request(Method::POST, "/api/items?page=2&token=abc")
            .header("content-type", "application/json")
            .header("authorization", "Bearer s3cr3t-value")
            .header("cf-connecting-ip", "203.0.113.7")
            .header("cf-ray", "8a1b2c3d4e5f-AMS")
            .header("x-forwarded-proto", "https")
            .header("connection", "keep-alive, x-hop")
            .header("x-hop", "drop me")
            .body(body(r#"{"name":"lens"}"#))
            .unwrap(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::OK);
    let seen = reply.text();
    assert!(
        seen.starts_with("POST /api/items?page=2&token=abc\n"),
        "{seen}"
    );
    assert!(
        seen.contains("host: app.test\n"),
        "Host is preserved: {seen}"
    );
    assert!(
        seen.contains("x-forwarded-proto: https\n"),
        "cloudflared's value kept"
    );
    assert!(seen.contains("x-forwarded-for: 203.0.113.7\n"));
    assert!(seen.contains("x-forwarded-host: app.test\n"));
    assert!(
        !seen.contains("x-hop"),
        "headers named in Connection are hop-by-hop"
    );
    assert!(!seen.contains("via:"), "no Via header");
    assert!(seen.ends_with(r#"{"name":"lens"}"#));

    let exchange = exchange_for(&lens, "/api/items").await;
    assert_eq!(exchange.state, ExchangeState::Complete);
    assert_eq!(exchange.kind, ExchangeKind::Http);
    assert_eq!(exchange.responder, Responder::Upstream);
    assert_eq!(exchange.request.method, Method::POST);
    assert_eq!(
        exchange.request.url(),
        "https://app.test/api/items?page=2&token=abc"
    );
    assert_eq!(exchange.client.ip.to_string(), "203.0.113.7");
    assert_eq!(exchange.client.cf_ray.as_deref(), Some("8a1b2c3d4e5f-AMS"));
    assert_eq!(&exchange.request.body.data[..], br#"{"name":"lens"}"#);
    assert!(exchange.request.body.complete && !exchange.request.body.truncated);
    let response = exchange.response.as_ref().unwrap();
    assert_eq!(response.status, StatusCode::OK);
    assert_eq!(response.body.size, reply.body.len() as u64);
    assert_eq!(response.body.data, reply.body);
    let t = exchange.timings;
    assert!(t.upstream_connected_us.is_some() && t.first_byte_us.is_some());
    assert!(t.complete_us >= t.first_byte_us);
    assert_eq!(exchange.seq, 1);

    // Views mask secrets by default and reveal them only when asked.
    let masked = exchange.view(&Redaction::masked());
    let auth = masked
        .request
        .headers
        .iter()
        .find(|h| h.name == "authorization")
        .unwrap();
    assert_eq!(auth.value, "Bearer [redacted]");
    assert_eq!(
        masked.request.query.as_deref(),
        Some("page=2&token=[redacted]")
    );
    let cookie = masked
        .response
        .as_ref()
        .unwrap()
        .headers
        .iter()
        .find(|h| h.name == "set-cookie")
        .unwrap();
    assert_eq!(cookie.value, "origin=[redacted]; Path=/");
    let revealed = exchange.view(&Redaction::revealed());
    assert!(
        revealed
            .request
            .headers
            .iter()
            .any(|h| h.value == "Bearer s3cr3t-value")
    );
}

#[tokio::test]
async fn keep_alive_connections_are_reused() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_for(&origin.url).await;
    for i in 0..5 {
        let reply = get(tap.addr, &format!("/reuse/{i}")).await;
        assert_eq!(reply.status, StatusCode::OK);
    }
    let page = lens.list(&Query::default(), &Redaction::masked());
    assert_eq!(page.items.len(), 5);
    let seqs: Vec<u64> = page.items.iter().map(|e| e.seq).collect();
    assert_eq!(seqs, vec![5, 4, 3, 2, 1], "newest first");
    assert_eq!(
        origin.connections.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "one pooled upstream connection serves sequential requests"
    );
}

#[tokio::test]
async fn bodies_beyond_the_cap_are_truncated_in_the_capture_only() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.capture = CaptureConfig {
            max_body_bytes: 16,
            ..CaptureConfig::default()
        };
    })
    .await;
    let payload = "x".repeat(100);
    let reply = fetch(
        tap.addr,
        request(Method::PUT, "/cap")
            .body(body(payload.clone()))
            .unwrap(),
    )
    .await;
    assert!(
        reply.text().ends_with(&payload),
        "the origin got everything"
    );
    let exchange = exchange_for(&lens, "/cap").await;
    let request = &exchange.request.body;
    assert_eq!(
        (request.data.len(), request.size, request.truncated),
        (16, 100, true)
    );
    let response = &exchange.response.as_ref().unwrap().body;
    assert_eq!(response.data.len(), 16);
    assert!(response.truncated && response.size > 100);
}

#[tokio::test]
async fn unreachable_upstream_is_a_502_with_the_reason_captured() {
    let url = closed_port().await;
    let (lens, tap) = lens_for(&url).await;
    let reply = fetch(
        tap.addr,
        request(Method::GET, "/down")
            .header("accept", "text/html")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(reply.status, StatusCode::BAD_GATEWAY);
    assert!(
        reply.text().contains("isn&#39;t answering") || reply.text().contains("isn't answering")
    );
    assert!(
        !reply.text().contains("127.0.0.1"),
        "visitors don't learn about the machine"
    );
    let exchange = exchange_for(&lens, "/down").await;
    assert_eq!(exchange.state, ExchangeState::Failed);
    let error = exchange.error.as_ref().unwrap();
    assert_eq!(error.kind, ErrorKind::ConnectionRefused);
    assert_eq!(exchange.responder, Responder::Lens);
    assert_eq!(lens.metrics(&tap.id).unwrap().errors, 1);
}

#[tokio::test]
async fn slow_upstreams_time_out_with_504() {
    let origin = origin(|_| async {
        tokio::time::sleep(Duration::from_secs(5)).await;
        text_response(200, "late")
    })
    .await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        if let Upstream::Origin(origin) = &mut config.upstream {
            origin.response_timeout = Some(Duration::from_millis(200));
        }
    })
    .await;
    let reply = get(tap.addr, "/slow").await;
    assert_eq!(reply.status, StatusCode::GATEWAY_TIMEOUT);
    let exchange = exchange_for(&lens, "/slow").await;
    assert_eq!(exchange.error.as_ref().unwrap().kind, ErrorKind::Timeout);
}

#[tokio::test]
async fn origins_on_ipv6_localhost_are_found() {
    // Dev servers often listen on ::1 only; `localhost` tries both loopbacks.
    let Ok(listener) = tokio::net::TcpListener::bind("[::1]:0").await else {
        return; // No IPv6 on this machine.
    };
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let service = hyper::service::service_fn(|_| async {
            Ok::<_, std::convert::Infallible>(text_response(200, "v6"))
        });
        let _ = hyper::server::conn::http1::Builder::new()
            .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
            .await;
    });
    let (_lens, tap) = lens_for(&format!("http://localhost:{port}")).await;
    assert_eq!(get(tap.addr, "/").await.text(), "v6");
}

#[tokio::test]
async fn host_header_modes() {
    let origin = echo_origin().await;
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.host_header = HostHeader::Upstream;
    })
    .await;
    let seen = get(tap.addr, "/").await.text();
    assert!(seen.contains(&format!("host: {}\n", origin.addr)), "{seen}");
}

#[tokio::test]
async fn host_routing_on_one_listener() {
    let a = origin(|_| async { text_response(200, "A") }).await;
    let b = origin(|_| async { text_response(200, "B") }).await;
    let lens = Lens::new(LensOptions::default()).unwrap();
    let tap_a = lens
        .add_tap(TapConfig {
            id: Some(TapId::new("site-a").unwrap()),
            ..TapConfig::new(Upstream::origin(&a.url).unwrap())
        })
        .unwrap();
    let tap_b = lens
        .add_tap(TapConfig::new(Upstream::origin(&b.url).unwrap()))
        .unwrap();
    let mut options = ListenOptions::tap(tap_a.clone());
    options.routing = Routing::Hosts {
        hosts: vec![
            ("a.test".into(), tap_a.clone()),
            ("*.b.test".into(), tap_b.clone()),
        ],
        fallback: None,
    };
    let listener = lens.listen(options).await.unwrap();
    let with_host = |host: &'static str| {
        Request::builder()
            .uri("/")
            .header("host", host)
            .body(lens::empty())
            .unwrap()
    };
    assert_eq!(
        fetch(listener.addr, with_host("A.test:443")).await.text(),
        "A"
    );
    assert_eq!(
        fetch(listener.addr, with_host("x.b.test")).await.text(),
        "B"
    );
    let unknown = fetch(listener.addr, with_host("c.test")).await;
    assert_eq!(unknown.status, StatusCode::MISDIRECTED_REQUEST);

    // Removing a tap drops its routes; the listener keeps serving the others.
    lens.remove_tap(&tap_b).await.unwrap();
    assert_eq!(
        fetch(listener.addr, with_host("x.b.test")).await.status,
        StatusCode::MISDIRECTED_REQUEST
    );
    assert_eq!(fetch(listener.addr, with_host("a.test")).await.text(), "A");

    // Non-loopback addresses need an explicit opt-in.
    let mut public = ListenOptions::tap(tap_a);
    public.addr = "0.0.0.0:0".parse().unwrap();
    assert!(matches!(
        lens.listen(public).await,
        Err(lens::LensError::NotLoopback(_))
    ));
}

#[tokio::test]
async fn removing_a_tap_closes_its_listener_and_shutdown_stops_everything() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_for(&origin.url).await;
    assert_eq!(get(tap.addr, "/").await.status, StatusCode::OK);
    assert_eq!(lens.taps().len(), 1);
    lens.remove_tap(&tap.id).await.unwrap();
    assert!(tokio::net::TcpStream::connect(tap.addr).await.is_err());
    assert!(lens.taps().is_empty());
    // Captures survive until cleared.
    assert_eq!(
        lens.list(&Query::default(), &Redaction::masked())
            .items
            .len(),
        1
    );
    lens.clear(None);
    assert!(
        lens.list(&Query::default(), &Redaction::masked())
            .items
            .is_empty()
    );
    lens.shutdown().await;
    assert!(matches!(
        lens.add_tap(TapConfig::new(Upstream::origin(&origin.url).unwrap())),
        Err(lens::LensError::Closed)
    ));
}

#[tokio::test]
async fn subscribers_see_added_updated_completed() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_for(&origin.url).await;
    let mut events = lens.subscribe();
    get(tap.addr, "/events").await;
    let mut changes = Vec::new();
    while changes.last() != Some(&lens::Change::Completed) {
        match tokio::time::timeout(Duration::from_secs(5), events.recv()).await {
            Ok(Ok(LensEvent::Exchange { change, .. })) => changes.push(change),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(changes.first(), Some(&lens::Change::Added));
    assert!(changes.contains(&lens::Change::Updated));
}

#[tokio::test]
async fn wait_for_blocks_until_a_matching_request_arrives() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_for(&origin.url).await;
    let waiter = {
        let lens = lens.clone();
        tokio::spawn(async move {
            lens.wait_for(
                &Filter {
                    methods: vec!["POST".into()],
                    path: Some("/webhooks/".into()),
                    ..Filter::default()
                },
                &WaitOptions {
                    timeout: Duration::from_secs(10),
                    ..WaitOptions::default()
                },
            )
            .await
        })
    };
    tokio::task::yield_now().await;
    get(tap.addr, "/webhooks/ignored-get").await;
    fetch(
        tap.addr,
        request(Method::POST, "/webhooks/stripe")
            .body(body("{}"))
            .unwrap(),
    )
    .await;
    let found = waiter.await.unwrap().unwrap();
    assert_eq!(found.request.path(), "/webhooks/stripe");

    let timeout = lens
        .wait_for(
            &Filter {
                path: Some("/never".into()),
                ..Filter::default()
            },
            &WaitOptions {
                timeout: Duration::from_millis(50),
                ..WaitOptions::default()
            },
        )
        .await;
    assert!(matches!(timeout, Err(lens::LensError::WaitTimeout)));
}

#[tokio::test]
async fn metrics_count_statuses_bytes_and_latency() {
    let origin = origin(|request: Request<hyper::body::Incoming>| async move {
        let status = if request.uri().path() == "/missing" {
            404
        } else {
            200
        };
        text_response(status, "hello")
    })
    .await;
    let (lens, tap) = lens_for(&origin.url).await;
    for path in ["/", "/", "/missing"] {
        get(tap.addr, path).await;
    }
    fetch(
        tap.addr,
        request(Method::POST, "/").body(body("12345")).unwrap(),
    )
    .await;
    // Wait for the last exchange to finish recording.
    next_exchange(
        &lens,
        Filter {
            methods: vec!["POST".into()],
            ..Filter::default()
        },
    )
    .await;
    let m = lens.metrics(&tap.id).unwrap();
    assert_eq!(m.requests, 4);
    assert_eq!(m.status.success, 3);
    assert_eq!(m.status.client_error, 1);
    assert_eq!(m.bytes_in, 5);
    assert_eq!(m.bytes_out, 20);
    assert_eq!(m.latency.count, 4);
    assert!(m.latency.p99_ms.unwrap() >= m.latency.p50_ms.unwrap());
    assert_eq!(m.active_requests, 0);
}

#[tokio::test]
async fn capture_can_be_turned_off_while_metrics_continue() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.capture.enabled = false;
    })
    .await;
    assert_eq!(get(tap.addr, "/quiet").await.status, StatusCode::OK);
    assert!(
        lens.list(&Query::default(), &Redaction::masked())
            .items
            .is_empty()
    );
    assert_eq!(lens.metrics(&tap.id).unwrap().requests, 1);
}

#[tokio::test]
async fn ring_keeps_the_configured_number_of_exchanges() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.capture.capacity = Some(3);
    })
    .await;
    for i in 0..6 {
        get(tap.addr, &format!("/ring/{i}")).await;
    }
    exchange_for(&lens, "/ring/5").await;
    let page = lens.list(&Query::default(), &Redaction::masked());
    let paths: Vec<&str> = page.items.iter().map(|e| e.request.path()).collect();
    assert_eq!(paths, vec!["/ring/5", "/ring/4", "/ring/3"]);
}

#[tokio::test]
async fn upstream_can_change_at_runtime() {
    let first = origin(|_| async { text_response(200, "first") }).await;
    let second = origin(|_| async { text_response(200, "second") }).await;
    let (lens, tap) = lens_for(&first.url).await;
    assert_eq!(get(tap.addr, "/").await.text(), "first");
    lens.update_tap(&tap.id, |config| {
        config.upstream = Upstream::origin(&second.url).unwrap();
    })
    .unwrap();
    assert_eq!(get(tap.addr, "/").await.text(), "second");
    // Invalid changes are refused and leave the tap as it was.
    let refused = lens.update_tap(&tap.id, |config| {
        config.host_header = HostHeader::Custom("bad\nhost".into());
    });
    assert!(refused.is_err());
    assert_eq!(get(tap.addr, "/").await.text(), "second");
}

#[tokio::test]
async fn http2_and_grpc_style_trailers_pass_through() {
    // An h2c origin that answers with trailers, like gRPC.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        loop {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::spawn(async move {
                let service = hyper::service::service_fn(
                    |request: Request<hyper::body::Incoming>| async move {
                        assert_eq!(request.version(), http::Version::HTTP_2);
                        let te = request.headers().get("te").cloned();
                        let body = request.into_body().collect().await.unwrap().to_bytes();
                        let mut trailers = http::HeaderMap::new();
                        trailers.insert("grpc-status", "0".parse().unwrap());
                        let frames: Vec<Result<http_body::Frame<Bytes>, std::convert::Infallible>> = vec![
                            Ok(http_body::Frame::data(body)),
                            Ok(http_body::Frame::trailers(trailers)),
                        ];
                        let body =
                            http_body_util::StreamBody::new(futures_util::stream::iter(frames));
                        let mut response = http::Response::new(body);
                        response
                            .headers_mut()
                            .insert("content-type", "application/grpc".parse().unwrap());
                        if let Some(te) = te {
                            response.headers_mut().insert("x-saw-te", te);
                        }
                        Ok::<_, std::convert::Infallible>(response)
                    },
                );
                let _ =
                    hyper::server::conn::http2::Builder::new(hyper_util::rt::TokioExecutor::new())
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                        .await;
            });
        }
    });
    let (lens, tap) = lens_with(
        Upstream::origin(&format!("http://{addr}")).unwrap(),
        |config| {
            if let Upstream::Origin(origin) = &mut config.upstream {
                origin.http2 = true;
            }
        },
    )
    .await;
    // cloudflared with `http2Origin` speaks h2c to Lens.
    let stream = tokio::net::TcpStream::connect(tap.addr).await.unwrap();
    let (mut sender, conn) = hyper::client::conn::http2::handshake(
        hyper_util::rt::TokioExecutor::new(),
        hyper_util::rt::TokioIo::new(stream),
    )
    .await
    .unwrap();
    tokio::spawn(conn);
    let request = Request::builder()
        .method(Method::POST)
        .uri("http://grpc.test/pkg.Service/Call")
        .header("content-type", "application/grpc")
        .header("te", "trailers")
        .body(body(Bytes::from_static(b"\x00\x00\x00\x00\x02hi")))
        .unwrap();
    let response = sender.send_request(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["x-saw-te"], "trailers");
    let collected = response.into_body().collect().await.unwrap();
    assert_eq!(collected.trailers().unwrap()["grpc-status"], "0");
    assert_eq!(&collected.to_bytes()[..], b"\x00\x00\x00\x00\x02hi");
    let exchange = exchange_for(&lens, "/pkg.Service/Call").await;
    assert_eq!(exchange.request.version, http::Version::HTTP_2);
    assert_eq!(exchange.request.host, "grpc.test");
    assert_eq!(exchange.state, ExchangeState::Complete);
}

#[tokio::test]
async fn malformed_requests_get_errors_not_panics() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let origin = echo_origin().await;
    let (lens, tap) = lens_for(&origin.url).await;
    for raw in [
        &b"GARBAGE\r\n\r\n"[..],
        b"GET / HTTP/1.1\r\nHost: a\r\nContent-Length: nope\r\n\r\n",
        b"GET / HTTP/1.1\r\nBad Header\r\n\r\n",
        b"\x16\x03\x01\x00\xa5\x01\x00\x00\xa1\x03\x03",
    ] {
        let mut stream = tokio::net::TcpStream::connect(tap.addr).await.unwrap();
        stream.write_all(raw).await.unwrap();
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut out)).await;
        if !out.is_empty() {
            assert!(
                out.starts_with(b"HTTP/1.1 4"),
                "{}",
                String::from_utf8_lossy(&out)
            );
        }
    }
    // Oversized heads are refused.
    let mut stream = tokio::net::TcpStream::connect(tap.addr).await.unwrap();
    let big = format!(
        "GET / HTTP/1.1\r\nHost: a\r\nX-Big: {}\r\n\r\n",
        "a".repeat(100 * 1024)
    );
    let _ = stream.write_all(big.as_bytes()).await;
    let mut out = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut out)).await;
    assert!(
        out.is_empty() || out.starts_with(b"HTTP/1.1 431"),
        "{}",
        String::from_utf8_lossy(&out)
    );
    // Overlong targets get 414 and are captured.
    let long = format!("/{}", "a".repeat(20 * 1024));
    assert_eq!(get(tap.addr, &long).await.status, StatusCode::URI_TOO_LONG);
    // Still serving.
    assert_eq!(get(tap.addr, "/fine").await.status, StatusCode::OK);
    drop(lens);
}

#[tokio::test]
async fn text_search_and_filters_through_the_runtime() {
    let origin = echo_origin().await;
    let (lens, tap) = lens_for(&origin.url).await;
    fetch(
        tap.addr,
        request(Method::POST, "/search/a")
            .body(body(r#"{"needle":"Haystack"}"#))
            .unwrap(),
    )
    .await;
    fetch(
        tap.addr,
        request(Method::POST, "/search/b")
            .body(body(r#"{"password":"Haystack"}"#))
            .unwrap(),
    )
    .await;
    exchange_for(&lens, "/search/b").await;
    let query = Query {
        filter: Filter {
            text: Some("haystack".into()),
            ..Filter::default()
        },
        ..Query::default()
    };
    // The second one's match is a masked secret, so only a revealed search finds it.
    let masked = lens.list(&query, &Redaction::masked());
    assert_eq!(masked.items.len(), 1);
    assert_eq!(masked.items[0].request.path(), "/search/a");
    assert_eq!(lens.list(&query, &Redaction::revealed()).items.len(), 2);
}
