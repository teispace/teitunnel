//! The server the app runs: authenticates each connection, applies limits, asks the
//! person before changes, and answers from a [`Host`].

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::{
    io::BufReader,
    sync::{Mutex, Semaphore, mpsc},
    task::JoinSet,
    time::Instant,
};

use crate::{
    endpoint::{Connection, Listener, Token},
    framing::{Frame, read_frame, write_frame},
    host::{Action, ConfirmRequest, Decision, Host, Requester},
    protocol::{
        AgentApproval, AgentDecision, AgentInfo, ApplyParams, ClientInfo, EVENT_NOTIFICATION,
        HelloParams, HelloResult, MAX_MESSAGE, Notification, PROTOCOL_VERSION, PauseShare, Request,
        Response, RoutesParams, RpcError, StartShare, StopShare, SubscribeParams, View, code,
        event, method,
    },
};

/// Limits and timeouts.
#[derive(Debug, Clone)]
pub struct Limits {
    /// Connections at once.
    pub max_connections: usize,
    /// Time to send `hello` after connecting.
    pub hello_timeout: Duration,
    /// Time for a read-only request.
    pub request_timeout: Duration,
    /// Time for a change, including the person's answer.
    pub mutation_timeout: Duration,
    /// Requests a connection may send in a burst…
    pub burst: u32,
    /// …refilled at this many per second.
    pub per_second: u32,
    /// Changes a connection may ask for per minute.
    pub mutations_per_minute: u32,
    /// Requests of one connection being answered at once.
    pub in_flight: usize,
    /// The longest message.
    pub max_message: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_connections: 32,
            hello_timeout: Duration::from_secs(5),
            request_timeout: Duration::from_secs(60),
            mutation_timeout: Duration::from_secs(180),
            burst: 40,
            per_second: 20,
            mutations_per_minute: 12,
            in_flight: 8,
            max_message: MAX_MESSAGE,
        }
    }
}

/// A token bucket.
#[derive(Debug)]
struct Bucket {
    capacity: f64,
    tokens: f64,
    per_second: f64,
    last: Instant,
}

impl Bucket {
    fn new(capacity: u32, per_second: f64) -> Self {
        Self {
            capacity: f64::from(capacity),
            tokens: f64::from(capacity),
            per_second,
            last: Instant::now(),
        }
    }

    fn take(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + elapsed * self.per_second).min(self.capacity);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// The control server.
pub struct Server {
    host: Arc<dyn Host>,
    token: Token,
    limits: Limits,
    connections: AtomicUsize,
    sessions: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for Server {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Server")
            .field("limits", &self.limits)
            .finish_non_exhaustive()
    }
}

fn to_value<T: Serialize>(value: &T) -> Result<Value, RpcError> {
    serde_json::to_value(value).map_err(|e| RpcError::new(code::INTERNAL, e.to_string()))
}

fn params<T: DeserializeOwned>(params: Option<Value>) -> Result<T, RpcError> {
    serde_json::from_value(params.unwrap_or(Value::Null))
        .map_err(|e| RpcError::new(code::INVALID_PARAMS, format!("Invalid parameters: {e}.")))
}

/// Parameters that may be left out entirely.
fn optional_params<T: DeserializeOwned + Default>(params: Option<Value>) -> Result<T, RpcError> {
    match params {
        None | Some(Value::Null) => Ok(T::default()),
        Some(value) => self::params(Some(value)),
    }
}

impl Server {
    /// A server answering from `host`, for clients presenting `token`.
    pub fn new(host: Arc<dyn Host>, token: Token) -> Arc<Self> {
        Self::with_limits(host, token, Limits::default())
    }

    /// A server with other limits (tests).
    pub fn with_limits(host: Arc<dyn Host>, token: Token, limits: Limits) -> Arc<Self> {
        Arc::new(Self {
            host,
            token,
            limits,
            connections: AtomicUsize::new(0),
            sessions: std::sync::atomic::AtomicU64::new(1),
        })
    }

    /// Accepts connections until `stop` resolves; then closes every connection.
    pub async fn run(self: Arc<Self>, mut listener: Listener, stop: impl Future<Output = ()>) {
        let mut connections = JoinSet::new();
        tokio::pin!(stop);
        loop {
            tokio::select! {
                () = &mut stop => break,
                accepted = listener.accept() => match accepted {
                    Ok(connection) => {
                        let server = Arc::clone(&self);
                        connections.spawn(async move { server.serve(connection).await });
                    }
                    Err(err) => {
                        tracing::warn!(%err, "control connection failed");
                        tokio::time::sleep(Duration::from_millis(100)).await;
                    }
                },
                Some(_) = connections.join_next(), if !connections.is_empty() => {}
            }
        }
        connections.shutdown().await;
    }

    /// Serves one connection until it closes (public so other transports can be used).
    pub async fn serve(self: Arc<Self>, connection: Connection) {
        let count = self.connections.fetch_add(1, Ordering::SeqCst) + 1;
        let (reader, mut writer) = tokio::io::split(connection);
        if count > self.limits.max_connections {
            let error = RpcError::new(code::RATE_LIMITED, "Too many connections.");
            if let Ok(line) = serde_json::to_string(&Response::err(Value::Null, error)) {
                let _ = write_frame(&mut writer, &line).await;
            }
            self.connections.fetch_sub(1, Ordering::SeqCst);
            return;
        }
        let (tx, mut rx) = mpsc::channel::<String>(64);
        let write = tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if write_frame(&mut writer, &line).await.is_err() {
                    break;
                }
            }
        });
        Session::new(Arc::clone(&self), tx)
            .run(BufReader::new(reader))
            .await;
        // Everything queued is written before the connection closes.
        let _ = write.await;
        self.connections.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One client's session.
struct Session {
    id: u64,
    agent: bool,
    server: Arc<Server>,
    tx: mpsc::Sender<String>,
    client: Option<ClientInfo>,
    requests: Bucket,
    mutations: Bucket,
    in_flight: Arc<Semaphore>,
    confirming: Arc<Mutex<()>>,
    tasks: JoinSet<()>,
    events: Option<tokio::task::JoinHandle<()>>,
}

impl Session {
    fn new(server: Arc<Server>, tx: mpsc::Sender<String>) -> Self {
        let limits = &server.limits;
        Self {
            id: server.sessions.fetch_add(1, Ordering::Relaxed),
            agent: false,
            requests: Bucket::new(limits.burst, f64::from(limits.per_second)),
            mutations: Bucket::new(
                limits.mutations_per_minute,
                f64::from(limits.mutations_per_minute) / 60.0,
            ),
            in_flight: Arc::new(Semaphore::new(limits.in_flight)),
            confirming: Arc::default(),
            tasks: JoinSet::new(),
            events: None,
            client: None,
            server,
            tx,
        }
    }

    async fn send(&self, response: Response) {
        if let Ok(line) = serde_json::to_string(&response) {
            let _ = self.tx.send(line).await;
        }
    }

    async fn run<R: tokio::io::AsyncBufRead + Unpin>(mut self, mut reader: R) {
        let max = self.server.limits.max_message;
        loop {
            let frame = if self.client.is_none() {
                match tokio::time::timeout(
                    self.server.limits.hello_timeout,
                    read_frame(&mut reader, max),
                )
                .await
                {
                    Ok(frame) => frame,
                    Err(_) => {
                        let error = RpcError::new(code::UNAUTHORIZED, "Send hello first.");
                        self.send(Response::err(Value::Null, error)).await;
                        break;
                    }
                }
            } else {
                read_frame(&mut reader, max).await
            };
            let line = match frame {
                Ok(Some(Frame::Line(line))) => line,
                Ok(Some(Frame::TooLarge)) => {
                    let error = RpcError::new(code::TOO_LARGE, "Message too large.")
                        .with_data(json!({ "max": max }));
                    self.send(Response::err(Value::Null, error)).await;
                    break;
                }
                Ok(None) | Err(_) => break,
            };
            if line.trim().is_empty() {
                continue;
            }
            if !self.handle_line(&line).await {
                break;
            }
        }
        if let Some(events) = self.events.take() {
            events.abort();
        }
        self.tasks.shutdown().await;
        if self.agent {
            self.server.host.agent_disconnected(self.id);
        }
    }

    /// Handles one message; `false` closes the connection.
    async fn handle_line(&mut self, line: &str) -> bool {
        let request: Request = match serde_json::from_str::<Value>(line) {
            Err(_) => {
                self.send(Response::err(
                    Value::Null,
                    RpcError::new(code::PARSE_ERROR, "Not JSON."),
                ))
                .await;
                return self.client.is_some();
            }
            Ok(value) => {
                let id = value.get("id").cloned().unwrap_or(Value::Null);
                match serde_json::from_value::<Request>(value) {
                    Ok(request) if request.jsonrpc == "2.0" => request,
                    _ => {
                        self.send(Response::err(
                            id,
                            RpcError::new(code::INVALID_REQUEST, "Not a JSON-RPC 2.0 request."),
                        ))
                        .await;
                        return self.client.is_some();
                    }
                }
            }
        };
        let Some(client) = self.client.clone() else {
            return self.hello(request).await;
        };
        // Notifications from clients mean nothing here.
        let Some(id) = request.id.clone() else {
            return true;
        };
        if !self.requests.take() {
            self.send(Response::err(
                id,
                RpcError::new(code::RATE_LIMITED, "Too many requests. Slow down."),
            ))
            .await;
            return true;
        }
        let mutation = method::is_mutation(&request.method);
        if mutation && !self.mutations.take() {
            self.send(Response::err(
                id,
                RpcError::new(code::RATE_LIMITED, "Too many changes in a short time."),
            ))
            .await;
            return true;
        }
        if request.method == method::AGENT_REGISTER {
            let response = match params::<AgentInfo>(request.params) {
                Ok(agent) if agent.is_valid() => {
                    self.server.host.agent_connected(self.id, agent, &client);
                    self.agent = true;
                    Response::ok(id, json!({}))
                }
                Ok(_) => Response::err(
                    id,
                    RpcError::new(code::INVALID_PARAMS, "Give the agent a short name."),
                ),
                Err(error) => Response::err(id, error),
            };
            self.send(response).await;
            return true;
        }
        if request.method == method::EVENTS_SUBSCRIBE {
            let response = match self.subscribe(request.params) {
                Ok(result) => Response::ok(id, result),
                Err(error) => Response::err(id, error),
            };
            self.send(response).await;
            return true;
        }
        let Ok(permit) = Arc::clone(&self.in_flight).try_acquire_owned() else {
            self.send(Response::err(
                id,
                RpcError::new(code::RATE_LIMITED, "Too many requests at once."),
            ))
            .await;
            return true;
        };
        let server = Arc::clone(&self.server);
        let tx = self.tx.clone();
        let confirming = Arc::clone(&self.confirming);
        let session = self.id;
        self.tasks.spawn(async move {
            let timeout = if mutation {
                server.limits.mutation_timeout
            } else {
                server.limits.request_timeout
            };
            let result = tokio::time::timeout(
                timeout,
                dispatch(
                    &server,
                    &client,
                    &confirming,
                    session,
                    &request.method,
                    request.params,
                ),
            )
            .await
            .unwrap_or_else(|_| Err(RpcError::new(code::TIMEOUT, "The request took too long.")));
            let response = match result {
                Ok(value) => Response::ok(id, value),
                Err(error) => Response::err(id, error),
            };
            if let Ok(line) = serde_json::to_string(&response) {
                let _ = tx.send(line).await;
            }
            drop(permit);
        });
        // Reap finished requests so the set doesn't grow.
        while self.tasks.try_join_next().is_some() {}
        true
    }

    /// The first message: `hello` with the token. Anything else ends the connection.
    async fn hello(&mut self, request: Request) -> bool {
        let id = request.id.clone().unwrap_or(Value::Null);
        let unauthorized = |message: &str| RpcError::new(code::UNAUTHORIZED, message);
        if request.method != method::HELLO {
            self.send(Response::err(id, unauthorized("Send hello first.")))
                .await;
            return false;
        }
        let hello: HelloParams = match params(request.params) {
            Ok(hello) => hello,
            Err(error) => {
                self.send(Response::err(id, error)).await;
                return false;
            }
        };
        if !self.server.token.matches(&hello.token) {
            tracing::warn!(client = %hello.client.name, "control client refused: wrong token");
            self.send(Response::err(id, unauthorized("Wrong token.")))
                .await;
            return false;
        }
        if hello.protocol != PROTOCOL_VERSION {
            let error = RpcError::new(
                code::UNSUPPORTED_PROTOCOL,
                format!(
                    "This Teitunnel speaks protocol {PROTOCOL_VERSION}, not {}.",
                    hello.protocol
                ),
            )
            .with_data(json!({ "supported": [PROTOCOL_VERSION] }));
            self.send(Response::err(id, error)).await;
            return false;
        }
        if !hello.client.is_valid() {
            let error = RpcError::new(code::INVALID_PARAMS, "Give the client a short name.");
            self.send(Response::err(id, error)).await;
            return false;
        }
        let approved = self.server.host.is_approved(&hello.client).await;
        let result = HelloResult {
            protocol: PROTOCOL_VERSION,
            app: self.server.host.app(),
            approved,
            methods: method::ALL.iter().map(|&m| m.to_owned()).collect(),
            events: event::ALL.iter().map(|&e| e.to_owned()).collect(),
        };
        tracing::info!(client = %hello.client.name, version = %hello.client.version, "control client connected");
        self.client = Some(hello.client);
        let response = match to_value(&result) {
            Ok(value) => Response::ok(id, value),
            Err(error) => Response::err(id, error),
        };
        self.send(response).await;
        true
    }

    /// Starts (or changes) this connection's event subscription.
    fn subscribe(&mut self, params: Option<Value>) -> Result<Value, RpcError> {
        let SubscribeParams { events } = optional_params(params)?;
        let wanted: Vec<String> = match events {
            None => event::ALL.iter().map(|&e| e.to_owned()).collect(),
            Some(list) => {
                if let Some(unknown) = list.iter().find(|e| !event::ALL.contains(&e.as_str())) {
                    return Err(RpcError::new(
                        code::INVALID_PARAMS,
                        format!("No event called {unknown}."),
                    ));
                }
                list
            }
        };
        if let Some(previous) = self.events.take() {
            previous.abort();
        }
        let mut events = self.server.host.subscribe();
        let tx = self.tx.clone();
        let filter = wanted.clone();
        self.events = Some(tokio::spawn(async move {
            loop {
                match events.recv().await {
                    Ok(event) => {
                        if !filter.iter().any(|kind| kind == event.kind()) {
                            continue;
                        }
                        let Ok(params) = serde_json::to_value(&event) else {
                            continue;
                        };
                        let notification = Notification {
                            jsonrpc: "2.0".into(),
                            method: EVENT_NOTIFICATION.into(),
                            params,
                        };
                        let Ok(line) = serde_json::to_string(&notification) else {
                            continue;
                        };
                        if tx.send(line).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }));
        Ok(json!({ "events": wanted }))
    }
}

/// Asks the person before a change, unless they allowed this client already. Changes
/// to DNS records Teitunnel didn't create are always asked about.
async fn gate(
    server: &Server,
    client: &ClientInfo,
    confirming: &Mutex<()>,
    action: Action,
) -> Result<(), RpcError> {
    let always_ask = matches!(
        &action,
        Action::Apply(ApplyParams {
            confirmed: true,
            ..
        })
    );
    if !always_ask && server.host.is_approved(client).await {
        return Ok(());
    }
    // One question per connection at a time.
    let Ok(_asking) = confirming.try_lock() else {
        return Err(RpcError::new(
            code::RATE_LIMITED,
            "Teitunnel is already asking about another change from this client.",
        ));
    };
    let decision = server
        .host
        .confirm(ConfirmRequest {
            requester: Requester::Client(client.clone()),
            action,
            offer_always: !always_ask,
        })
        .await;
    match decision {
        Decision::Once => Ok(()),
        Decision::Always => {
            server.host.approve(client).await;
            Ok(())
        }
        Decision::Deny => Err(RpcError::new(
            code::DECLINED,
            "The change wasn't allowed in Teitunnel.",
        )),
    }
}

async fn dispatch(
    server: &Server,
    client: &ClientInfo,
    confirming: &Mutex<()>,
    session: u64,
    name: &str,
    raw: Option<Value>,
) -> Result<Value, RpcError> {
    let host = &server.host;
    match name {
        method::HELLO => Err(RpcError::new(code::INVALID_REQUEST, "Already said hello.")),
        method::STATUS => to_value(&host.status().await?),
        method::SHARES_LIST => to_value(&host.shares().await?),
        method::SHARES_START => {
            let request: StartShare = params(raw)?;
            gate(
                server,
                client,
                confirming,
                Action::StartShare(request.clone()),
            )
            .await?;
            to_value(&host.start_share(request).await?)
        }
        method::SHARES_STOP => {
            let request: StopShare = params(raw)?;
            gate(
                server,
                client,
                confirming,
                Action::StopShare(request.clone()),
            )
            .await?;
            host.stop_share(request).await?;
            Ok(json!({}))
        }
        method::SHARES_PAUSE | method::SHARES_RESUME => {
            let request: PauseShare = params(raw)?;
            let paused = name == method::SHARES_PAUSE;
            let action = if paused {
                Action::PauseShare(request.clone())
            } else {
                Action::ResumeShare(request.clone())
            };
            gate(server, client, confirming, action).await?;
            host.pause_share(request, paused).await?;
            Ok(json!({}))
        }
        method::AGENT_APPROVE => {
            let request: AgentApproval = params(raw)?;
            if request.title.trim().is_empty() {
                return Err(RpcError::new(
                    code::INVALID_PARAMS,
                    "Say what needs approving.",
                ));
            }
            // One question per connection at a time, like other changes.
            let Ok(_asking) = confirming.try_lock() else {
                return Err(RpcError::new(
                    code::RATE_LIMITED,
                    "Teitunnel is already asking about another change from this agent.",
                ));
            };
            let approved = host.approve_for_agent(session, request).await;
            to_value(&AgentDecision { approved })
        }
        method::ROUTES_LIST => {
            let request: RoutesParams = optional_params(raw)?;
            to_value(&host.routes(request).await?)
        }
        method::ROUTES_PREVIEW => to_value(&host.preview(params(raw)?).await?),
        method::ROUTES_APPLY => {
            let request: ApplyParams = params(raw)?;
            gate(server, client, confirming, Action::Apply(request.clone())).await?;
            to_value(&host.apply(request, client).await?)
        }
        method::OPEN => {
            let view: View = params(raw)?;
            host.open(view).await?;
            Ok(json!({}))
        }
        method::DOCTOR_RUN => to_value(&host.doctor().await?),
        method::LOCAL_DOMAINS_LIST => to_value(&host.local_domains().await?),
        method::LOCAL_DOMAINS_RELOAD => to_value(&host.reload_local_domains().await?),
        other => Err(RpcError::new(
            code::METHOD_NOT_FOUND,
            format!("No method called {other}."),
        )),
    }
}

#[cfg(test)]
mod bucket_tests {
    use std::time::Duration;

    use super::Bucket;

    #[tokio::test(start_paused = true)]
    async fn refills_over_time_up_to_its_capacity() {
        let mut bucket = Bucket::new(2, 1.0);
        assert!(bucket.take() && bucket.take());
        assert!(!bucket.take());
        tokio::time::advance(Duration::from_millis(1500)).await;
        assert!(bucket.take());
        assert!(!bucket.take());
        tokio::time::advance(Duration::from_secs(60)).await;
        assert!(bucket.take() && bucket.take());
        assert!(!bucket.take(), "never more than the capacity");
    }
}
