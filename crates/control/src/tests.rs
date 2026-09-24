//! Round trips over a real socket (named pipe on Windows) in a temporary folder.

use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::{
    Action, ClientError, ControlClient, Decision, Endpoint, Limits, Requester,
    deeplink::{DeepLink, Handled, LinkHandler},
    protocol::{
        AgentApproval, AgentInfo, ApplyParams, ClientInfo, Event, HostHeader, PauseShare,
        PreviewParams, StartShare, View, code,
    },
    testing::FakeHost,
};

struct Running {
    inner: crate::testing::Running,
    _dir: tempfile::TempDir,
}

impl std::ops::Deref for Running {
    type Target = crate::testing::Running;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

async fn start(limits: Limits) -> Running {
    let dir = tempfile::tempdir().unwrap();
    let inner = crate::testing::serve(dir.path(), limits).await.unwrap();
    Running { inner, _dir: dir }
}

fn cli() -> ClientInfo {
    ClientInfo {
        name: "teitunnel-cli".into(),
        version: "0.1.1".into(),
    }
}

/// A raw connection for sending hand-written lines.
struct Raw {
    reader: BufReader<tokio::io::ReadHalf<crate::endpoint::Connection>>,
    writer: tokio::io::WriteHalf<crate::endpoint::Connection>,
}

impl Raw {
    async fn open(endpoint: &Endpoint) -> Self {
        let (reader, writer) = tokio::io::split(endpoint.connect().await.unwrap());
        Self {
            reader: BufReader::new(reader),
            writer,
        }
    }

    async fn send(&mut self, line: &str) {
        self.writer.write_all(line.as_bytes()).await.unwrap();
        self.writer.write_all(b"\n").await.unwrap();
    }

    /// The next message, or `None` when the server closed the connection.
    async fn next(&mut self) -> Option<Value> {
        let mut line = String::new();
        let read = tokio::time::timeout(Duration::from_secs(5), self.reader.read_line(&mut line))
            .await
            .expect("the server answers");
        match read {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(serde_json::from_str(&line).unwrap()),
        }
    }

    async fn hello(&mut self, token: &str) -> Value {
        let hello = json!({"jsonrpc": "2.0", "id": 1, "method": "hello", "params": {
            "protocol": 1, "token": token, "client": {"name": "raw", "version": "1"}}});
        self.send(&hello.to_string()).await;
        self.next().await.unwrap()
    }
}

fn error_code(response: &Value) -> Option<i64> {
    response.pointer("/error/code").and_then(Value::as_i64)
}

#[tokio::test]
async fn local_domains_list_and_reload_without_asking() {
    let running = start(Limits::default()).await;
    let client = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap();
    for method in ["localDomains.list", "localDomains.reload"] {
        assert!(
            client.hello().methods.iter().any(|m| m == method),
            "{method}"
        );
    }
    let list = client.local_domains().await.unwrap();
    assert!(list.running);
    assert_eq!(list.domains[0].url, "https://shop.test");
    let reloaded = client.reload_local_domains().await.unwrap();
    assert_eq!(reloaded, list);
    assert_eq!(*running.host.reloads.lock().unwrap(), 1);
    assert_eq!(
        running.host.asked(),
        0,
        "reading and reloading need no approval"
    );
}

#[tokio::test]
async fn hello_then_reads() {
    let running = start(Limits::default()).await;
    let client = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap();
    assert_eq!(client.hello().app.version, "9.9.9");
    assert_eq!(client.hello().protocol, 1);
    assert!(!client.hello().approved);
    assert!(client.hello().methods.iter().any(|m| m == "shares.start"));
    let status = client.status().await.unwrap();
    assert_eq!(status.accounts[0].name, "Personal");
    assert_eq!(client.shares().await.unwrap()[0].id, "qs-1");
    assert_eq!(client.routes(None).await.unwrap().account.id, "a1");
    let plan = client
        .preview(&PreviewParams {
            account: None,
            tunnel: None,
            change: json!({"type": "removeRoute", "hostname": "a.example.com"}),
        })
        .await
        .unwrap();
    assert_eq!(plan.fingerprint, "f1");
    client.open(&View::Doctor).await.unwrap();
    assert_eq!(running.host.opened.lock().unwrap()[0], View::Doctor);
    assert!(client.doctor().await.unwrap().is_empty());
    // Reads never ask the person.
    assert_eq!(running.host.asked(), 0);
}

#[tokio::test]
async fn nothing_is_answered_without_hello() {
    let running = start(Limits::default()).await;
    let mut raw = Raw::open(&running.endpoint).await;
    raw.send(r#"{"jsonrpc":"2.0","id":1,"method":"status"}"#)
        .await;
    let response = raw.next().await.unwrap();
    assert_eq!(error_code(&response), Some(code::UNAUTHORIZED));
    assert_eq!(raw.next().await, None, "the connection is closed");
}

#[tokio::test]
async fn a_wrong_token_is_refused() {
    let running = start(Limits::default()).await;
    let mut raw = Raw::open(&running.endpoint).await;
    let response = raw.hello(&"0".repeat(64)).await;
    assert_eq!(error_code(&response), Some(code::UNAUTHORIZED));
    assert_eq!(raw.next().await, None);

    // The client reports it the same way.
    let dir = tempfile::tempdir().unwrap();
    let other = Endpoint::new(dir.path());
    other.ensure_token().unwrap();
    let connection = running.endpoint.connect().await.unwrap();
    let refused = ControlClient::handshake(connection, other.read_token().unwrap().as_str(), cli())
        .await
        .unwrap_err();
    assert_eq!(refused.code(), Some(code::UNAUTHORIZED));
}

#[tokio::test]
async fn an_other_protocol_version_is_refused_with_the_supported_ones() {
    let running = start(Limits::default()).await;
    let token = running.endpoint.read_token().unwrap();
    let mut raw = Raw::open(&running.endpoint).await;
    let hello = json!({"jsonrpc": "2.0", "id": 1, "method": "hello", "params": {
        "protocol": 2, "token": token.as_str(), "client": {"name": "raw", "version": "1"}}});
    raw.send(&hello.to_string()).await;
    let response = raw.next().await.unwrap();
    assert_eq!(error_code(&response), Some(code::UNSUPPORTED_PROTOCOL));
    assert_eq!(response.pointer("/error/data/supported"), Some(&json!([1])));
}

#[tokio::test]
async fn a_silent_connection_is_closed() {
    let running = start(Limits {
        hello_timeout: Duration::from_millis(100),
        ..Limits::default()
    })
    .await;
    let mut raw = Raw::open(&running.endpoint).await;
    let response = raw.next().await.unwrap();
    assert_eq!(error_code(&response), Some(code::UNAUTHORIZED));
    assert_eq!(raw.next().await, None);
}

#[tokio::test]
async fn an_oversized_message_is_refused() {
    let running = start(Limits {
        max_message: 1024,
        ..Limits::default()
    })
    .await;
    let token = running.endpoint.read_token().unwrap();
    let mut raw = Raw::open(&running.endpoint).await;
    let response = raw.hello(token.as_str()).await;
    assert!(response.get("result").is_some(), "{response}");
    let huge = format!(
        r#"{{"jsonrpc":"2.0","id":2,"method":"status","params":{{"x":"{}"}}}}"#,
        "a".repeat(4096)
    );
    raw.send(&huge).await;
    let response = raw.next().await.unwrap();
    assert_eq!(error_code(&response), Some(code::TOO_LARGE));
    assert_eq!(raw.next().await, None);
}

#[tokio::test]
async fn bad_messages_get_json_rpc_errors() {
    let running = start(Limits::default()).await;
    let token = running.endpoint.read_token().unwrap();
    let mut raw = Raw::open(&running.endpoint).await;
    raw.hello(token.as_str()).await;
    raw.send("not json").await;
    assert_eq!(
        error_code(&raw.next().await.unwrap()),
        Some(code::PARSE_ERROR)
    );
    raw.send(r#"{"jsonrpc":"1.0","id":3,"method":"status"}"#)
        .await;
    let response = raw.next().await.unwrap();
    assert_eq!(error_code(&response), Some(code::INVALID_REQUEST));
    assert_eq!(response["id"], 3);
    raw.send(r#"{"jsonrpc":"2.0","id":4,"method":"routes.delete"}"#)
        .await;
    assert_eq!(
        error_code(&raw.next().await.unwrap()),
        Some(code::METHOD_NOT_FOUND)
    );
    raw.send(r#"{"jsonrpc":"2.0","id":5,"method":"shares.start","params":{"port":1}}"#)
        .await;
    assert_eq!(
        error_code(&raw.next().await.unwrap()),
        Some(code::INVALID_PARAMS)
    );
    // Still usable afterwards.
    raw.send(r#"{"jsonrpc":"2.0","id":6,"method":"status"}"#)
        .await;
    assert!(raw.next().await.unwrap().get("result").is_some());
}

#[tokio::test]
async fn changes_need_the_persons_approval() {
    let running = start(Limits::default()).await;
    let client = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap();
    let request = StartShare {
        origin: "3000".into(),
        stop_after_seconds: None,
        host_header: HostHeader::Auto,
    };

    // No: nothing happens.
    running.host.answer(Decision::Deny);
    let declined = client.start_share(&request).await.unwrap_err();
    assert_eq!(declined.code(), Some(code::DECLINED));
    assert!(running.host.started.lock().unwrap().is_empty());
    {
        let asked = running.host.asked.lock().unwrap();
        assert_eq!(asked[0].requester, Requester::Client(cli()));
        assert!(matches!(asked[0].action, Action::StartShare(_)));
        assert!(asked[0].offer_always);
    }

    // Once: done, but asked again next time.
    running.host.answer(Decision::Once);
    assert_eq!(client.start_share(&request).await.unwrap().id, "qs-2");
    running.host.answer(Decision::Always);
    client.stop_share("qs-1").await.unwrap();
    assert_eq!(running.host.asked(), 3);

    // Always: not asked any more (until revoked).
    client.start_share(&request).await.unwrap();
    assert_eq!(running.host.asked(), 3);
    let again = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap();
    assert!(again.hello().approved);

    // …except for DNS records Teitunnel didn't create.
    running.host.answer(Decision::Once);
    let apply = ApplyParams {
        account: None,
        tunnel: None,
        change: json!({"type": "removeRoute", "hostname": "a.example.com"}),
        fingerprint: "f1".into(),
        confirmed: true,
    };
    client.apply(&apply).await.unwrap();
    assert_eq!(running.host.asked(), 4);
    assert!(!running.host.asked.lock().unwrap()[3].offer_always);
    // An unconfirmed apply of an approved client goes straight through.
    client
        .apply(&ApplyParams {
            confirmed: false,
            ..apply
        })
        .await
        .unwrap();
    assert_eq!(running.host.asked(), 4);
}

#[tokio::test]
async fn pausing_asks_the_person() {
    let running = start(Limits::default()).await;
    let client = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap();
    let request = PauseShare {
        id: "demo.example.com".into(),
        account: None,
    };
    running.host.answer(Decision::Deny);
    let declined = client.pause_share(&request, true).await.unwrap_err();
    assert_eq!(declined.code(), Some(code::DECLINED));
    assert!(running.host.paused.lock().unwrap().is_empty());
    running.host.answer(Decision::Once);
    client.pause_share(&request, true).await.unwrap();
    running.host.answer(Decision::Once);
    client.pause_share(&request, false).await.unwrap();
    assert_eq!(
        *running.host.paused.lock().unwrap(),
        [
            ("demo.example.com".to_owned(), true),
            ("demo.example.com".to_owned(), false)
        ]
    );
    let asked = running.host.asked.lock().unwrap();
    assert!(matches!(asked[1].action, Action::PauseShare(_)));
    assert!(matches!(asked[2].action, Action::ResumeShare(_)));
}

#[tokio::test]
async fn agents_are_listed_while_connected_and_approvals_are_asked_in_the_app() {
    let running = start(Limits::default()).await;
    let mcp = ClientInfo {
        name: "teitunnel-mcp".into(),
        version: "0.2.0".into(),
    };
    let client = ControlClient::connect(&running.endpoint, mcp)
        .await
        .unwrap();
    let agent = AgentInfo {
        name: "claude-code".into(),
        version: Some("2.1".into()),
        mode: "ask".into(),
    };
    client.register_agent(&agent).await.unwrap();
    assert_eq!(running.host.agents.lock().unwrap()[0].1, agent);
    let bad = AgentInfo {
        name: "a\nb".into(),
        ..agent.clone()
    };
    assert_eq!(
        client.register_agent(&bad).await.unwrap_err().code(),
        Some(code::INVALID_PARAMS)
    );

    let question = AgentApproval {
        agent: "claude-code".into(),
        title: "Add app.example.com".into(),
        details: "1. Create DNS record".into(),
    };
    running.host.agent_answers.lock().unwrap().push_back(true);
    assert!(client.approve_for_agent(&question).await.unwrap());
    assert!(
        !client.approve_for_agent(&question).await.unwrap(),
        "no answer is a no"
    );
    assert_eq!(running.host.agent_questions.lock().unwrap().len(), 2);
    // Approving an agent's change isn't a way around the program's own approval.
    assert_eq!(running.host.asked(), 0);

    drop(client);
    for _ in 0..100 {
        if running.host.agents.lock().unwrap().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        running.host.agents.lock().unwrap().is_empty(),
        "gone with the connection"
    );
}

#[tokio::test]
async fn subscriptions_deliver_events() {
    let running = start(Limits::default()).await;
    let client = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap();
    let mut events = client.subscribe(Some(&["sharesChanged"])).await.unwrap();
    running.host.emit(Event::RoutesChanged { account_id: None });
    running.host.emit(Event::SharesChanged {
        id: Some("qs-1".into()),
    });
    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        event,
        Event::SharesChanged {
            id: Some("qs-1".into())
        },
        "only the subscribed type"
    );
    let unknown = client.subscribe(Some(&["everything"])).await.unwrap_err();
    assert_eq!(unknown.code(), Some(code::INVALID_PARAMS));
}

#[tokio::test]
async fn requests_are_rate_limited() {
    // No refill, so the test doesn't depend on the clock (the bucket has its own test).
    let running = start(Limits {
        burst: 4,
        per_second: 0,
        mutations_per_minute: 1,
        ..Limits::default()
    })
    .await;
    let client = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap();
    for _ in 0..2 {
        client.status().await.unwrap();
    }
    running.host.answer(Decision::Once);
    client.stop_share("qs-1").await.unwrap();
    let limited = client.stop_share("qs-1").await.unwrap_err();
    assert_eq!(
        limited.code(),
        Some(code::RATE_LIMITED),
        "one change a minute"
    );
    let limited = client.status().await.unwrap_err();
    assert_eq!(limited.code(), Some(code::RATE_LIMITED), "burst used up");
}

#[tokio::test]
async fn too_many_connections_are_refused() {
    let running = start(Limits {
        max_connections: 1,
        ..Limits::default()
    })
    .await;
    let _first = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap();
    let second = ControlClient::connect(&running.endpoint, cli())
        .await
        .unwrap_err();
    assert_eq!(second.code(), Some(code::RATE_LIMITED));
}

#[tokio::test]
async fn no_app_means_not_running() {
    let dir = tempfile::tempdir().unwrap();
    let endpoint = Endpoint::new(dir.path());
    assert!(matches!(
        ControlClient::connect(&endpoint, cli()).await,
        Err(ClientError::NotRunning)
    ));
    // A token from an earlier run, but no app.
    endpoint.ensure_token().unwrap();
    assert!(matches!(
        ControlClient::connect(&endpoint, cli()).await,
        Err(ClientError::NotRunning)
    ));
}

#[tokio::test]
async fn links_ask_before_sharing_and_only_navigate_otherwise() {
    let host = FakeHost::new();
    let links = LinkHandler::default();
    // Declined: nothing shared.
    let handled = links
        .handle(
            host.as_ref(),
            DeepLink::parse("teitunnel://share?port=3000").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(handled, Handled::Declined);
    assert!(host.started.lock().unwrap().is_empty());
    {
        let asked = host.asked.lock().unwrap();
        assert_eq!(asked[0].requester, Requester::Link);
        assert!(!asked[0].offer_always, "a link is never allowed for good");
    }
    host.answer(Decision::Once);
    let handled = links
        .handle(host.as_ref(), DeepLink::Share { port: 3000 })
        .await
        .unwrap();
    assert!(matches!(handled, Handled::Shared(ref s) if s.id == "qs-2"));
    assert_eq!(host.started.lock().unwrap()[0].origin, "3000");
    assert_eq!(
        host.opened.lock().unwrap()[0],
        View::Share {
            id: Some("qs-2".into())
        }
    );

    let asked = host.asked();
    links
        .handle(
            host.as_ref(),
            DeepLink::parse("teitunnel://inspect?share=qs-2").unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(host.asked(), asked, "navigation isn't confirmed");
    assert_eq!(
        host.opened.lock().unwrap()[1],
        View::Inspector {
            share: "qs-2".into()
        }
    );
}
