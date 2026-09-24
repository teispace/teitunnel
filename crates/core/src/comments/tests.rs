use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr},
    time::{Duration, Instant},
};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, StatusCode, Uri};
use http_body_util::BodyExt;
use lens::{ReservedHandler, ReservedRequest, TapId};
use serde_json::{Value, json};

use super::{
    remote::{self, DATABASE_NAME},
    serve::{CommentsHandler, Limiter, WRITES_PER_MINUTE, same_origin_json},
    *,
};
use crate::{
    engine::{
        CloudApi,
        fake::{CloudState, FakeCloud},
    },
    store::Store,
};

fn anchor() -> Anchor {
    Anchor {
        selector: "main > h1:nth-of-type(1)".into(),
        x: 0.25,
        y: 1.5,
        left: 120.0,
        top: -4.0,
        vw: 1440,
        vh: 900,
    }
}

#[test]
fn text_is_checked_and_kept_as_typed() {
    assert_eq!(
        clean_body("  <b>hi</b>\r\nthere  ").unwrap(),
        "<b>hi</b>\nthere"
    );
    assert!(matches!(clean_body("   "), Err(CommentsError::InvalidBody)));
    assert!(matches!(
        clean_body(&"x".repeat(MAX_BODY + 1)),
        Err(CommentsError::InvalidBody)
    ));
    assert!(clean_body(&"é".repeat(MAX_BODY)).is_ok());
    assert!(matches!(
        clean_body("a\u{0007}b"),
        Err(CommentsError::InvalidBody)
    ));
    // Right-to-left overrides can disguise text; refused.
    assert!(matches!(
        clean_body("abc\u{202e}fed"),
        Err(CommentsError::InvalidBody)
    ));
    assert_eq!(clean_name(" Ana ").unwrap(), "Ana");
    assert!(clean_name("a\nb").is_err());
    assert!(clean_name("").is_err());
    assert!(clean_name(&"n".repeat(MAX_NAME + 1)).is_err());
}

#[test]
fn paths_and_anchors_are_bounded() {
    assert_eq!(clean_path("/pricing?x=1#top").unwrap(), "/pricing");
    for bad in ["pricing", "//evil.com", "/a\\b", "/\u{0000}"] {
        assert!(clean_path(bad).is_err(), "{bad}");
    }
    assert!(clean_path(&format!("/{}", "a".repeat(MAX_PATH))).is_err());
    let cleaned = clean_anchor(&anchor()).unwrap();
    assert!((cleaned.y - 1.0).abs() < f64::EPSILON);
    assert!(cleaned.top.abs() < f64::EPSILON);
    let mut bad = anchor();
    bad.x = f64::NAN;
    assert!(clean_anchor(&bad).is_err());
    bad = anchor();
    bad.selector = "a".repeat(MAX_SELECTOR + 1);
    assert!(clean_anchor(&bad).is_err());
}

#[test]
fn rows_group_into_threads_in_order() {
    let row = |id: &str, thread: &str, at: u64| Row {
        id: id.into(),
        thread: thread.into(),
        path: "/".into(),
        anchor: None,
        author: "A".into(),
        email: Some("a@x.com".into()),
        verified: true,
        by_owner: false,
        body: id.into(),
        created_at: at,
        resolved_at: None,
        resolved_by: None,
    };
    let threads = threads_from(vec![
        row("r2", "t1", 30),
        row("t2", "t2", 20),
        row("orphan", "gone", 5),
        row("t1", "t1", 10),
        row("r1", "t1", 25),
    ]);
    assert_eq!(threads.len(), 2);
    assert_eq!(threads[0].id, "t1");
    let bodies: Vec<_> = threads[0]
        .comments
        .iter()
        .map(|c| c.body.as_str())
        .collect();
    assert_eq!(bodies, ["t1", "r1", "r2"]);
    assert_eq!(threads[0].latest(), 30);
    assert!(
        threads[0]
            .clone()
            .public()
            .comments
            .iter()
            .all(|c| c.email.is_none())
    );
}

#[test]
fn excerpts_are_one_short_line() {
    assert_eq!(excerpt("short"), "short");
    assert_eq!(excerpt("line one\nline two"), "line one…");
    assert_eq!(excerpt(&"x".repeat(200)).chars().count(), 121);
}

#[test]
fn the_limiter_allows_a_burst_then_refuses() {
    let limiter = Limiter::default();
    let ip = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7));
    let other = IpAddr::V4(Ipv4Addr::new(203, 0, 113, 8));
    let start = Instant::now();
    for _ in 0..WRITES_PER_MINUTE {
        assert!(limiter.allow(ip, start));
    }
    assert!(!limiter.allow(ip, start));
    assert!(limiter.allow(other, start));
    assert!(limiter.allow(ip, start + Duration::from_secs(61)));
}

#[test]
fn only_same_origin_json_may_write() {
    let headers = |pairs: &[(&'static str, &'static str)]| {
        let mut map = HeaderMap::new();
        for (k, v) in pairs {
            map.insert(*k, HeaderValue::from_static(v));
        }
        map
    };
    assert!(same_origin_json(&headers(&[
        ("content-type", "application/json"),
        ("sec-fetch-site", "same-origin")
    ])));
    assert!(!same_origin_json(&headers(&[
        ("content-type", "application/json"),
        ("sec-fetch-site", "cross-site")
    ])));
    assert!(!same_origin_json(&headers(&[
        ("content-type", "text/plain"),
        ("sec-fetch-site", "same-origin")
    ])));
    assert!(same_origin_json(&headers(&[
        ("content-type", "application/json; charset=utf-8"),
        ("origin", "https://app.example.com"),
        ("host", "app.example.com")
    ])));
    assert!(!same_origin_json(&headers(&[
        ("content-type", "application/json"),
        ("origin", "https://evil.example"),
        ("host", "app.example.com")
    ])));
}

async fn comments() -> Comments {
    Comments::new(Store::open_in_memory().unwrap())
}

#[tokio::test]
async fn live_share_comments_are_kept_here() {
    let comments = comments().await;
    let mut events = comments.subscribe();
    let subject = Subject::route("acc", "App.Example.com");
    assert_eq!(subject.key, "route:acc:app.example.com");
    let ana = Author::reviewer("Ana").unwrap();
    let thread = comments
        .local_start(&subject, "/pricing", Some(&anchor()), "Typo here", &ana)
        .await
        .unwrap();
    assert_eq!(thread.path, "/pricing");
    assert_eq!(thread.anchor.as_ref().unwrap().vw, 1440);
    match events.recv().await.unwrap() {
        CommentsEvent::New {
            author, excerpt, ..
        } => {
            assert_eq!(author, "Ana");
            assert_eq!(excerpt, "Typo here");
        }
        other => panic!("{other:?}"),
    }
    let owner = Author::owner("Krishna");
    let replied = comments
        .local_reply(&subject, &thread.id, "Fixed", &owner)
        .await
        .unwrap();
    assert_eq!(replied.comments.len(), 2);
    assert!(replied.comments[1].by_owner);
    assert!(matches!(
        events.recv().await.unwrap(),
        CommentsEvent::Changed { .. }
    ));
    let resolved = comments
        .local_resolve(&subject, &thread.id, true, "Ana")
        .await
        .unwrap();
    assert!(resolved.resolved);
    assert_eq!(resolved.resolved_by.as_deref(), Some("Ana"));
    assert!(matches!(
        comments
            .local_reply(&subject, "nope", "x", &ana)
            .await
            .unwrap_err(),
        CommentsError::NotFound
    ));
    // Other pages and subjects don't see it.
    assert!(
        comments
            .local_threads(&subject.key, Some("/other"))
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        comments
            .local_threads("route:acc:x.example.com", None)
            .await
            .unwrap()
            .is_empty()
    );
    let subjects = comments.subjects().await.unwrap();
    assert_eq!(subjects.len(), 1);
    assert_eq!(subjects[0].comments, 2);
    assert_eq!(subjects[0].open, 0);
    assert_eq!(subjects[0].unread, 1);
    comments.mark_seen(&subject.key).await.unwrap();
    assert_eq!(comments.subjects().await.unwrap()[0].unread, 0);
    let reopened = comments
        .local_resolve(&subject, &thread.id, false, "")
        .await
        .unwrap();
    assert!(!reopened.resolved && reopened.resolved_by.is_none());
    comments.forget(&subject.key).await.unwrap();
    assert!(comments.subjects().await.unwrap().is_empty());
}

fn request(
    method: Method,
    uri: &str,
    body: &str,
    headers: &[(&'static str, &str)],
) -> ReservedRequest {
    let mut map = HeaderMap::new();
    for (k, v) in headers {
        map.insert(*k, HeaderValue::from_str(v).unwrap());
    }
    ReservedRequest {
        tap: TapId::new("share1").unwrap(),
        method,
        uri: uri.parse::<Uri>().unwrap(),
        headers: map,
        body: Bytes::from(body.to_owned()),
        client_ip: IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)),
    }
}

async fn call(
    handler: &CommentsHandler,
    request: ReservedRequest,
) -> (StatusCode, HeaderMap, Value) {
    let response = handler.handle(request).await;
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = serde_json::from_slice(&bytes)
        .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into_owned()));
    (status, headers, value)
}

const JSON: [(&str, &str); 2] = [
    ("content-type", "application/json"),
    ("sec-fetch-site", "same-origin"),
];

#[tokio::test]
async fn the_share_answers_the_overlay_and_its_api() {
    let comments = comments().await;
    let handler = CommentsHandler::new(
        comments.clone(),
        Subject::quick_share("share1", Some("https://a-b.trycloudflare.com")),
        false,
    );
    let (status, headers, body) = call(
        &handler,
        request(Method::GET, "/__teitunnel/comments/overlay.js", "", &[]),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert!(
        headers["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/javascript")
    );
    assert!(body.as_str().unwrap().contains("attachShadow"));

    let (status, _, thread) = call(
        &handler,
        request(
            Method::POST,
            "/__teitunnel/comments/api/threads",
            &json!({"path": "/", "anchor": anchor(), "body": "<img src=x onerror=alert(1)>", "author": "Bo"}).to_string(),
            &JSON,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{thread}");
    // Stored as typed; the overlay renders it as text.
    assert_eq!(
        thread["comments"][0]["body"],
        "<img src=x onerror=alert(1)>"
    );
    let id = thread["id"].as_str().unwrap().to_owned();

    // A visitor can't claim an Access identity on a share without a login.
    let (_, _, reply) = call(
        &handler,
        request(
            Method::POST,
            &format!("/__teitunnel/comments/api/threads/{id}/replies"),
            &json!({"body": "me too", "author": "Cy"}).to_string(),
            &[
                JSON[0],
                JSON[1],
                ("cf-access-authenticated-user-email", "boss@example.com"),
            ],
        ),
    )
    .await;
    assert_eq!(reply["comments"][1]["author"], "Cy");
    assert_eq!(reply["comments"][1]["verified"], false);

    let (status, _, list) = call(
        &handler,
        request(
            Method::GET,
            "/__teitunnel/comments/api/threads?path=%2F",
            "",
            &[],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["threads"].as_array().unwrap().len(), 1);
    assert_eq!(list["me"]["verified"], false);

    let (status, _, resolved) = call(
        &handler,
        request(
            Method::POST,
            &format!("/__teitunnel/comments/api/threads/{id}/resolve"),
            &json!({"resolved": true, "author": "Bo"}).to_string(),
            &JSON,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resolved["resolved"], true);

    // Cross-site and non-JSON writes are refused before anything is read.
    let (status, _, _) = call(
        &handler,
        request(
            Method::POST,
            "/__teitunnel/comments/api/threads",
            "path=/&body=x",
            &[
                ("content-type", "application/x-www-form-urlencoded"),
                ("sec-fetch-site", "cross-site"),
            ],
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, _, error) = call(
        &handler,
        request(
            Method::POST,
            "/__teitunnel/comments/api/threads",
            &json!({"path": "/", "body": "no name"}).to_string(),
            &JSON,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(error["error"].as_str().unwrap().contains("name"));
    let (status, _, _) = call(
        &handler,
        request(Method::GET, "/__teitunnel/comments/../etc", "", &[]),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_login_vouches_for_reviewers() {
    let handler = CommentsHandler::new(
        comments().await,
        Subject::route("acc", "app.example.com"),
        true,
    );
    let (_, _, list) = call(
        &handler,
        request(
            Method::GET,
            "/__teitunnel/comments/api/threads",
            "",
            &[("cf-access-authenticated-user-email", "ana@example.com")],
        ),
    )
    .await;
    assert_eq!(list["me"]["verified"], true);
    let (_, _, thread) = call(
        &handler,
        request(
            Method::POST,
            "/__teitunnel/comments/api/threads",
            &json!({"path": "/", "body": "Signed in"}).to_string(),
            &[
                JSON[0],
                JSON[1],
                ("cf-access-authenticated-user-email", "ana@example.com"),
            ],
        ),
    )
    .await;
    assert_eq!(thread["comments"][0]["author"], "ana@example.com");
    assert_eq!(thread["comments"][0]["verified"], true);
    // Reviewers never see addresses; the owner does.
    assert!(thread["comments"][0].get("email").is_none());
}

#[tokio::test]
async fn visitors_are_rate_limited() {
    let handler = CommentsHandler::new(comments().await, Subject::quick_share("s", None), false);
    let body = json!({"path": "/", "body": "x", "author": "Spam"}).to_string();
    for _ in 0..WRITES_PER_MINUTE {
        let (status, _, _) = call(
            &handler,
            request(
                Method::POST,
                "/__teitunnel/comments/api/threads",
                &body,
                &JSON,
            ),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
    }
    let (status, _, _) = call(
        &handler,
        request(
            Method::POST,
            "/__teitunnel/comments/api/threads",
            &body,
            &JSON,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn snapshot_comments_are_read_and_answered_on_cloudflare() {
    let cloud = FakeCloud::new(CloudState::default());
    let db = cloud
        .create_d1_database("acc", DATABASE_NAME)
        .await
        .unwrap()
        .uuid;
    // The table is created on first use (a database made by the inbox has none).
    assert!(
        remote::threads(&cloud, "acc", &db, "teitunnel-demo")
            .await
            .unwrap()
            .is_empty()
    );
    // What the Worker writes for a reviewer.
    cloud
        .d1_query(
            "acc",
            &db,
            &[cf_api::D1Statement::new(
                "INSERT INTO teitunnel_comments (id, site, thread, path, anchor, author, body, created_at) VALUES ('t1', 'teitunnel-demo', 't1', '/', NULL, 'Ana', 'Hello', 1000), ('t2', 'teitunnel-other', 't2', '/', NULL, 'Bo', 'Other', 2000)",
                vec![],
            )],
        )
        .await
        .unwrap();
    let threads = remote::threads(&cloud, "acc", &db, "teitunnel-demo")
        .await
        .unwrap();
    assert_eq!(threads.len(), 1);
    let replied = remote::reply(
        &cloud,
        "acc",
        &db,
        "teitunnel-demo",
        "t1",
        "Thanks!",
        &Author::owner("Krishna"),
    )
    .await
    .unwrap();
    assert_eq!(replied.comments.len(), 2);
    assert!(replied.comments[1].by_owner);
    let resolved = remote::resolve(&cloud, "acc", &db, "teitunnel-demo", "t1", true, "Krishna")
        .await
        .unwrap();
    assert!(resolved.resolved);
    assert!(matches!(
        remote::resolve(&cloud, "acc", &db, "teitunnel-demo", "nope", true, "")
            .await
            .unwrap_err(),
        CommentsError::NotFound
    ));
    let counts = remote::counts(
        &cloud,
        "acc",
        &db,
        &BTreeMap::from([
            ("teitunnel-demo".to_owned(), 0),
            ("teitunnel-other".to_owned(), 1500),
        ]),
    )
    .await
    .unwrap();
    assert_eq!(counts["teitunnel-demo"].comments, 2);
    assert_eq!(counts["teitunnel-demo"].open, 0);
    assert_eq!(counts["teitunnel-demo"].unread, 1);
    assert_eq!(counts["teitunnel-demo"].reviewer_latest_at, 1000);
    assert_eq!(counts["teitunnel-other"].unread, 1);
    remote::delete_site(&cloud, "acc", &db, "teitunnel-demo")
        .await
        .unwrap();
    assert!(
        remote::threads(&cloud, "acc", &db, "teitunnel-demo")
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn snapshot_notifications_fire_once_per_new_comment() {
    let comments = comments().await;
    let subject = Subject::snapshot("s1", "acc", "Demo", "https://demo.example.com");
    comments.register(&subject).await.unwrap();
    let counts = |latest: u64, reviewer: u64| remote::RemoteCounts {
        latest_at: latest,
        reviewer_latest_at: reviewer,
        comments: 1,
        open: 1,
        unread: 1,
    };
    assert!(
        comments
            .record_remote(&subject.key, counts(10, 10))
            .await
            .unwrap()
    );
    assert!(
        !comments
            .record_remote(&subject.key, counts(10, 10))
            .await
            .unwrap()
    );
    // The owner's own reply isn't news.
    assert!(
        !comments
            .record_remote(&subject.key, counts(20, 10))
            .await
            .unwrap()
    );
    assert!(
        comments
            .record_remote(&subject.key, counts(30, 30))
            .await
            .unwrap()
    );
    let view = &comments.subjects().await.unwrap()[0];
    assert_eq!((view.comments, view.open, view.unread), (1, 1, 1));
    assert_eq!(view.latest_at, Some(30));
}

#[test]
fn the_worker_creates_the_same_table() {
    let worker = crate::engine::sites::WORKER_JS;
    for sql in remote::COMMENTS_SCHEMA {
        assert!(worker.contains(sql), "the Snapshot Worker lacks: {sql}");
    }
}

/// An origin answering every request with a small HTML page.
async fn html_origin() -> String {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
                    if !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        continue;
                    }
                    let body = "<html><body><h1>Hi</h1></body></html>";
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\n\r\n{body}",
                        body.len()
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

#[tokio::test(flavor = "multi_thread")]
async fn a_live_share_serves_the_overlay_and_keeps_its_comments() {
    use crate::inspect::{Inspector, TapScope, TapSpec};
    let store = Store::open_in_memory().unwrap();
    let inspector = Inspector::new(Some(store), None, "app");
    let origin = html_origin().await;
    let tap = inspector
        .start(TapSpec::new(
            TapScope::QuickShare {
                share_id: "share1".into(),
            },
            "share",
            &origin,
        ))
        .await
        .unwrap();
    let http = reqwest::Client::builder().no_proxy().build().unwrap();
    let page = |http: &reqwest::Client| {
        http.get(format!("{}/", tap.address))
            .header("accept", "text/html")
            .header("sec-fetch-dest", "document")
            .send()
    };
    let before = page(&http).await.unwrap().text().await.unwrap();
    assert!(!before.contains("overlay.js"), "{before}");

    let view = inspector.set_comments(&tap.id, true, false).await.unwrap();
    assert!(view.comments);
    let after = page(&http).await.unwrap().text().await.unwrap();
    assert!(after.contains(SNIPPET), "{after}");
    let script = http
        .get(format!("{}{OVERLAY_PATH}", tap.address))
        .send()
        .await
        .unwrap();
    assert_eq!(script.status(), 200);

    let posted = http
        .post(format!("{}/__teitunnel/comments/api/threads", tap.address))
        .header("content-type", "application/json")
        .header("sec-fetch-site", "same-origin")
        .body(json!({"path": "/", "body": "Looks good", "author": "Ana"}).to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(posted.status(), 200);
    let comments = inspector.comments().unwrap();
    let subjects = comments.subjects().await.unwrap();
    assert_eq!(subjects[0].subject.key, "share:share1");
    assert_eq!(subjects[0].comments, 1);

    // Off again: the page is untouched and the API isn't there.
    inspector.set_comments(&tap.id, false, false).await.unwrap();
    assert!(
        !page(&http)
            .await
            .unwrap()
            .text()
            .await
            .unwrap()
            .contains("overlay.js")
    );
    let gone = http
        .get(format!("{}/__teitunnel/comments/api/threads", tap.address))
        .send()
        .await
        .unwrap();
    assert_eq!(gone.status(), 404);
    inspector.stop(&tap.id).await;
}
