//! Tunnel connectors and remote logs through Cloudflare's management service.
//!
//! `GET …/cfd_tunnel/{id}/connections` lists each connector (one per machine running
//! the tunnel) with its edge connections. `POST …/cfd_tunnel/{id}/management` issues a
//! short-lived token for `wss://management.argotunnel.com/logs`, which relays a
//! connector's own log stream: the client sends `start_streaming` first, then receives
//! `logs` events until either side closes. Shapes and close codes per cloudflared's
//! `cfapi/tunnel.go`, `management/events.go` and `management/service.go` (checked
//! 2026-09-23).

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{
        Message,
        client::IntoClientRequest,
        http::{HeaderValue, header::USER_AGENT},
        protocol::{CloseFrame, frame::coding::CloseCode},
    },
};

use crate::{Client, Result, resources::encode};

/// Where connectors' log streams are relayed.
pub const MANAGEMENT_BASE: &str = "wss://management.argotunnel.com";

/// How long connecting (TCP, TLS and the WebSocket handshake) may take.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// One machine running a tunnel.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Connector {
    /// Connector id (UUID), as cloudflared reports it on its `/ready` endpoint.
    pub id: String,
    /// cloudflared version.
    #[serde(default)]
    pub version: String,
    /// OS and architecture, e.g. `darwin_arm64`.
    #[serde(default)]
    pub arch: String,
    /// When it started (RFC 3339).
    #[serde(default)]
    pub run_at: String,
    /// Its connections to the edge.
    #[serde(default)]
    pub conns: Vec<ConnectorConnection>,
}

/// One edge connection of a connector.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct ConnectorConnection {
    /// Edge location, e.g. `ams01`.
    #[serde(default)]
    pub colo_name: String,
    /// Public IP the connector connects from.
    #[serde(default)]
    pub origin_ip: String,
    /// When it connected (RFC 3339).
    #[serde(default)]
    pub opened_at: String,
    /// Whether it's reconnecting.
    #[serde(default)]
    pub is_pending_reconnect: bool,
}

impl Client {
    /// The connectors running a tunnel.
    ///
    /// # Errors
    /// API or network errors.
    pub async fn tunnel_connectors(&self, account: &str, tunnel: &str) -> Result<Vec<Connector>> {
        self.get(&format!(
            "/accounts/{}/cfd_tunnel/{}/connections",
            encode(account),
            encode(tunnel)
        ))
        .await
    }

    /// A short-lived token for streaming a tunnel's connector logs. It's a secret: it
    /// reads everything the tunnel's connectors log.
    ///
    /// # Errors
    /// API or network errors (the credential needs Cloudflare Tunnel: Edit).
    pub async fn management_token(&self, account: &str, tunnel: &str) -> Result<String> {
        self.post(
            &format!(
                "/accounts/{}/cfd_tunnel/{}/management",
                encode(account),
                encode(tunnel)
            ),
            &json!({ "resources": ["logs"] }),
        )
        .await
    }
}

/// One log entry from a connector.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct RemoteLog {
    /// When (RFC 3339).
    #[serde(default)]
    pub time: Option<String>,
    /// `debug`, `info`, `warn` or `error`.
    #[serde(default)]
    pub level: Option<String>,
    /// The message.
    #[serde(default)]
    pub message: String,
    /// `cloudflared`, `http`, `tcp` or `udp`.
    #[serde(default)]
    pub event: Option<String>,
    /// Structured fields.
    #[serde(default)]
    pub fields: Map<String, Value>,
}

#[derive(Deserialize)]
struct ServerEvent {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    logs: Vec<RemoteLog>,
}

#[derive(Serialize)]
struct StartStreaming<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    filters: Filters<'a>,
}

#[derive(Serialize)]
struct Filters<'a> {
    level: &'a str,
}

/// Why a log stream ended.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum StreamError {
    /// Couldn't connect (network, TLS, or the relay refused the token).
    #[error("Couldn't connect to the connector's logs: {0}")]
    Connect(String),
    /// The connector already has as many log sessions as it allows.
    #[error(
        "This connector already streams its logs elsewhere (a `cloudflared tail`, or another window). Close that and try again."
    )]
    SessionLimit,
    /// The session was idle for too long.
    #[error("The log stream timed out.")]
    Idle,
    /// The stream closed or broke.
    #[error("The log stream closed{}", .0.as_deref().map(|r| format!(": {r}")).unwrap_or_default())]
    Closed(Option<String>),
}

impl StreamError {
    /// Whether connecting again may help.
    pub fn is_transient(&self) -> bool {
        matches!(self, Self::Idle | Self::Closed(_) | Self::Connect(_))
    }
}

/// A connector's live log stream.
pub struct LogStream {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
}

impl std::fmt::Debug for LogStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogStream").finish_non_exhaustive()
    }
}

impl LogStream {
    /// Connects to `base` (see [`MANAGEMENT_BASE`]) with a management `token` and starts
    /// streaming `connector`'s logs (every connector's when `None`) at `level` and above.
    ///
    /// # Errors
    /// See [`StreamError`].
    pub async fn connect(
        base: &str,
        token: &str,
        connector: Option<&str>,
        level: &str,
    ) -> Result<Self, StreamError> {
        let mut url = format!("{base}/logs?access_token={}", crate::encode_query(token));
        if let Some(id) = connector {
            url.push_str("&connector_id=");
            url.push_str(&crate::encode_query(id));
        }
        let mut request = url
            .into_client_request()
            .map_err(|e| StreamError::Connect(redact(&e.to_string())))?;
        request.headers_mut().insert(
            USER_AGENT,
            HeaderValue::from_static(concat!("Teitunnel/", env!("CARGO_PKG_VERSION"))),
        );
        let (mut socket, _) =
            tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async(request))
                .await
                .map_err(|_| StreamError::Connect("timed out".into()))?
                .map_err(|e| StreamError::Connect(redact(&e.to_string())))?;
        let start = StartStreaming {
            kind: "start_streaming",
            filters: Filters { level },
        };
        let text = serde_json::to_string(&start).unwrap_or_default();
        socket
            .send(Message::text(text))
            .await
            .map_err(|e| StreamError::Connect(redact(&e.to_string())))?;
        Ok(Self { socket })
    }

    /// The next batch of log entries.
    ///
    /// # Errors
    /// The stream ended; see [`StreamError`].
    pub async fn next(&mut self) -> Result<Vec<RemoteLog>, StreamError> {
        loop {
            let message = match self.socket.next().await {
                Some(Ok(message)) => message,
                Some(Err(err)) => return Err(StreamError::Closed(Some(redact(&err.to_string())))),
                None => return Err(StreamError::Closed(None)),
            };
            match message {
                Message::Text(text) => {
                    // Unknown events are skipped: the protocol may grow.
                    if let Ok(event) = serde_json::from_str::<ServerEvent>(&text)
                        && event.kind == "logs"
                    {
                        return Ok(event.logs);
                    }
                }
                Message::Close(frame) => return Err(closed(frame.as_ref())),
                // Pings are answered by tungstenite while reading.
                _ => {}
            }
        }
    }

    /// Asks the connector to stop and closes the connection.
    pub async fn close(mut self) {
        let _ = self
            .socket
            .send(Message::text(r#"{"type":"stop_streaming"}"#))
            .await;
        let _ = self.socket.close(None).await;
    }
}

fn closed(frame: Option<&CloseFrame>) -> StreamError {
    match frame.map(|f| f.code) {
        Some(CloseCode::Library(4002)) => StreamError::SessionLimit,
        Some(CloseCode::Library(4003)) => StreamError::Idle,
        _ => StreamError::Closed(
            frame
                .map(|f| f.reason.to_string())
                .filter(|reason| !reason.is_empty()),
        ),
    }
}

/// Error texts may quote the request URL, which carries the token.
fn redact(text: &str) -> String {
    let Some(start) = text.find("access_token=") else {
        return text.to_owned();
    };
    let value = start + "access_token=".len();
    let end = text[value..]
        .find(|c: char| c == '&' || c == '"' || c.is_whitespace())
        .map_or(text.len(), |i| value + i);
    format!("{}[redacted]{}", &text[..value], &text[end..])
}

#[cfg(test)]
mod tests {
    use tokio::net::TcpListener;
    use tokio_tungstenite::{accept_hdr_async, tungstenite};
    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;
    use crate::ApiToken;

    fn ok(result: &Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "success": true, "errors": [], "messages": [], "result": result
        }))
    }

    #[tokio::test]
    async fn lists_connectors_and_issues_a_logs_token() {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t")).unwrap();
        Mock::given(method("GET"))
            .and(path("/accounts/a1/cfd_tunnel/t1/connections"))
            .respond_with(ok(&json!([{
                "id": "c1", "arch": "darwin_arm64", "version": "2026.9.1",
                "run_at": "2026-09-23T00:00:00Z", "features": ["ha-origin"],
                "conns": [{"colo_name": "ams01", "id": "x", "is_pending_reconnect": false,
                           "origin_ip": "198.51.100.7", "opened_at": "2026-09-23T00:00:01Z",
                           "client_id": "c1", "client_version": "2026.9.1", "uuid": "x"}]
            }])))
            .mount(&server)
            .await;
        let connectors = client.tunnel_connectors("a1", "t1").await.unwrap();
        assert_eq!(connectors[0].id, "c1");
        assert_eq!(connectors[0].conns[0].colo_name, "ams01");

        Mock::given(method("POST"))
            .and(path("/accounts/a1/cfd_tunnel/t1/management"))
            .respond_with(|req: &Request| {
                let body: Value = serde_json::from_slice(&req.body).unwrap();
                assert_eq!(body, json!({"resources": ["logs"]}));
                ok(&json!("mgmt-token"))
            })
            .mount(&server)
            .await;
        assert_eq!(
            client.management_token("a1", "t1").await.unwrap(),
            "mgmt-token"
        );
    }

    /// A stand-in for the relay: checks the request, expects `start_streaming`, sends
    /// `frames`, then closes with `close`.
    async fn relay(frames: Vec<Value>, close: Option<u16>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            // The callback's signature (and its large error type) is tungstenite's.
            #[allow(clippy::result_large_err)]
            let mut ws = accept_hdr_async(
                tcp,
                |req: &tungstenite::handshake::server::Request,
                 res: tungstenite::handshake::server::Response| {
                    let query = req.uri().query().unwrap_or_default();
                    assert!(query.contains("access_token=secret%2Btoken"), "{query}");
                    assert!(query.contains("connector_id=c1"), "{query}");
                    assert!(
                        req.headers()[USER_AGENT]
                            .to_str()
                            .unwrap()
                            .starts_with("Teitunnel/")
                    );
                    Ok(res)
                },
            )
            .await
            .unwrap();
            let first = ws.next().await.unwrap().unwrap();
            let start: Value = serde_json::from_str(first.to_text().unwrap()).unwrap();
            assert_eq!(start["type"], "start_streaming");
            assert_eq!(start["filters"]["level"], "debug");
            for frame in frames {
                ws.send(Message::text(frame.to_string())).await.unwrap();
            }
            let frame = close.map(|code| CloseFrame {
                code: CloseCode::from(code),
                reason: "bye".into(),
            });
            let _ = ws.close(frame).await;
        });
        format!("ws://{addr}")
    }

    #[tokio::test]
    async fn streams_a_connectors_logs() {
        let base = relay(
            vec![
                json!({"type": "future_event"}),
                json!({"type": "logs", "logs": [
                    {"time": "2026-09-23T00:00:00Z", "level": "info", "message": "Registered tunnel connection",
                     "event": "cloudflared", "fields": {"connIndex": 0, "location": "ams01"}},
                    {"level": "debug", "message": "GET https://app.xyz.com/ HTTP/1.1", "event": "http"}
                ]}),
            ],
            Some(4002),
        )
        .await;
        let mut stream = LogStream::connect(&base, "secret+token", Some("c1"), "debug")
            .await
            .unwrap();
        let logs = stream.next().await.unwrap();
        assert_eq!(logs.len(), 2);
        assert_eq!(logs[0].message, "Registered tunnel connection");
        assert_eq!(logs[0].fields["location"], "ams01");
        assert_eq!(logs[1].event.as_deref(), Some("http"));
        assert_eq!(stream.next().await.unwrap_err(), StreamError::SessionLimit);
    }

    #[tokio::test]
    async fn maps_close_codes() {
        let base = relay(Vec::new(), Some(4003)).await;
        let mut stream = LogStream::connect(&base, "secret+token", Some("c1"), "debug")
            .await
            .unwrap();
        let err = stream.next().await.unwrap_err();
        assert_eq!(err, StreamError::Idle);
        assert!(err.is_transient());
        assert!(!StreamError::SessionLimit.is_transient());
    }

    #[tokio::test]
    async fn never_reveals_the_token_in_errors() {
        // Nothing listens here, so connecting fails with an error.
        let err = LogStream::connect("ws://127.0.0.1:9", "s3cr3t", None, "info")
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("s3cr3t"), "{err}");
        assert_eq!(
            redact(r#"GET wss://m/logs?access_token=abc.def&connector_id=1 failed"#),
            "GET wss://m/logs?access_token=[redacted]&connector_id=1 failed"
        );
    }
}
