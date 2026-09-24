//! Streaming: large bodies, SSE, WebSocket and other upgrades never buffer.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering::SeqCst},
    },
    time::Duration,
};

use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use http::{Method, Request, Response, StatusCode};
use http_body::Frame;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use lens::{ExchangeKind, ExchangeState, LensBody, MessageKind};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::{mpsc, oneshot},
};
use tokio_tungstenite::tungstenite::{Message, protocol::Role};

use crate::support::*;

const MB: usize = 1024 * 1024;

/// A body fed from a channel.
fn channel_body(capacity: usize) -> (mpsc::Sender<Bytes>, LensBody) {
    let (tx, rx) = mpsc::channel::<Bytes>(capacity);
    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv()
            .await
            .map(|chunk| (Ok::<_, lens::BoxError>(Frame::data(chunk)), rx))
    });
    (tx, StreamBody::new(stream).boxed_unsync())
}

#[tokio::test]
async fn large_upload_streams_to_the_origin_as_it_arrives() {
    const TOTAL: usize = 200 * MB;
    let (first_seen_tx, first_seen_rx) = oneshot::channel::<()>();
    let first_seen = Arc::new(std::sync::Mutex::new(Some(first_seen_tx)));
    let origin = origin(move |request: Request<Incoming>| {
        let first_seen = Arc::clone(&first_seen);
        async move {
            let mut body = request.into_body();
            let mut total = 0usize;
            while let Some(frame) = body.frame().await {
                if let Ok(data) = frame.unwrap().into_data() {
                    total += data.len();
                    if total >= MB
                        && let Some(tx) = first_seen.lock().unwrap().take()
                    {
                        let _ = tx.send(());
                    }
                }
            }
            text_response(200, total.to_string())
        }
    })
    .await;
    let (lens, tap) = lens_for(&origin.url).await;
    let (tx, upload) = channel_body(4);
    let request = request(Method::POST, "/upload")
        .header("content-type", "application/octet-stream")
        .body(upload)
        .unwrap();
    let response = tokio::spawn(fetch(tap.addr, request));
    let chunk = Bytes::from(vec![7u8; 256 * 1024]);
    // Send the first megabyte, then wait until the origin has it: impossible if Lens
    // buffered the body.
    for _ in 0..4 {
        tx.send(chunk.clone()).await.unwrap();
    }
    tokio::time::timeout(Duration::from_secs(20), first_seen_rx)
        .await
        .expect("the origin should see the upload before it ends")
        .unwrap();
    for _ in 4..(TOTAL / chunk.len()) {
        tx.send(chunk.clone()).await.unwrap();
    }
    drop(tx);
    let reply = response.await.unwrap();
    assert_eq!(reply.text(), TOTAL.to_string());
    let exchange = exchange_for(&lens, "/upload").await;
    let body = &exchange.request.body;
    assert_eq!(body.size, TOTAL as u64);
    assert_eq!(body.data.len(), lens::DEFAULT_MAX_BODY_BYTES);
    assert!(body.truncated && body.complete);
    assert_eq!(lens.metrics(&tap.id).unwrap().bytes_in, TOTAL as u64);
}

#[tokio::test]
async fn large_download_streams_with_bounded_capture() {
    const TOTAL: usize = 200 * MB;
    let origin = origin(|_| async {
        let chunk = Bytes::from(vec![1u8; 64 * 1024]);
        let stream = futures_util::stream::iter(
            (0..TOTAL / chunk.len())
                .map(move |_| Ok::<_, lens::BoxError>(Frame::data(chunk.clone()))),
        );
        Response::new(StreamBody::new(stream).boxed_unsync())
    })
    .await;
    let (lens, tap) = lens_for(&origin.url).await;
    let response = send(
        tap.addr,
        request(Method::GET, "/download")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let mut total = 0usize;
    while let Some(frame) = body.frame().await {
        if let Ok(data) = frame.unwrap().into_data() {
            total += data.len();
        }
    }
    assert_eq!(total, TOTAL);
    let exchange = exchange_for(&lens, "/download").await;
    let captured = &exchange.response.as_ref().unwrap().body;
    assert_eq!(captured.size, TOTAL as u64);
    assert_eq!(captured.data.len(), lens::DEFAULT_MAX_BODY_BYTES);
    assert!(captured.truncated && captured.complete);
}

#[tokio::test]
async fn slow_readers_apply_backpressure_to_the_origin() {
    // The origin produces as fast as it may; the client reads one chunk and stalls.
    // With backpressure, the origin's progress stays within a few socket buffers.
    let produced = Arc::new(AtomicU64::new(0));
    let origin = {
        let produced = Arc::clone(&produced);
        origin(move |_| {
            let produced = Arc::clone(&produced);
            async move {
                let chunk = Bytes::from(vec![0u8; 64 * 1024]);
                let stream = futures_util::stream::repeat(chunk)
                    .take(100_000)
                    .map(move |chunk| {
                        produced.fetch_add(chunk.len() as u64, SeqCst);
                        Ok::<_, lens::BoxError>(Frame::data(chunk))
                    });
                Response::new(StreamBody::new(stream).boxed_unsync())
            }
        })
        .await
    };
    let (_lens, tap) = lens_for(&origin.url).await;
    let response = send(
        tap.addr,
        request(Method::GET, "/firehose")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    let mut body = response.into_body();
    let _ = body.frame().await;
    // Wait until the origin stops making progress.
    let mut last = 0;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(40)).await;
        let now = produced.load(SeqCst);
        if now == last {
            break;
        }
        last = now;
    }
    let stalled_at = produced.load(SeqCst);
    assert!(
        stalled_at < 64 * MB as u64,
        "the origin produced {stalled_at} bytes while the client read one chunk"
    );
    drop(body);
}

#[tokio::test]
async fn server_sent_events_arrive_as_they_are_sent() {
    let (release_tx, release_rx) = oneshot::channel::<()>();
    let release = Arc::new(std::sync::Mutex::new(Some(release_rx)));
    let origin = origin(move |_| {
        let release = Arc::clone(&release);
        async move {
            let (tx, rx) = mpsc::channel::<Bytes>(4);
            let waiter = release.lock().unwrap().take();
            tokio::spawn(async move {
                tx.send(Bytes::from_static(b"event: tick\ndata: one\n\n"))
                    .await
                    .unwrap();
                if let Some(waiter) = waiter {
                    let _ = waiter.await;
                }
                tx.send(Bytes::from_static(b": keep-alive\n\ndata: two\n\n"))
                    .await
                    .unwrap();
            });
            let stream = futures_util::stream::unfold(rx, |mut rx| async move {
                rx.recv()
                    .await
                    .map(|chunk| (Ok::<_, lens::BoxError>(Frame::data(chunk)), rx))
            });
            let mut response = Response::new(StreamBody::new(stream).boxed_unsync());
            response
                .headers_mut()
                .insert("content-type", "text/event-stream".parse().unwrap());
            response
        }
    })
    .await;
    let (lens, tap) = lens_for(&origin.url).await;
    let response = send(
        tap.addr,
        request(Method::GET, "/events").body(lens::empty()).unwrap(),
    )
    .await;
    let mut body = response.into_body();
    // The first event arrives while the origin is still holding the second.
    let first = tokio::time::timeout(Duration::from_secs(10), body.frame())
        .await
        .expect("the first event should arrive before the stream ends")
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert_eq!(&first[..], b"event: tick\ndata: one\n\n");
    release_tx.send(()).unwrap();
    let rest = body.collect().await.unwrap().to_bytes();
    assert_eq!(&rest[..], b": keep-alive\n\ndata: two\n\n");

    let exchange = exchange_for(&lens, "/events").await;
    assert_eq!(exchange.kind, ExchangeKind::Sse);
    let stream = exchange.stream.as_ref().unwrap();
    assert_eq!(stream.server.count, 2);
    assert!(stream.closed);
    assert_eq!(stream.previews[0].kind, MessageKind::Event);
    assert_eq!(stream.previews[0].preview, "event: tick\ndata: one");
    assert_eq!(stream.previews[1].preview, "data: two");
}

/// A WebSocket echo origin (upgrades with hyper, then speaks tungstenite).
async fn websocket_origin() -> Origin {
    origin(|mut request: Request<Incoming>| async move {
        let key = request.headers()["sec-websocket-key"].clone();
        let upgrade = hyper::upgrade::on(&mut request);
        tokio::spawn(async move {
            let upgraded = upgrade.await.unwrap();
            let mut ws = tokio_tungstenite::WebSocketStream::from_raw_socket(
                TokioIo::new(upgraded),
                Role::Server,
                None,
            )
            .await;
            while let Some(Ok(message)) = ws.next().await {
                match message {
                    Message::Text(_) | Message::Binary(_) => ws.send(message).await.unwrap(),
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });
        let accept = tokio_tungstenite::tungstenite::handshake::derive_accept_key(key.as_bytes());
        Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header("upgrade", "websocket")
            .header("connection", "Upgrade")
            .header("sec-websocket-accept", accept)
            .body(lens::empty())
            .unwrap()
    })
    .await
}

#[tokio::test]
async fn websocket_echo_through_lens() {
    let origin = websocket_origin().await;
    let (lens, tap) = lens_for(&origin.url).await;
    let url = format!("ws://{}/socket", tap.addr);
    let (mut ws, response) = tokio_tungstenite::connect_async(url).await.unwrap();
    assert_eq!(response.status(), StatusCode::SWITCHING_PROTOCOLS);
    ws.send(Message::text("hello lens")).await.unwrap();
    assert_eq!(
        ws.next().await.unwrap().unwrap(),
        Message::text("hello lens")
    );
    ws.send(Message::binary(vec![1u8, 2, 3])).await.unwrap();
    assert_eq!(
        ws.next().await.unwrap().unwrap(),
        Message::binary(vec![1u8, 2, 3])
    );
    let big = "z".repeat(100_000);
    ws.send(Message::text(big.clone())).await.unwrap();
    assert_eq!(ws.next().await.unwrap().unwrap(), Message::text(big));
    ws.close(None).await.unwrap();
    while ws.next().await.is_some() {}

    let exchange = exchange_for(&lens, "/socket").await;
    assert_eq!(exchange.kind, ExchangeKind::WebSocket);
    assert_eq!(exchange.state, ExchangeState::Complete);
    assert_eq!(exchange.status(), Some(StatusCode::SWITCHING_PROTOCOLS));
    let stream = exchange.stream.as_ref().unwrap();
    assert!(stream.closed);
    assert!(stream.client.count >= 3, "{stream:?}");
    assert!(stream.server.count >= 3, "{stream:?}");
    let first = &stream.previews[0];
    assert_eq!(
        (first.kind, first.preview.as_str()),
        (MessageKind::Text, "hello lens")
    );
    assert!(
        stream
            .previews
            .iter()
            .any(|p| p.kind == MessageKind::Binary && p.preview == "010203")
    );
    let big_preview = stream.previews.iter().find(|p| p.size == 100_000).unwrap();
    assert!(big_preview.truncated && big_preview.preview.len() == 1024);
    assert_eq!(lens.metrics(&tap.id).unwrap().active_streams, 0);
}

#[tokio::test]
async fn any_upgrade_including_post_is_tunnelled() {
    // A custom protocol upgraded from a POST (as some tools do); the origin echoes bytes
    // in upper case after switching.
    let origin = origin(|mut request: Request<Incoming>| async move {
        assert_eq!(request.method(), Method::POST);
        let upgrade = hyper::upgrade::on(&mut request);
        tokio::spawn(async move {
            let mut io = TokioIo::new(upgrade.await.unwrap());
            let mut buf = [0u8; 64];
            loop {
                let n = io.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    break;
                }
                let upper: Vec<u8> = buf[..n].to_ascii_uppercase();
                io.write_all(&upper).await.unwrap();
            }
        });
        Response::builder()
            .status(StatusCode::SWITCHING_PROTOCOLS)
            .header("upgrade", "shout/1")
            .header("connection", "upgrade")
            .body(lens::empty())
            .unwrap()
    })
    .await;
    let (lens, tap) = lens_for(&origin.url).await;
    let mut stream = tokio::net::TcpStream::connect(tap.addr).await.unwrap();
    stream
        .write_all(b"POST /shout HTTP/1.1\r\nHost: app.test\r\nConnection: Upgrade\r\nUpgrade: shout/1\r\nContent-Length: 0\r\n\r\n")
        .await
        .unwrap();
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.unwrap();
        head.push(byte[0]);
    }
    let head = String::from_utf8(head).unwrap().to_ascii_lowercase();
    assert!(head.starts_with("http/1.1 101"), "{head}");
    assert!(head.contains("upgrade: shout/1"));
    stream.write_all(b"quiet words").await.unwrap();
    let mut out = [0u8; 11];
    stream.read_exact(&mut out).await.unwrap();
    assert_eq!(&out, b"QUIET WORDS");
    drop(stream);
    let exchange = exchange_for(&lens, "/shout").await;
    assert_eq!(exchange.kind, ExchangeKind::Upgrade);
    assert!(exchange.is_finished());
}

#[tokio::test]
async fn chunked_and_long_poll_responses() {
    let origin = origin(|_| async {
        tokio::time::sleep(Duration::from_millis(300)).await;
        let stream = futures_util::stream::iter(
            ["a", "b", "c"]
                .map(|s| Ok::<_, lens::BoxError>(Frame::data(Bytes::from_static(s.as_bytes())))),
        );
        Response::new(StreamBody::new(stream).boxed_unsync())
    })
    .await;
    let (lens, tap) = lens_for(&origin.url).await;
    let reply = get(tap.addr, "/poll").await;
    assert_eq!(reply.text(), "abc");
    let exchange = exchange_for(&lens, "/poll").await;
    assert!(exchange.timings.first_byte_us.unwrap() >= 300_000);
    assert_eq!(&exchange.response.as_ref().unwrap().body.data[..], b"abc");
}

#[tokio::test]
async fn client_abort_mid_download_is_recorded() {
    let origin = origin(|_| async {
        let stream = futures_util::stream::repeat(Bytes::from(vec![0u8; 64 * 1024]))
            .map(|chunk| Ok::<_, lens::BoxError>(Frame::data(chunk)));
        Response::new(StreamBody::new(stream).boxed_unsync())
    })
    .await;
    let (lens, tap) = lens_for(&origin.url).await;
    let response = send(
        tap.addr,
        request(Method::GET, "/endless")
            .body(lens::empty())
            .unwrap(),
    )
    .await;
    let mut body = response.into_body();
    let _ = body.frame().await;
    drop(body);
    let exchange = exchange_for(&lens, "/endless").await;
    assert_eq!(exchange.state, ExchangeState::Failed);
    assert_eq!(
        exchange.error.as_ref().unwrap().kind,
        lens::ErrorKind::ClientAborted
    );
}
