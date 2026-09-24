//! Listening sockets: accept loop, pluggable acceptor (plain TCP now; a rustls acceptor
//! for local HTTPS domains later), limits, host routing and graceful shutdown.

use std::{
    collections::HashMap,
    fmt,
    future::Future,
    io,
    net::{Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{
        Arc, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::Duration,
};

use hyper::service::service_fn;
use hyper_util::{
    rt::{TokioExecutor, TokioIo, TokioTimer},
    server::conn::auto,
};
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::{TcpListener, TcpStream},
    sync::Semaphore,
};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{LensError, ListenerId, TapId, lens::Shared, metrics::TapMetrics, service};

/// A byte stream a listener serves HTTP on.
pub trait Io: AsyncRead + AsyncWrite + Send + Unpin + 'static {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin + 'static> Io for T {}

/// A connection after the acceptor's handshake.
pub struct Accepted {
    /// The stream to serve HTTP on (decrypted, for TLS acceptors).
    pub io: Box<dyn Io>,
    /// The TLS server name (SNI) the client asked for, if any; used for host routing
    /// when requests carry no `Host`.
    pub server_name: Option<String>,
}

impl fmt::Debug for Accepted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Accepted")
            .field("server_name", &self.server_name)
            .finish_non_exhaustive()
    }
}

/// A future returned by an [`Acceptor`].
pub type AcceptFuture = Pin<Box<dyn Future<Output = io::Result<Accepted>> + Send>>;

/// Turns an accepted TCP connection into a stream to serve HTTP on. The plain acceptor
/// passes it through; a TLS acceptor (local HTTPS domains) performs the handshake.
pub trait Acceptor: Send + Sync + fmt::Debug {
    /// Performs the handshake. Lens bounds it with the header-read timeout.
    fn accept(&self, stream: TcpStream, peer: SocketAddr) -> AcceptFuture;

    /// Whether connections are encrypted (the scheme is `https`).
    fn is_secure(&self) -> bool;
}

/// Plain TCP (cloudflared connects over loopback).
#[derive(Debug, Clone, Copy, Default)]
pub struct PlainAcceptor;

impl Acceptor for PlainAcceptor {
    fn accept(&self, stream: TcpStream, _peer: SocketAddr) -> AcceptFuture {
        Box::pin(async move {
            Ok(Accepted {
                io: Box::new(stream),
                server_name: None,
            })
        })
    }

    fn is_secure(&self) -> bool {
        false
    }
}

/// Protocol limits and timeouts for a listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Largest request head (request line and headers), in bytes.
    pub max_header_bytes: usize,
    /// Most headers in a request.
    pub max_headers: usize,
    /// Longest request target (path and query), in bytes.
    pub max_uri_bytes: usize,
    /// How long a client may take to send a request head (also bounds idle keep-alive
    /// connections and TLS handshakes).
    pub header_read_timeout: Duration,
    /// Concurrent connections; more are closed at once.
    pub max_connections: usize,
    /// Concurrent HTTP/2 streams per connection.
    pub max_h2_streams: u32,
    /// How long in-flight requests get to finish when a listener closes.
    pub shutdown_grace: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_header_bytes: 64 * 1024,
            max_headers: 128,
            max_uri_bytes: 16 * 1024,
            header_read_timeout: Duration::from_secs(30),
            max_connections: 4_096,
            max_h2_streams: 256,
            shutdown_grace: Duration::from_secs(5),
        }
    }
}

/// Which tap answers which host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Routing {
    /// Every request goes to one tap.
    Tap(TapId),
    /// By `Host` (`app.test`, or `*.app.test` for any subdomain), with an optional
    /// fallback for other hosts.
    Hosts {
        /// Host → tap. Exact names win over wildcards; the longest wildcard wins.
        hosts: Vec<(String, TapId)>,
        /// For hosts that match nothing (otherwise: 404).
        fallback: Option<TapId>,
    },
}

impl Routing {
    /// Taps this routing refers to.
    pub fn taps(&self) -> Vec<TapId> {
        match self {
            Self::Tap(tap) => vec![tap.clone()],
            Self::Hosts { hosts, fallback } => hosts
                .iter()
                .map(|(_, tap)| tap.clone())
                .chain(fallback.clone())
                .collect(),
        }
    }
}

/// Compiled routing.
#[derive(Debug)]
pub(crate) struct HostTable {
    single: Option<TapId>,
    exact: HashMap<String, TapId>,
    /// (suffix including the leading dot, tap), longest first.
    wildcards: Vec<(String, TapId)>,
    fallback: Option<TapId>,
    routing: Routing,
}

impl HostTable {
    pub(crate) fn compile(routing: Routing) -> Result<Self, LensError> {
        let mut table = Self {
            single: None,
            exact: HashMap::new(),
            wildcards: Vec::new(),
            fallback: None,
            routing: routing.clone(),
        };
        match routing {
            Routing::Tap(tap) => table.single = Some(tap),
            Routing::Hosts { hosts, fallback } => {
                table.fallback = fallback;
                for (host, tap) in hosts {
                    let host = normalize_host(&host);
                    if let Some(suffix) = host.strip_prefix("*.") {
                        if suffix.is_empty() {
                            return Err(LensError::InvalidConfig(
                                "a wildcard host needs a domain".into(),
                            ));
                        }
                        table.wildcards.push((format!(".{suffix}"), tap));
                    } else if host.is_empty() || host.contains('*') {
                        return Err(LensError::InvalidConfig(format!(
                            "invalid route host {host:?}"
                        )));
                    } else {
                        table.exact.insert(host, tap);
                    }
                }
                table
                    .wildcards
                    .sort_by_key(|(suffix, _)| std::cmp::Reverse(suffix.len()));
            }
        }
        Ok(table)
    }

    /// The tap for `host` (already normalized).
    pub(crate) fn resolve(&self, host: &str) -> Option<&TapId> {
        if let Some(tap) = &self.single {
            return Some(tap);
        }
        self.exact
            .get(host)
            .or_else(|| {
                self.wildcards
                    .iter()
                    .find(|(suffix, _)| {
                        host.ends_with(suffix.as_str()) && host.len() > suffix.len()
                    })
                    .map(|(_, tap)| tap)
            })
            .or(self.fallback.as_ref())
    }

    pub(crate) fn routing(&self) -> &Routing {
        &self.routing
    }

    pub(crate) fn refers_to(&self, tap: &TapId) -> bool {
        self.routing.taps().contains(tap)
    }

    /// Removes `tap` from host routes (single-tap tables are closed by the caller).
    pub(crate) fn without(&self, tap: &TapId) -> Option<Routing> {
        match &self.routing {
            Routing::Tap(_) => None,
            Routing::Hosts { hosts, fallback } => Some(Routing::Hosts {
                hosts: hosts.iter().filter(|(_, t)| t != tap).cloned().collect(),
                fallback: fallback.clone().filter(|t| t != tap),
            }),
        }
    }
}

/// Lowercase, without port, brackets or a trailing dot.
pub(crate) fn normalize_host(host: &str) -> String {
    let host = host.trim();
    let without_port = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or_default()
    } else {
        match host.rsplit_once(':') {
            Some((name, port))
                if port.bytes().all(|b| b.is_ascii_digit()) && !name.contains(':') =>
            {
                name
            }
            _ => host,
        }
    };
    without_port.trim_end_matches('.').to_ascii_lowercase()
}

/// Options for [`crate::Lens::listen`].
#[derive(Debug, Clone)]
pub struct ListenOptions {
    /// Address to bind (default `127.0.0.1:0`, a random free port).
    pub addr: SocketAddr,
    /// Allow a non-loopback address. Off by default: Lens should be reachable only
    /// from this computer (cloudflared runs locally).
    pub allow_non_loopback: bool,
    /// Which taps answer.
    pub routing: Routing,
    /// Handshake for each connection (default: plain TCP).
    pub acceptor: Arc<dyn Acceptor>,
    /// Limits and timeouts.
    pub limits: Limits,
}

impl ListenOptions {
    /// A loopback listener on a random port routing everything to `tap`.
    pub fn tap(tap: TapId) -> Self {
        Self {
            addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
            allow_non_loopback: false,
            routing: Routing::Tap(tap),
            acceptor: Arc::new(PlainAcceptor),
            limits: Limits::default(),
        }
    }
}

/// A bound listener.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenerInfo {
    /// Id.
    pub id: ListenerId,
    /// The address actually bound (with the real port when 0 was asked for).
    pub addr: SocketAddr,
}

/// Connection facts the request path needs.
#[derive(Debug, Clone)]
pub(crate) struct ConnInfo {
    pub(crate) peer: SocketAddr,
    pub(crate) server_name: Option<String>,
}

/// A listener's shared state.
#[derive(Debug)]
pub(crate) struct ListenerState {
    pub(crate) id: ListenerId,
    pub(crate) addr: SocketAddr,
    pub(crate) secure: bool,
    pub(crate) limits: Limits,
    pub(crate) routes: RwLock<Arc<HostTable>>,
    pub(crate) cancel: CancellationToken,
    pub(crate) tracker: TaskTracker,
    pub(crate) connections: AtomicU64,
}

impl ListenerState {
    pub(crate) fn routes(&self) -> Arc<HostTable> {
        Arc::clone(&self.routes.read().unwrap_or_else(PoisonError::into_inner))
    }

    pub(crate) fn set_routes(&self, table: HostTable) {
        *self.routes.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(table);
    }

    /// The tap whose connection gauge this listener feeds (single-tap listeners only).
    fn gauge(&self, shared: &Shared) -> Option<Arc<TapMetrics>> {
        match self.routes().routing() {
            Routing::Tap(tap) => shared.tap(tap).map(|tap| Arc::clone(&tap.metrics)),
            Routing::Hosts { .. } => None,
        }
    }
}

/// Binds and starts accepting. Returns the state and the accept loop's future.
pub(crate) async fn bind(
    shared: &Arc<Shared>,
    id: ListenerId,
    options: ListenOptions,
) -> Result<(Arc<ListenerState>, TcpListener, Arc<dyn Acceptor>), LensError> {
    if !options.allow_non_loopback && !options.addr.ip().is_loopback() {
        return Err(LensError::NotLoopback(options.addr));
    }
    let table = HostTable::compile(options.routing)?;
    let listener = TcpListener::bind(options.addr)
        .await
        .map_err(|source| LensError::Bind {
            addr: options.addr,
            source,
        })?;
    let addr = listener.local_addr().map_err(|source| LensError::Bind {
        addr: options.addr,
        source,
    })?;
    let state = Arc::new(ListenerState {
        id,
        addr,
        secure: options.acceptor.is_secure(),
        limits: options.limits,
        routes: RwLock::new(Arc::new(table)),
        cancel: shared.shutdown.child_token(),
        tracker: TaskTracker::new(),
        connections: AtomicU64::new(0),
    });
    Ok((state, listener, options.acceptor))
}

/// The accept loop; ends when the listener is cancelled.
pub(crate) async fn run(
    shared: Arc<Shared>,
    state: Arc<ListenerState>,
    listener: TcpListener,
    acceptor: Arc<dyn Acceptor>,
) {
    let permits = Arc::new(Semaphore::new(state.limits.max_connections.max(1)));
    loop {
        let accepted = tokio::select! {
            () = state.cancel.cancelled() => break,
            accepted = listener.accept() => accepted,
        };
        let (stream, peer) = match accepted {
            Ok(pair) => pair,
            Err(err) => {
                // Out of file descriptors and similar: back off instead of spinning.
                tracing::warn!(error = %err, "Lens couldn't accept a connection");
                tokio::time::sleep(Duration::from_millis(50)).await;
                continue;
            }
        };
        let Ok(permit) = Arc::clone(&permits).try_acquire_owned() else {
            tracing::warn!(%peer, "Lens is at its connection limit; closing a new connection");
            continue;
        };
        let _ = stream.set_nodelay(true);
        let shared = Arc::clone(&shared);
        let conn_state = Arc::clone(&state);
        let acceptor = Arc::clone(&acceptor);
        state.tracker.spawn(async move {
            serve_connection(shared, conn_state, acceptor, stream, peer).await;
            drop(permit);
        });
    }
    drop(listener);
    state.tracker.close();
}

async fn serve_connection(
    shared: Arc<Shared>,
    state: Arc<ListenerState>,
    acceptor: Arc<dyn Acceptor>,
    stream: TcpStream,
    peer: SocketAddr,
) {
    let limits = state.limits;
    let accepted =
        match tokio::time::timeout(limits.header_read_timeout, acceptor.accept(stream, peer)).await
        {
            Ok(Ok(accepted)) => accepted,
            Ok(Err(err)) => {
                tracing::debug!(error = %err, %peer, "handshake failed");
                return;
            }
            Err(_) => return,
        };
    let gauge = state.gauge(&shared);
    state.connections.fetch_add(1, Relaxed);
    if let Some(gauge) = &gauge {
        gauge.active_connections.fetch_add(1, Relaxed);
    }
    let conn = ConnInfo {
        peer,
        server_name: accepted.server_name,
    };
    let service = {
        let shared = Arc::clone(&shared);
        let state = Arc::clone(&state);
        service_fn(move |request| {
            let shared = Arc::clone(&shared);
            let state = Arc::clone(&state);
            let conn = conn.clone();
            async move { service::handle(shared, state, conn, request).await }
        })
    };
    let mut builder = auto::Builder::new(TokioExecutor::new());
    builder
        .http1()
        .timer(TokioTimer::new())
        .header_read_timeout(limits.header_read_timeout)
        .max_buf_size(limits.max_header_bytes.max(8 * 1024))
        .max_headers(limits.max_headers)
        .keep_alive(true);
    builder
        .http2()
        .timer(TokioTimer::new())
        .max_concurrent_streams(limits.max_h2_streams)
        .max_header_list_size(u32::try_from(limits.max_header_bytes).unwrap_or(u32::MAX))
        .keep_alive_interval(Some(Duration::from_secs(30)));
    let connection = builder.serve_connection_with_upgrades(TokioIo::new(accepted.io), service);
    tokio::pin!(connection);
    tokio::select! {
        result = connection.as_mut() => {
            if let Err(err) = result {
                tracing::debug!(error = %err, %peer, "connection ended with an error");
            }
        }
        () = state.cancel.cancelled() => {
            connection.as_mut().graceful_shutdown();
            let _ = tokio::time::timeout(limits.shutdown_grace, connection.as_mut()).await;
        }
    }
    TapMetrics::dec(&state.connections);
    if let Some(gauge) = &gauge {
        TapMetrics::dec(&gauge.active_connections);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tap(id: &str) -> TapId {
        TapId::new(id).unwrap()
    }

    #[test]
    fn normalizes_hosts() {
        assert_eq!(normalize_host("App.Test:8080"), "app.test");
        assert_eq!(normalize_host("app.test."), "app.test");
        assert_eq!(normalize_host("[::1]:80"), "::1");
        assert_eq!(normalize_host("::1"), "::1");
        assert_eq!(normalize_host("localhost"), "localhost");
    }

    #[test]
    fn host_table_resolution() {
        let table = HostTable::compile(Routing::Hosts {
            hosts: vec![
                ("app.test".into(), tap("a")),
                ("*.test".into(), tap("any")),
                ("*.api.test".into(), tap("api")),
            ],
            fallback: Some(tap("fb")),
        })
        .unwrap();
        assert_eq!(table.resolve("app.test"), Some(&tap("a")));
        assert_eq!(table.resolve("v1.api.test"), Some(&tap("api")));
        assert_eq!(table.resolve("other.test"), Some(&tap("any")));
        assert_eq!(table.resolve("test"), Some(&tap("fb")));
        assert_eq!(table.resolve("example.com"), Some(&tap("fb")));
        assert!(table.refers_to(&tap("api")));
        let without = table.without(&tap("fb")).unwrap();
        let smaller = HostTable::compile(without).unwrap();
        assert_eq!(smaller.resolve("example.com"), None);

        let single = HostTable::compile(Routing::Tap(tap("s"))).unwrap();
        assert_eq!(single.resolve("anything"), Some(&tap("s")));
        assert!(single.without(&tap("s")).is_none());
    }

    #[test]
    fn rejects_bad_hosts() {
        for bad in ["*.", "", "a*b.test"] {
            assert!(
                HostTable::compile(Routing::Hosts {
                    hosts: vec![(bad.into(), tap("a"))],
                    fallback: None,
                })
                .is_err(),
                "{bad}"
            );
        }
    }
}
