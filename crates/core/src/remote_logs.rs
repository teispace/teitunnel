//! Live logs of a connector on another machine, relayed by Cloudflare's management
//! service (`cf_api::LogStream`).
//!
//! A session starts when the UI first reads a connector's logs and ends by itself once
//! nobody has read it for [`IDLE`], so a closed window never leaves a stream open. It
//! reconnects after transient drops (with a fresh token each time, since tokens are
//! short-lived) and gives up with a readable reason otherwise. Lines are kept in a
//! bounded ring and parsed like local cloudflared output, so the same viewer shows both.
//! The management token never leaves this module.

use std::{
    collections::HashMap,
    future::Future,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cf_api::{LogStream, RemoteLog, StreamError};
use serde::Serialize;
use serde_json::{Map, Value};
use tokio::time::Instant;

use crate::{Secret, runtime::LogBuffer, runtime::LogEvent};

/// Lines kept per session.
const CAPACITY: usize = 2_000;
/// A session nobody reads for this long stops.
pub const IDLE: Duration = Duration::from_secs(30);
/// Connection attempts in a row before giving up.
const ATTEMPTS: u32 = 5;
/// Minimum severity streamed. Request logs (debug) would flood a busy tunnel.
const LEVEL: &str = "info";

/// Issues management tokens (the Cloudflare API, or a stand-in in tests).
pub trait ManagementTokens: Clone + Send + Sync + 'static {
    /// A token to stream `tunnel`'s connector logs.
    fn management_token(
        &self,
        account: &str,
        tunnel: &str,
    ) -> impl Future<Output = cf_api::Result<Secret<String>>> + Send;
}

impl ManagementTokens for cf_api::Client {
    async fn management_token(
        &self,
        account: &str,
        tunnel: &str,
    ) -> cf_api::Result<Secret<String>> {
        Self::management_token(self, account, tunnel)
            .await
            .map(Secret::new)
    }
}

/// Where a session is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum RemoteLogState {
    /// Getting a token and connecting.
    Connecting,
    /// Receiving the connector's logs.
    Streaming,
    /// Stopped for good; stop the session and read again to retry.
    Ended {
        /// Why, in a sentence.
        message: String,
    },
}

/// A session's newest lines and its state.
#[derive(Debug, Clone)]
pub struct RemoteLogBatch {
    /// Where the session is.
    pub state: RemoteLogState,
    /// The newest lines, oldest first.
    pub lines: Vec<Arc<LogEvent>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    account: String,
    tunnel: String,
    connector: String,
}

#[derive(Debug)]
struct Session {
    lines: LogBuffer,
    state: Mutex<RemoteLogState>,
    last_read: Mutex<Instant>,
    task: Mutex<Option<tokio::task::AbortHandle>>,
}

impl Session {
    fn set(&self, state: RemoteLogState) {
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = state;
    }

    fn state(&self) -> RemoteLogState {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn idle(&self) -> bool {
        self.last_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .elapsed()
            >= IDLE
    }
}

/// Live log sessions, one per connector. Cheap to clone.
#[derive(Debug, Clone)]
pub struct RemoteLogs {
    base: Arc<str>,
    sessions: Arc<Mutex<HashMap<Key, Arc<Session>>>>,
}

impl Default for RemoteLogs {
    fn default() -> Self {
        Self::new(cf_api::MANAGEMENT_BASE)
    }
}

impl RemoteLogs {
    /// Sessions relayed by `base` (`cf_api::MANAGEMENT_BASE`, or a stand-in in tests).
    pub fn new(base: &str) -> Self {
        Self {
            base: Arc::from(base),
            sessions: Arc::default(),
        }
    }

    fn sessions(&self) -> std::sync::MutexGuard<'_, HashMap<Key, Arc<Session>>> {
        self.sessions.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The newest `limit` lines of `connector` (of `tunnel` in `account`), starting a
    /// session if there's none. An ended session stays (with its lines and reason) until
    /// [`Self::stop`]; reading after that starts a new one. Must be called from a Tokio
    /// runtime.
    pub fn read<T: ManagementTokens>(
        &self,
        tokens: &T,
        account: &str,
        tunnel: &str,
        connector: &str,
        limit: usize,
    ) -> RemoteLogBatch {
        let key = Key {
            account: account.to_owned(),
            tunnel: tunnel.to_owned(),
            connector: connector.to_owned(),
        };
        let session = {
            let mut sessions = self.sessions();
            match sessions.get(&key).cloned() {
                Some(session) => session,
                None => {
                    let session = Arc::new(Session {
                        lines: LogBuffer::new(CAPACITY),
                        state: Mutex::new(RemoteLogState::Connecting),
                        last_read: Mutex::new(Instant::now()),
                        task: Mutex::new(None),
                    });
                    let task = tokio::spawn(run(
                        self.clone(),
                        tokens.clone(),
                        key.clone(),
                        Arc::clone(&session),
                    ));
                    *session.task.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(task.abort_handle());
                    sessions.insert(key, Arc::clone(&session));
                    session
                }
            }
        };
        *session
            .last_read
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Instant::now();
        RemoteLogBatch {
            state: session.state(),
            lines: session.lines.tail(limit),
        }
    }

    /// Stops `connector`'s session, if any.
    pub fn stop(&self, account: &str, tunnel: &str, connector: &str) {
        let key = Key {
            account: account.to_owned(),
            tunnel: tunnel.to_owned(),
            connector: connector.to_owned(),
        };
        if let Some(session) = self.sessions().remove(&key) {
            abort(&session);
        }
    }

    /// Stops every session of `account` (signing out).
    pub fn forget_account(&self, account: &str) {
        self.sessions().retain(|key, session| {
            let keep = key.account != account;
            if !keep {
                abort(session);
            }
            keep
        });
    }

    /// Number of running sessions.
    pub fn len(&self) -> usize {
        self.sessions().len()
    }

    /// Whether no session is running.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes `session` if it's still the one registered for `key`.
    fn remove(&self, key: &Key, session: &Arc<Session>) {
        let mut sessions = self.sessions();
        if sessions.get(key).is_some_and(|s| Arc::ptr_eq(s, session)) {
            sessions.remove(key);
        }
    }
}

fn abort(session: &Session) {
    if let Some(task) = session
        .task
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        task.abort();
    }
}

/// A relayed entry as a parsed log event (same classification as local output).
fn to_event(log: RemoteLog) -> LogEvent {
    let mut object: Map<String, Value> = log.fields;
    if let Some(time) = log.time {
        object.insert("time".into(), Value::String(time));
    }
    object.insert(
        "level".into(),
        Value::String(log.level.unwrap_or_else(|| "info".into())),
    );
    object.insert("message".into(), Value::String(log.message));
    cloudflared::parse_line(&Value::Object(object).to_string())
}

fn backoff(attempt: u32) -> Duration {
    Duration::from_secs(u64::from(attempt.min(4)).pow(2).max(1))
}

async fn run<T: ManagementTokens>(owner: RemoteLogs, tokens: T, key: Key, session: Arc<Session>) {
    let ended = |message: String| RemoteLogState::Ended { message };
    let mut attempt = 0;
    loop {
        if session.idle() {
            owner.remove(&key, &session);
            return;
        }
        session.set(RemoteLogState::Connecting);
        let token = match tokens.management_token(&key.account, &key.tunnel).await {
            Ok(token) => token,
            Err(err) if err.is_auth() => {
                session.set(ended(
                    "This account's token can't read connector logs. It needs Cloudflare Tunnel: Edit.".into(),
                ));
                return;
            }
            Err(err) => {
                attempt += 1;
                if attempt >= ATTEMPTS {
                    session.set(ended(format!("Couldn't get access to the logs: {err}")));
                    return;
                }
                tokio::time::sleep(backoff(attempt)).await;
                continue;
            }
        };
        let mut stream = match LogStream::connect(
            &owner.base,
            token.expose(),
            Some(&key.connector),
            LEVEL,
        )
        .await
        {
            Ok(stream) => stream,
            Err(err) => {
                attempt += 1;
                if !err.is_transient() || attempt >= ATTEMPTS {
                    session.set(ended(err.to_string()));
                    return;
                }
                tokio::time::sleep(backoff(attempt)).await;
                continue;
            }
        };
        session.set(RemoteLogState::Streaming);
        attempt = 0;
        let outcome = loop {
            tokio::select! {
                batch = stream.next() => match batch {
                    Ok(logs) => {
                        for log in logs {
                            session.lines.push(Arc::new(to_event(log)));
                        }
                    }
                    Err(err) => break Some(err),
                },
                () = tokio::time::sleep(Duration::from_secs(5)) => {
                    if session.idle() {
                        break None;
                    }
                }
            }
        };
        match outcome {
            None => {
                stream.close().await;
                owner.remove(&key, &session);
                return;
            }
            Some(err @ StreamError::SessionLimit) => {
                session.set(ended(err.to_string()));
                return;
            }
            // Dropped or timed out: reconnect with a fresh token.
            Some(err) => {
                tracing::debug!(%err, "remote log stream ended; reconnecting");
                tokio::time::sleep(backoff(1)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use futures_util::{SinkExt, StreamExt};
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::{
        Message,
        protocol::{CloseFrame, frame::coding::CloseCode},
    };

    use super::*;

    #[derive(Clone, Default)]
    struct Tokens {
        issued: Arc<AtomicU32>,
        forbidden: bool,
    }

    impl ManagementTokens for Tokens {
        async fn management_token(&self, _: &str, _: &str) -> cf_api::Result<Secret<String>> {
            if self.forbidden {
                return Err(cf_api::Error::Api {
                    status: 403,
                    errors: Vec::new(),
                });
            }
            let n = self.issued.fetch_add(1, Ordering::SeqCst);
            Ok(Secret::new(format!("token-{n}")))
        }
    }

    /// A relay that serves `sessions` connections: each sends one log batch, then closes
    /// with the given code.
    async fn relay(sessions: Vec<u16>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for (n, code) in sessions.into_iter().enumerate() {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let _start = ws.next().await;
                let batch = json!({"type": "logs", "logs": [
                    {"time": "2026-09-23T00:00:00Z", "level": "error", "message": "Request failed",
                     "event": "cloudflared", "fields": {"error": format!("dial tcp 127.0.0.1:3000: refused #{n}")}}
                ]});
                ws.send(Message::text(batch.to_string())).await.unwrap();
                let _ = ws
                    .close(Some(CloseFrame {
                        code: CloseCode::from(code),
                        reason: "".into(),
                    }))
                    .await;
            }
            // Keep the listener open (unanswered) so later attempts just wait.
            std::future::pending::<()>().await;
        });
        format!("ws://{addr}")
    }

    async fn until(
        logs: &RemoteLogs,
        tokens: &Tokens,
        done: impl Fn(&RemoteLogBatch) -> bool,
    ) -> RemoteLogBatch {
        for _ in 0..200 {
            let batch = logs.read(tokens, "acc", "t1", "c1", 100);
            if done(&batch) {
                return batch;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("timed out");
    }

    #[tokio::test]
    async fn streams_reconnects_with_a_fresh_token_and_stops_at_the_session_limit() {
        // 1000 = a normal close (reconnect), then the session limit (give up).
        let logs = RemoteLogs::new(&relay(vec![1000, 4002]).await);
        let tokens = Tokens::default();
        let batch = until(&logs, &tokens, |b| {
            matches!(b.state, RemoteLogState::Ended { .. })
        })
        .await;
        let RemoteLogState::Ended { message } = &batch.state else {
            unreachable!()
        };
        assert!(message.contains("already streams its logs"), "{message}");
        let errors: Vec<_> = batch.lines.iter().filter_map(|l| l.error.clone()).collect();
        assert_eq!(
            errors,
            [
                "dial tcp 127.0.0.1:3000: refused #0",
                "dial tcp 127.0.0.1:3000: refused #1"
            ]
        );
        assert_eq!(batch.lines[0].level, cloudflared::Level::Error);
        assert_eq!(
            tokens.issued.load(Ordering::SeqCst),
            2,
            "a token per connection"
        );
    }

    #[tokio::test]
    async fn a_token_without_permission_ends_with_a_reason() {
        let logs = RemoteLogs::new("ws://127.0.0.1:9");
        let tokens = Tokens {
            forbidden: true,
            ..Tokens::default()
        };
        let batch = until(&logs, &tokens, |b| {
            matches!(b.state, RemoteLogState::Ended { .. })
        })
        .await;
        assert!(
            matches!(&batch.state, RemoteLogState::Ended { message } if message.contains("Cloudflare Tunnel: Edit"))
        );
    }

    #[tokio::test]
    async fn sessions_are_per_connector_and_stop_when_asked() {
        let logs = RemoteLogs::new(&relay(Vec::new()).await);
        let tokens = Tokens::default();
        logs.read(&tokens, "acc", "t1", "c1", 10);
        logs.read(&tokens, "acc", "t1", "c1", 10);
        logs.read(&tokens, "acc", "t1", "c2", 10);
        logs.read(&tokens, "other", "t9", "c9", 10);
        assert_eq!(logs.len(), 3);
        logs.stop("acc", "t1", "c2");
        assert_eq!(logs.len(), 2);
        logs.forget_account("acc");
        assert_eq!(logs.len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn an_unread_session_stops_by_itself() {
        let logs = RemoteLogs::new(&relay(Vec::new()).await);
        let tokens = Tokens::default();
        logs.read(&tokens, "acc", "t1", "c1", 10);
        assert_eq!(logs.len(), 1);
        tokio::time::sleep(IDLE + Duration::from_secs(10)).await;
        for _ in 0..50 {
            if logs.is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        assert!(logs.is_empty());
    }
}
