//! Breakpoints: requests and answers stop, can be changed, answered or dropped, and
//! always go on in the end.

use std::{
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::SeqCst},
    },
    time::Duration,
};

use bytes::Bytes;
use http::{Method, Request, StatusCode};
use http_body_util::{BodyExt, StreamBody};
use hyper_util::rt::TokioIo;
use lens::{
    BodyLock, BreakEdit, BreakStage, BreakpointRule, ExchangeState, Lens, LensBody, PathPattern,
    Paused, Responder, Resume, TapConfig, Upstream,
};
use tokio::net::TcpStream;

use crate::support::*;

fn rule(method: Option<&str>, path: &str, request: bool, response: bool) -> BreakpointRule {
    BreakpointRule {
        method: method.map(str::to_owned),
        path: PathPattern::parse(path).unwrap(),
        request,
        response,
    }
}

/// An origin that answers with what it got: `METHOD target body`.
async fn echo() -> Origin {
    origin(|request: Request<hyper::body::Incoming>| async move {
        let line = format!(
            "{} {} x-a={} ",
            request.method(),
            request.uri(),
            request
                .headers()
                .get("x-a")
                .map_or("", |v| v.to_str().unwrap())
        );
        let body = request.into_body().collect().await.unwrap().to_bytes();
        text_response(200, [line.as_bytes(), &body].concat())
    })
    .await
}

async fn lens_stopping(url: &str, rules: Vec<BreakpointRule>) -> (Lens, lens::TapHandle) {
    lens_with(Upstream::origin(url).unwrap(), |config: &mut TapConfig| {
        config.breakpoints = rules;
    })
    .await
}

/// Waits until one exchange is paused, and returns it.
async fn paused(lens: &Lens) -> Paused {
    for _ in 0..500 {
        if let Some(first) = lens.paused(None).into_iter().next() {
            return first;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("nothing paused");
}

fn in_background(addr: SocketAddr, request: Request<LensBody>) -> tokio::task::JoinHandle<Reply> {
    tokio::spawn(fetch(addr, request))
}

#[tokio::test]
async fn a_paused_request_goes_on_changed() {
    let origin = echo().await;
    let (lens, tap) =
        lens_stopping(&origin.url, vec![rule(Some("post"), "/hooks", true, false)]).await;
    let reply = in_background(
        tap.addr,
        request(Method::POST, "/hooks")
            .header("x-a", "1")
            .header("content-type", "application/json")
            .body(body(r#"{"n":1}"#))
            .unwrap(),
    );
    let stopped = paused(&lens).await;
    assert_eq!(stopped.stage, BreakStage::Request);
    assert_eq!(
        (stopped.method.as_str(), stopped.target.as_str()),
        ("POST", "/hooks")
    );
    assert_eq!(stopped.body.as_deref(), Some(r#"{"n":1}"#));
    assert!(stopped.headers.contains(&("x-a".into(), "1".into())));
    assert!(stopped.resumes_at_ms > stopped.since_ms);
    // It shows as waiting while it waits.
    let waiting = lens.get(stopped.exchange).unwrap();
    assert_eq!(
        waiting.breakpoint.as_ref().unwrap().waiting,
        Some(BreakStage::Request)
    );

    // A change that doesn't fit is refused, and it keeps waiting.
    let wrong = Resume::Edited {
        edit: BreakEdit {
            status: Some(500),
            ..BreakEdit::default()
        },
    };
    assert!(lens.resume(stopped.exchange, wrong).is_err());
    assert_eq!(lens.paused(None).len(), 1);

    let mut headers = stopped.headers.clone();
    headers.retain(|(name, _)| name != "x-a");
    headers.push(("x-a".into(), "2".into()));
    lens.resume(
        stopped.exchange,
        Resume::Edited {
            edit: BreakEdit {
                method: Some("PUT".into()),
                target: Some("/hooks?retry=1".into()),
                headers: Some(headers),
                body: Some(r#"{"n":2,"more":true}"#.into()),
                ..BreakEdit::default()
            },
        },
    )
    .unwrap();
    let reply = reply.await.unwrap();
    assert_eq!(
        reply.text(),
        r#"PUT /hooks?retry=1 x-a=2 {"n":2,"more":true}"#
    );

    let exchange = exchange_for(&lens, "/hooks").await;
    let mark = exchange.breakpoint.clone().unwrap();
    assert!(mark.request_edited && !mark.response_edited && !mark.timed_out);
    assert_eq!(mark.waiting, None);
    assert_eq!(exchange.request.method, Method::PUT);
    assert_eq!(&exchange.request.body.data[..], br#"{"n":2,"more":true}"#);
    assert!(lens.paused(None).is_empty());
}

#[tokio::test]
async fn a_paused_answer_goes_back_changed() {
    let origin = origin(|_| async { text_response(200, "hello") }).await;
    let (lens, tap) = lens_stopping(&origin.url, vec![rule(None, "/page", false, true)]).await;
    let reply = in_background(
        tap.addr,
        request(Method::GET, "/page").body(lens::empty()).unwrap(),
    );
    let stopped = paused(&lens).await;
    assert_eq!(stopped.stage, BreakStage::Response);
    assert_eq!(stopped.status, Some(200));
    assert_eq!(stopped.body.as_deref(), Some("hello"));

    lens.resume(
        stopped.exchange,
        Resume::Edited {
            edit: BreakEdit {
                status: Some(503),
                body: Some("down for a moment".into()),
                ..BreakEdit::default()
            },
        },
    )
    .unwrap();
    let reply = reply.await.unwrap();
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(reply.text(), "down for a moment");
    assert_eq!(reply.headers["content-length"], "17");

    let exchange = exchange_for(&lens, "/page").await;
    assert!(exchange.breakpoint.as_ref().unwrap().response_edited);
    assert_eq!(exchange.status(), Some(StatusCode::SERVICE_UNAVAILABLE));
    assert_eq!(
        &exchange.response.as_ref().unwrap().body.data[..],
        b"down for a moment"
    );
}

#[tokio::test]
async fn a_paused_request_can_be_answered_without_the_service() {
    let hits = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&hits);
    let origin = origin(move |_| {
        counted.fetch_add(1, SeqCst);
        async { text_response(200, "service") }
    })
    .await;
    let (lens, tap) = lens_stopping(&origin.url, vec![rule(None, "/*", true, true)]).await;
    let reply = in_background(
        tap.addr,
        request(Method::GET, "/api").body(lens::empty()).unwrap(),
    );
    let stopped = paused(&lens).await;
    lens.resume(
        stopped.exchange,
        Resume::Answer {
            status: 201,
            headers: vec![("content-type".into(), "application/json".into())],
            body: r#"{"made":true}"#.into(),
        },
    )
    .unwrap();
    let reply = reply.await.unwrap();
    assert_eq!(reply.status, StatusCode::CREATED);
    assert_eq!(reply.text(), r#"{"made":true}"#);
    assert_eq!(hits.load(SeqCst), 0, "the service never saw it");
    let exchange = exchange_for(&lens, "/api").await;
    assert_eq!(exchange.responder, Responder::Breakpoint);
    // Answered at the request stage: it doesn't stop again on the way back.
    assert!(lens.paused(None).is_empty());
}

#[tokio::test]
async fn an_aborted_request_gets_no_answer() {
    let origin = echo().await;
    let (lens, tap) = lens_stopping(&origin.url, vec![rule(None, "/drop", true, false)]).await;
    let outcome = tokio::spawn(async move {
        let stream = TcpStream::connect(tap.addr).await.unwrap();
        let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .unwrap();
        tokio::spawn(conn);
        sender
            .send_request(request(Method::GET, "/drop").body(lens::empty()).unwrap())
            .await
            .is_err()
    });
    let stopped = paused(&lens).await;
    lens.resume(stopped.exchange, Resume::Abort).unwrap();
    assert!(outcome.await.unwrap(), "the connection was dropped");
    let exchange = exchange_for(&lens, "/drop").await;
    assert_eq!(exchange.state, ExchangeState::Failed);
}

#[tokio::test]
async fn a_body_still_arriving_goes_on_untouched_and_other_paths_never_stop() {
    let origin = echo().await;
    let (lens, tap) = lens_stopping(&origin.url, vec![rule(None, "/upload", true, false)]).await;

    assert_eq!(get(tap.addr, "/elsewhere").await.status, StatusCode::OK);
    assert!(lens.paused(None).is_empty());

    // Chunked: its size isn't known, so it can't be changed, only let go.
    let chunks = futures_util::stream::iter(["part one, ", "part two"].map(|chunk| {
        Ok::<_, std::convert::Infallible>(http_body::Frame::data(Bytes::from(chunk)))
    }));
    let streamed: LensBody = StreamBody::new(chunks)
        .map_err(|never| match never {})
        .boxed_unsync();
    let reply = in_background(
        tap.addr,
        request(Method::POST, "/upload").body(streamed).unwrap(),
    );
    let stopped = paused(&lens).await;
    assert_eq!(stopped.body, None);
    assert_eq!(stopped.body_locked, Some(BodyLock::Streamed));
    let change_body = Resume::Edited {
        edit: BreakEdit {
            body: Some("x".into()),
            ..BreakEdit::default()
        },
    };
    assert!(lens.resume(stopped.exchange, change_body).is_err());

    // Taking the breakpoints away lets it go.
    lens.update_tap(&tap.id, |config| config.breakpoints.clear())
        .unwrap();
    let reply = reply.await.unwrap();
    assert_eq!(reply.text(), "POST /upload x-a= part one, part two");
    assert!(lens.paused(None).is_empty());
}
