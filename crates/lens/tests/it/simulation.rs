//! Network simulation, faults, SSE keep-alive, WebSocket frames, and the bearer gate.

use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use bytes::Bytes;
use http::{Method, Request, Response, StatusCode};
use http_body::Frame;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use lens::{
    BearerToken, ErrorKind, ExchangeState, FaultAction, FaultRule, FrameOpcode, Latency, Lens,
    LensOptions, NetworkConfig, PathPattern, RandomSource, Responder, TapConfig, TapHandle,
    Upstream,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::mpsc,
};

use crate::support::*;

/// Returns the given numbers in order, then repeats the last.
#[derive(Debug)]
struct Sequence(Mutex<Vec<f64>>);

impl RandomSource for Sequence {
    fn next_u64(&self) -> u64 {
        0
    }
    fn next_f64(&self) -> f64 {
        let mut values = self.0.lock().unwrap();
        if values.len() > 1 {
            values.remove(0)
        } else {
            values[0]
        }
    }
}

async fn lens_with_random(
    origin_url: &str,
    random: Vec<f64>,
    configure: impl FnOnce(&mut TapConfig),
) -> (Lens, TapHandle) {
    let lens = Lens::new(LensOptions {
        random: Some(Arc::new(Sequence(Mutex::new(random)))),
        ..LensOptions::default()
    })
    .unwrap();
    let mut config = TapConfig::new(Upstream::origin(origin_url).unwrap());
    configure(&mut config);
    let tap = lens.start_tap(config).await.unwrap();
    (lens, tap)
}

fn fault(percent: f64, action: FaultAction) -> FaultRule {
    FaultRule {
        method: None,
        path: PathPattern::parse("/api/*").unwrap(),
        percent,
        action,
    }
}

#[tokio::test]
async fn status_faults_hit_the_configured_share() {
    let origin = origin(|_| async { text_response(200, "real") }).await;
    let (lens, tap) = lens_with_random(&origin.url, vec![0.1, 0.9, 0.3, 0.7], |config| {
        config.faults = vec![fault(
            50.0,
            FaultAction::Status {
                status: 503,
                retry_after_secs: Some(7),
            },
        )];
    })
    .await;
    let mut statuses = Vec::new();
    for i in 0..4 {
        let reply = get(tap.addr, &format!("/api/{i}")).await;
        if reply.status == StatusCode::SERVICE_UNAVAILABLE {
            assert_eq!(reply.headers["retry-after"], "7");
        }
        statuses.push(reply.status.as_u16());
    }
    assert_eq!(statuses, vec![503, 200, 503, 200]);
    assert_eq!(
        get(tap.addr, "/other").await.text(),
        "real",
        "unmatched paths pass"
    );
    let faulted = exchange_for(&lens, "/api/0").await;
    assert_eq!(faulted.responder, Responder::Fault { rule: 0 });
    assert_eq!(faulted.fault.as_ref().unwrap().rule, 0);
    assert!(exchange_for(&lens, "/api/1").await.fault.is_none());
}

#[tokio::test]
async fn reset_faults_close_the_connection() {
    let origin = origin(|_| async { text_response(200, "real") }).await;
    let (lens, tap) = lens_with_random(&origin.url, vec![0.0], |config| {
        config.faults = vec![fault(100.0, FaultAction::Reset)];
    })
    .await;
    let stream = tokio::net::TcpStream::connect(tap.addr).await.unwrap();
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(conn);
    let result = sender
        .send_request(
            request(Method::GET, "/api/reset")
                .body(lens::empty())
                .unwrap(),
        )
        .await;
    assert!(result.is_err(), "no response, just a closed connection");
    let exchange = exchange_for(&lens, "/api/reset").await;
    assert_eq!(exchange.state, ExchangeState::Failed);
    assert_eq!(
        exchange.error.as_ref().unwrap().kind,
        ErrorKind::ConnectionReset
    );
    assert_eq!(exchange.fault.as_ref().unwrap().action, FaultAction::Reset);
}

#[tokio::test]
async fn delay_and_timeout_faults() {
    let origin = origin(|_| async { text_response(200, "real") }).await;
    let (lens, tap) = lens_with_random(&origin.url, vec![0.0], |config| {
        config.faults = vec![
            FaultRule {
                path: PathPattern::parse("/api/slow").unwrap(),
                ..fault(100.0, FaultAction::Delay { ms: 200 })
            },
            fault(100.0, FaultAction::Timeout { after_ms: 100 }),
        ];
    })
    .await;
    let start = Instant::now();
    let slow = get(tap.addr, "/api/slow").await;
    assert_eq!(slow.text(), "real");
    assert!(start.elapsed() >= Duration::from_millis(200));
    let delayed = exchange_for(&lens, "/api/slow").await;
    assert_eq!(delayed.responder, Responder::Upstream);
    assert!(delayed.timings.first_byte_us.unwrap() >= 200_000);

    let timed_out = get(tap.addr, "/api/other").await;
    assert_eq!(timed_out.status, StatusCode::GATEWAY_TIMEOUT);
    let exchange = exchange_for(&lens, "/api/other").await;
    assert_eq!(exchange.error.as_ref().unwrap().kind, ErrorKind::Timeout);
    assert_eq!(exchange.responder, Responder::Fault { rule: 1 });
}

#[tokio::test]
async fn latency_and_bandwidth_limits() {
    const SIZE: usize = 100 * 1024;
    let origin = origin(|request: Request<Incoming>| async move {
        if request.method() == Method::POST {
            let body = request.into_body().collect().await.unwrap().to_bytes();
            text_response(200, body.len().to_string())
        } else {
            text_response(200, vec![b'x'; SIZE])
        }
    })
    .await;
    let (_lens, tap) = lens_with_random(&origin.url, vec![0.5], |config| {
        config.network = NetworkConfig {
            latency: Some(Latency {
                base_ms: 150,
                jitter_ms: 50,
            }),
            up_bytes_per_sec: Some(50 * 1024),
            down_bytes_per_sec: Some(100 * 1024),
        };
    })
    .await;
    let start = Instant::now();
    let reply = get(tap.addr, "/download").await;
    assert_eq!(reply.body.len(), SIZE);
    // 150 ms latency (jitter centred by the 0.5 draw) plus ~84 kB beyond the burst at
    // 100 kB/s.
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(900), "{elapsed:?}");

    let start = Instant::now();
    let reply = fetch(
        tap.addr,
        request(Method::POST, "/upload")
            .body(body(vec![b'y'; 50 * 1024]))
            .unwrap(),
    )
    .await;
    assert_eq!(reply.text(), (50 * 1024).to_string());
    let elapsed = start.elapsed();
    assert!(elapsed >= Duration::from_millis(700), "{elapsed:?}");
}

#[tokio::test]
async fn sse_keepalive_fills_idle_gaps_between_events_only() {
    let origin = origin(|_| async {
        let (tx, rx) = mpsc::channel::<&'static [u8]>(4);
        tokio::spawn(async move {
            tx.send(b"data: a\n\n").await.unwrap();
            tokio::time::sleep(Duration::from_millis(700)).await;
            tx.send(b"data: b").await.unwrap();
            tokio::time::sleep(Duration::from_millis(700)).await;
            tx.send(b"\n\n").await.unwrap();
        });
        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| {
                (
                    Ok::<_, lens::BoxError>(Frame::data(Bytes::from_static(chunk))),
                    rx,
                )
            })
        });
        let mut response = Response::new(StreamBody::new(stream).boxed_unsync());
        response
            .headers_mut()
            .insert("content-type", "text/event-stream".parse().unwrap());
        response
    })
    .await;
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.sse_keepalive = Some(Duration::from_millis(200));
    })
    .await;
    let reply = get(tap.addr, "/stream").await;
    let text = reply.text();
    assert!(text.starts_with("data: a\n\n: keep-alive\n\n"), "{text:?}");
    assert!(
        text.ends_with("data: b\n\n"),
        "no heartbeat inside an event: {text:?}"
    );
    let heartbeats = text.matches(": keep-alive").count();
    assert!(
        (2..=4).contains(&heartbeats),
        "{heartbeats} heartbeats in {text:?}"
    );
    // The capture holds what the origin sent, without heartbeats.
    let exchange = exchange_for(&lens, "/stream").await;
    let captured = &exchange.response.as_ref().unwrap().body.data;
    assert_eq!(&captured[..], b"data: a\n\ndata: b\n\n");
    assert_eq!(exchange.stream.as_ref().unwrap().server.count, 2);

    // Off switch.
    let (_lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.sse_keepalive = None;
    })
    .await;
    assert_eq!(
        get(tap.addr, "/stream").await.text(),
        "data: a\n\ndata: b\n\n"
    );
}

fn ws_frame(opcode: u8, rsv1: bool, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = vec![
        0x80 | (u8::from(rsv1) << 6) | opcode,
        0x80 | u8::try_from(payload.len()).unwrap(),
    ];
    out.extend_from_slice(&mask);
    out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

#[tokio::test]
async fn websocket_frames_are_captured_and_deflate_is_inflated() {
    // An origin that accepts permessage-deflate and echoes raw bytes.
    let origin = origin(|mut request: Request<Incoming>| async move {
        let upgrade = hyper::upgrade::on(&mut request);
        tokio::spawn(async move {
            let mut io = TokioIo::new(upgrade.await.unwrap());
            let mut buf = [0u8; 1024];
            loop {
                let n = io.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                io.write_all(&buf[..n]).await.unwrap();
            }
        });
        Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header("upgrade", "websocket")
            .header("connection", "upgrade")
            .header("sec-websocket-accept", "unused-by-this-test")
            .header(
                "sec-websocket-extensions",
                "permessage-deflate; client_no_context_takeover",
            )
            .body(lens::empty())
            .unwrap()
    })
    .await;
    let (lens, tap) = lens_for(&origin.url).await;
    let mut stream = tokio::net::TcpStream::connect(tap.addr).await.unwrap();
    stream
        .write_all(b"GET /ws HTTP/1.1\r\nHost: app.test\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Extensions: permessage-deflate\r\n\r\n")
        .await
        .unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }
    assert!(head.starts_with(b"HTTP/1.1 101"));

    let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
    let mut compressed = Vec::with_capacity(128);
    compressor
        .compress_vec(
            b"hello deflate",
            &mut compressed,
            flate2::FlushCompress::Sync,
        )
        .unwrap();
    compressed.truncate(compressed.len() - 4);
    let mut bytes = ws_frame(0x1, true, &compressed, [1, 2, 3, 4]);
    bytes.extend(ws_frame(0x1, false, b"plain text", [9, 8, 7, 6]));
    let mut close = 1000u16.to_be_bytes().to_vec();
    close.extend_from_slice(b"done");
    bytes.extend(ws_frame(0x8, false, &close, [0, 0, 0, 0]));
    stream.write_all(&bytes).await.unwrap();
    let mut echoed = vec![0u8; bytes.len()];
    stream.read_exact(&mut echoed).await.unwrap();
    assert_eq!(echoed, bytes, "bytes pass through unchanged");
    drop(stream);

    let exchange = exchange_for(&lens, "/ws").await;
    let stream = exchange.stream.as_ref().unwrap();
    let client_frames: Vec<_> = stream
        .frames
        .iter()
        .filter(|f| f.direction == lens::Direction::ClientToServer)
        .collect();
    assert_eq!(client_frames.len(), 3);
    assert!(client_frames[0].compressed && client_frames[0].masked);
    assert_eq!(client_frames[0].preview, None);
    assert_eq!(client_frames[1].preview.as_deref(), Some("plain text"));
    assert_eq!(client_frames[2].opcode, FrameOpcode::Close);
    assert_eq!(client_frames[2].close_code, Some(1000));
    assert_eq!(client_frames[2].close_reason.as_deref(), Some("done"));
    let inflated = &stream.previews[0];
    assert!(inflated.compressed && inflated.inflated);
    assert_eq!(inflated.preview, "hello deflate");
    // The echo came back through the server direction too.
    assert_eq!(stream.frames.len(), 6);
}

#[tokio::test]
async fn bearer_gate_protects_apis_and_is_stripped() {
    let origin = origin(|request: Request<Incoming>| async move {
        let auth = request
            .headers()
            .get("authorization")
            .map_or("-", |v| v.to_str().unwrap_or("?"))
            .to_owned();
        text_response(200, auth)
    })
    .await;
    let token = BearerToken::generate().unwrap();
    let secret = token.token().expose().clone();
    let (lens, tap) = lens_with(Upstream::origin(&origin.url).unwrap(), |config| {
        config.gates.bearer = vec![BearerToken::new("second-token-for-ci-000").unwrap(), token];
        config.gates.bypass = vec![PathPattern::parse("/health").unwrap()];
    })
    .await;
    let denied = get(tap.addr, "/v1/chat/completions").await;
    assert_eq!(denied.status, StatusCode::UNAUTHORIZED);
    assert_eq!(
        denied.headers["www-authenticate"],
        "Bearer realm=\"Teitunnel\""
    );
    assert!(denied.text().contains("\"unauthorized\""));
    let wrong = fetch(
        tap.addr,
        request(Method::GET, "/v1/models")
            .header("authorization", "Bearer not-the-right-token-at-all")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(wrong.status, StatusCode::UNAUTHORIZED);
    let allowed = fetch(
        tap.addr,
        request(Method::GET, "/v1/models")
            .header("authorization", format!("Bearer {secret}"))
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(allowed.status, StatusCode::OK);
    assert_eq!(allowed.text(), "-", "the gate's token isn't forwarded");
    assert_eq!(get(tap.addr, "/health").await.status, StatusCode::OK);
    let exchange = exchange_for(&lens, "/v1/chat/completions").await;
    assert_eq!(
        exchange.responder,
        Responder::Gate {
            reason: lens::GateOutcome::BearerRequired
        }
    );
}
