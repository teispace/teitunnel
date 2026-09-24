//! A client for the control connection: the CLI uses it, and it documents how an
//! extension talks to the app.

use std::{
    collections::HashMap,
    io,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    io::BufReader,
    sync::{broadcast, mpsc, oneshot},
};

use crate::{
    endpoint::{Connection, Endpoint},
    framing::{Frame, read_frame, write_frame},
    protocol::{
        ApplyParams, ApplyResult, ClientInfo, DoctorIssue, EVENT_NOTIFICATION, Event, HelloParams,
        HelloResult, MAX_MESSAGE, PROTOCOL_VERSION, PlanInfo, PreviewParams, Response, RoutesList,
        RoutesParams, RpcError, ShareInfo, StartShare, Status, StopShare, View, code, method,
    },
};

/// How long `hello` may take.
const HELLO_TIMEOUT: Duration = Duration::from_secs(3);

/// Why a call failed.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The app isn't running (or never ran on this machine).
    #[error("Teitunnel isn't running.")]
    NotRunning,
    /// The app refused or failed the request.
    #[error("{0}")]
    Rpc(RpcError),
    /// The connection broke.
    #[error("The connection to Teitunnel broke: {0}")]
    Io(#[from] io::Error),
    /// The app answered something unexpected.
    #[error("Teitunnel answered unexpectedly: {0}")]
    Protocol(String),
}

impl ClientError {
    /// The protocol error code, when the app answered with one.
    pub fn code(&self) -> Option<i64> {
        match self {
            Self::Rpc(error) => Some(error.code),
            _ => None,
        }
    }
}

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Response>>>>;

/// A connection to the running app.
#[derive(Debug)]
pub struct ControlClient {
    tx: mpsc::Sender<String>,
    pending: Pending,
    next: AtomicU64,
    events: broadcast::Sender<Event>,
    hello: HelloResult,
    reader: tokio::task::JoinHandle<()>,
    writer: tokio::task::JoinHandle<()>,
}

impl Drop for ControlClient {
    fn drop(&mut self) {
        self.reader.abort();
        self.writer.abort();
    }
}

fn not_running(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::NotFound
            | io::ErrorKind::ConnectionRefused
            | io::ErrorKind::TimedOut
            | io::ErrorKind::InvalidData
            | io::ErrorKind::PermissionDenied
    )
}

impl ControlClient {
    /// Connects to the app of the data folder `endpoint` belongs to and says hello.
    ///
    /// # Errors
    /// [`ClientError::NotRunning`] when no app answers; the app's error when it refuses.
    pub async fn connect(endpoint: &Endpoint, client: ClientInfo) -> Result<Self, ClientError> {
        let token = endpoint.read_token().map_err(|_| ClientError::NotRunning)?;
        let connection = endpoint.connect().await.map_err(|err| {
            if not_running(&err) {
                ClientError::NotRunning
            } else {
                ClientError::Io(err)
            }
        })?;
        Self::handshake(connection, token.as_str(), client).await
    }

    /// Says hello over an open connection.
    ///
    /// # Errors
    /// The app refused the token or the protocol version, or didn't answer in time.
    pub async fn handshake(
        connection: Connection,
        token: &str,
        client: ClientInfo,
    ) -> Result<Self, ClientError> {
        let (reader, mut writer) = tokio::io::split(connection);
        let (tx, mut rx) = mpsc::channel::<String>(32);
        let writer = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if write_frame(&mut writer, &line).await.is_err() {
                    break;
                }
            }
        });
        let pending: Pending = Arc::default();
        let (events, _) = broadcast::channel(256);
        let reader = tokio::spawn(read_loop(
            BufReader::new(reader),
            Arc::clone(&pending),
            events.clone(),
        ));
        let mut this = Self {
            tx,
            pending,
            next: AtomicU64::new(1),
            events,
            hello: HelloResult {
                protocol: 0,
                app: crate::protocol::AppInfo {
                    name: String::new(),
                    version: String::new(),
                },
                approved: false,
                methods: Vec::new(),
                events: Vec::new(),
            },
            reader,
            writer,
        };
        let hello = HelloParams {
            protocol: PROTOCOL_VERSION,
            token: token.to_owned(),
            client,
        };
        this.hello = tokio::time::timeout(HELLO_TIMEOUT, this.call(method::HELLO, &hello))
            .await
            .map_err(|_| ClientError::NotRunning)??;
        Ok(this)
    }

    /// What the app said in `hello`.
    pub fn hello(&self) -> &HelloResult {
        &self.hello
    }

    /// Calls a method.
    ///
    /// # Errors
    /// The app's error, or a broken connection.
    pub async fn call<P: Serialize + ?Sized, R: DeserializeOwned>(
        &self,
        method: &str,
        params: &P,
    ) -> Result<R, ClientError> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (done, answer) = oneshot::channel();
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, done);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let line =
            serde_json::to_string(&request).map_err(|e| ClientError::Protocol(e.to_string()))?;
        if self.tx.send(line).await.is_err() {
            return Err(ClientError::Io(io::ErrorKind::BrokenPipe.into()));
        }
        let response = answer
            .await
            .map_err(|_| ClientError::Io(io::ErrorKind::UnexpectedEof.into()))?;
        if let Some(error) = response.error {
            return Err(ClientError::Rpc(error));
        }
        serde_json::from_value(response.result.unwrap_or(Value::Null))
            .map_err(|e| ClientError::Protocol(e.to_string()))
    }

    /// Accounts, tunnels and every share.
    ///
    /// # Errors
    /// See [`ControlClient::call`].
    pub async fn status(&self) -> Result<Status, ClientError> {
        self.call(method::STATUS, &json!({})).await
    }

    /// Every share.
    ///
    /// # Errors
    /// See [`ControlClient::call`].
    pub async fn shares(&self) -> Result<Vec<ShareInfo>, ClientError> {
        self.call(method::SHARES_LIST, &json!({})).await
    }

    /// Shares a local service in the app (the person may be asked first).
    ///
    /// # Errors
    /// See [`ControlClient::call`]; [`code::DECLINED`] when the person said no.
    pub async fn start_share(&self, request: &StartShare) -> Result<ShareInfo, ClientError> {
        self.call(method::SHARES_START, request).await
    }

    /// Stops a share.
    ///
    /// # Errors
    /// See [`ControlClient::call`].
    pub async fn stop_share(&self, id: &str) -> Result<(), ClientError> {
        let _: Value = self
            .call(method::SHARES_STOP, &StopShare { id: id.to_owned() })
            .await?;
        Ok(())
    }

    /// This machine's routes in an account.
    ///
    /// # Errors
    /// See [`ControlClient::call`].
    pub async fn routes(&self, account: Option<&str>) -> Result<RoutesList, ClientError> {
        let params = RoutesParams {
            account: account.map(str::to_owned),
        };
        self.call(method::ROUTES_LIST, &params).await
    }

    /// Plans a change.
    ///
    /// # Errors
    /// See [`ControlClient::call`].
    pub async fn preview(&self, request: &PreviewParams) -> Result<PlanInfo, ClientError> {
        self.call(method::ROUTES_PREVIEW, request).await
    }

    /// Applies a reviewed plan.
    ///
    /// # Errors
    /// See [`ControlClient::call`]; [`code::STALE`] with the new plan when Cloudflare
    /// changed since the preview.
    pub async fn apply(&self, request: &ApplyParams) -> Result<ApplyResult, ClientError> {
        self.call(method::ROUTES_APPLY, request).await
    }

    /// Brings the app's window to a view.
    ///
    /// # Errors
    /// See [`ControlClient::call`].
    pub async fn open(&self, view: &View) -> Result<(), ClientError> {
        let _: Value = self.call(method::OPEN, view).await?;
        Ok(())
    }

    /// Runs the Doctor.
    ///
    /// # Errors
    /// See [`ControlClient::call`].
    pub async fn doctor(&self) -> Result<Vec<DoctorIssue>, ClientError> {
        self.call(method::DOCTOR_RUN, &json!({})).await
    }

    /// Subscribes to events (`None`: all) and returns a receiver for them.
    ///
    /// # Errors
    /// See [`ControlClient::call`].
    pub async fn subscribe(
        &self,
        events: Option<&[&str]>,
    ) -> Result<broadcast::Receiver<Event>, ClientError> {
        let receiver = self.events.subscribe();
        let _: Value = self
            .call(method::EVENTS_SUBSCRIBE, &json!({ "events": events }))
            .await?;
        Ok(receiver)
    }
}

async fn read_loop<R: tokio::io::AsyncBufRead + Unpin>(
    mut reader: R,
    pending: Pending,
    events: broadcast::Sender<Event>,
) {
    loop {
        let line = match read_frame(&mut reader, MAX_MESSAGE).await {
            Ok(Some(Frame::Line(line))) => line,
            Ok(Some(Frame::TooLarge) | None) | Err(_) => break,
        };
        let Ok(value) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if value.get("method").and_then(Value::as_str) == Some(EVENT_NOTIFICATION) {
            // Events of types this client doesn't know are skipped.
            if let Some(event) = value
                .get("params")
                .cloned()
                .and_then(|p| serde_json::from_value::<Event>(p).ok())
            {
                let _ = events.send(event);
            }
            continue;
        }
        let Ok(response) = serde_json::from_value::<Response>(value) else {
            continue;
        };
        let waiting = {
            let mut pending = pending.lock().unwrap_or_else(PoisonError::into_inner);
            match response.id.as_u64() {
                Some(id) => pending.remove(&id),
                // An error without an id (e.g. the connection is being closed) goes to
                // every caller still waiting.
                None => {
                    let all: Vec<_> = pending.drain().map(|(_, tx)| tx).collect();
                    for tx in all {
                        let _ = tx.send(response.clone());
                    }
                    None
                }
            }
        };
        if let Some(waiting) = waiting {
            let _ = waiting.send(response);
        }
    }
    // Callers still waiting learn the connection ended.
    pending
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clear();
}

/// Whether an error means "use the app another time, not now": the app isn't running,
/// or the control connection is off.
pub fn is_unavailable(error: &ClientError) -> bool {
    matches!(error, ClientError::NotRunning)
        || error.code() == Some(code::DISABLED)
        || matches!(error, ClientError::Rpc(e) if e.code == code::UNAUTHORIZED)
}
