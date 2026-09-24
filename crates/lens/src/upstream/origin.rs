//! A small pooled HTTP client for one origin: HTTP/1.1 keep-alive connections (with
//! upgrade support), or one multiplexed HTTP/2 connection; plain TCP or TLS.
//!
//! Lens needs control a general-purpose client doesn't give: the moment a connection
//! is ready (for timings), requests returned unsent on a stale pooled connection (so
//! they can be retried safely), and upgrades.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::{Arc, Mutex, PoisonError, Weak},
    time::Instant,
};

use http::{Request, Response};
use hyper::{
    body::Incoming,
    client::conn::{http1, http2},
};
use hyper_util::rt::{TokioExecutor, TokioIo, TokioTimer};
use rustls::pki_types::ServerName;
use tokio::{
    io::{AsyncRead, AsyncWrite},
    net::TcpStream,
};
use tokio_rustls::TlsConnector;

use super::tls;
use crate::{LensBody, LensError, OriginConfig, capture::ErrorKind};

/// A failure talking to the origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpstreamError {
    pub(crate) kind: ErrorKind,
    pub(crate) message: String,
}

impl UpstreamError {
    fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}

trait Io: AsyncRead + AsyncWrite + Send + Unpin {}
impl<T: AsyncRead + AsyncWrite + Send + Unpin> Io for T {}

type BoxIo = Box<dyn Io>;

#[derive(Debug)]
struct Idle {
    sender: http1::SendRequest<LensBody>,
    since: Instant,
}

/// Connections to one origin.
pub(crate) struct OriginClient {
    config: OriginConfig,
    tls: Option<(TlsConnector, ServerName<'static>)>,
    idle: Mutex<Vec<Idle>>,
    h2: tokio::sync::Mutex<Option<http2::SendRequest<LensBody>>>,
}

impl std::fmt::Debug for OriginClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OriginClient")
            .field("url", &self.config.url)
            .field("http2", &self.config.http2)
            .finish_non_exhaustive()
    }
}

impl OriginClient {
    pub(crate) fn new(config: OriginConfig) -> Result<Self, LensError> {
        let tls = if config.url.is_https() {
            let client = tls::client_config(config.verify_tls, config.http2)?;
            let name = tls::server_name(
                config
                    .server_name
                    .as_deref()
                    .unwrap_or_else(|| config.url.host()),
            )?;
            Some((TlsConnector::from(client), name))
        } else {
            None
        };
        Ok(Self {
            config,
            tls,
            idle: Mutex::new(Vec::new()),
            h2: tokio::sync::Mutex::new(None),
        })
    }

    pub(crate) fn config(&self) -> &OriginConfig {
        &self.config
    }

    /// Sends `request` (origin-form or absolute URI; `Host` already set) and returns the
    /// response head. `connected` runs once a connection is ready.
    pub(crate) async fn send(
        self: &Arc<Self>,
        request: Request<LensBody>,
        connected: impl FnOnce(),
    ) -> Result<Response<Incoming>, UpstreamError> {
        let work = async {
            if self.config.http2 {
                self.send_h2(request, connected).await
            } else {
                self.send_h1(request, connected).await
            }
        };
        match self.config.response_timeout {
            Some(limit) => tokio::time::timeout(limit, work).await.unwrap_or_else(|_| {
                Err(UpstreamError::new(
                    ErrorKind::Timeout,
                    format!(
                        "{} didn't answer within {} s",
                        self.config.url,
                        limit.as_secs()
                    ),
                ))
            }),
            None => work.await,
        }
    }

    fn take_idle(&self) -> Option<http1::SendRequest<LensBody>> {
        let mut idle = self.idle.lock().unwrap_or_else(PoisonError::into_inner);
        let timeout = self.config.idle_timeout;
        idle.retain(|conn| !conn.sender.is_closed() && conn.since.elapsed() < timeout);
        while let Some(conn) = idle.pop() {
            if conn.sender.is_ready() {
                return Some(conn.sender);
            }
        }
        None
    }

    async fn send_h1(
        self: &Arc<Self>,
        mut request: Request<LensBody>,
        connected: impl FnOnce(),
    ) -> Result<Response<Incoming>, UpstreamError> {
        let mut connected = Some(connected);
        // Pooled connections may have been closed by the origin since they were used;
        // hyper hands the request back when it wasn't sent, so try the next one.
        while let Some(mut sender) = self.take_idle() {
            if let Some(ready) = connected.take() {
                ready();
            }
            match sender.try_send_request(request).await {
                Ok(response) => {
                    self.recycle(sender);
                    return Ok(response);
                }
                Err(mut err) => match err.take_message() {
                    Some(unsent) => request = unsent,
                    None => return Err(map_hyper(err.error())),
                },
            }
        }
        let io = self.connect().await?;
        let (mut sender, conn) = http1::Builder::new()
            .handshake::<_, LensBody>(TokioIo::new(io))
            .await
            .map_err(|err| map_hyper(&err))?;
        tokio::spawn(async move {
            if let Err(err) = conn.with_upgrades().await {
                tracing::debug!(error = %err, "origin connection ended");
            }
        });
        if let Some(ready) = connected.take() {
            ready();
        }
        let response = sender
            .send_request(request)
            .await
            .map_err(|err| map_hyper(&err))?;
        self.recycle(sender);
        Ok(response)
    }

    /// Returns the connection to the pool once its response has been fully read.
    fn recycle(self: &Arc<Self>, mut sender: http1::SendRequest<LensBody>) {
        if self.config.max_idle_connections == 0 {
            return;
        }
        let pool: Weak<Self> = Arc::downgrade(self);
        tokio::spawn(async move {
            if sender.ready().await.is_err() {
                return; // closed or upgraded
            }
            let Some(client) = pool.upgrade() else {
                return;
            };
            let mut idle = client.idle.lock().unwrap_or_else(PoisonError::into_inner);
            if idle.len() < client.config.max_idle_connections {
                idle.push(Idle {
                    sender,
                    since: Instant::now(),
                });
            }
        });
    }

    async fn send_h2(
        &self,
        request: Request<LensBody>,
        connected: impl FnOnce(),
    ) -> Result<Response<Incoming>, UpstreamError> {
        let mut sender = {
            let mut slot = self.h2.lock().await;
            match slot.as_ref().filter(|sender| !sender.is_closed()) {
                Some(sender) => sender.clone(),
                None => {
                    let io = self.connect().await?;
                    let (sender, conn) = http2::Builder::new(TokioExecutor::new())
                        .timer(TokioTimer::new())
                        .handshake::<_, LensBody>(TokioIo::new(io))
                        .await
                        .map_err(|err| map_hyper(&err))?;
                    tokio::spawn(async move {
                        if let Err(err) = conn.await {
                            tracing::debug!(error = %err, "origin h2 connection ended");
                        }
                    });
                    *slot = Some(sender.clone());
                    sender
                }
            }
        };
        sender.ready().await.map_err(|err| map_hyper(&err))?;
        connected();
        sender
            .send_request(request)
            .await
            .map_err(|err| map_hyper(&err))
    }

    /// Opens a TCP (and TLS) connection, trying each resolved address in turn.
    async fn connect(&self) -> Result<BoxIo, UpstreamError> {
        let url = &self.config.url;
        let addrs = resolve(url.host(), url.port()).await?;
        let mut last =
            UpstreamError::new(ErrorKind::Dns, format!("{} has no addresses", url.host()));
        for addr in addrs {
            match tokio::time::timeout(self.config.connect_timeout, TcpStream::connect(addr)).await
            {
                Ok(Ok(stream)) => {
                    let _ = stream.set_nodelay(true);
                    return self.wrap(stream).await;
                }
                Ok(Err(err)) => {
                    let error = map_io(&err, &format!("couldn't connect to {url} ({addr})"));
                    // A refusal on one address shouldn't hide a timeout on another, and
                    // vice versa; keep the most informative (refused beats the rest).
                    if last.kind != ErrorKind::ConnectionRefused {
                        last = error;
                    }
                }
                Err(_) => {
                    if last.kind != ErrorKind::ConnectionRefused {
                        last = UpstreamError::new(
                            ErrorKind::Timeout,
                            format!("connecting to {url} ({addr}) timed out"),
                        );
                    }
                }
            }
        }
        Err(last)
    }

    async fn wrap(&self, stream: TcpStream) -> Result<BoxIo, UpstreamError> {
        let Some((connector, name)) = &self.tls else {
            return Ok(Box::new(stream));
        };
        let handshake = connector.connect(name.clone(), stream);
        match tokio::time::timeout(self.config.connect_timeout, handshake).await {
            Ok(Ok(tls)) => Ok(Box::new(tls)),
            Ok(Err(err)) => Err(UpstreamError::new(
                ErrorKind::Tls,
                format!("TLS handshake with {} failed: {err}", self.config.url),
            )),
            Err(_) => Err(UpstreamError::new(
                ErrorKind::Timeout,
                format!("TLS handshake with {} timed out", self.config.url),
            )),
        }
    }
}

/// Resolves a host. `localhost` means both loopback addresses (IPv4 first), without
/// asking the system resolver: dev servers often listen on only one of them.
async fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, UpstreamError> {
    if host.eq_ignore_ascii_case("localhost") {
        return Ok(vec![
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
            SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
        ]);
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    tokio::net::lookup_host((host, port))
        .await
        .map(Iterator::collect)
        .map_err(|err| {
            UpstreamError::new(ErrorKind::Dns, format!("couldn't resolve {host}: {err}"))
        })
}

fn map_io(err: &io::Error, context: &str) -> UpstreamError {
    let kind = match err.kind() {
        io::ErrorKind::ConnectionRefused => ErrorKind::ConnectionRefused,
        io::ErrorKind::TimedOut => ErrorKind::Timeout,
        io::ErrorKind::ConnectionReset
        | io::ErrorKind::ConnectionAborted
        | io::ErrorKind::BrokenPipe
        | io::ErrorKind::UnexpectedEof => ErrorKind::ConnectionReset,
        _ if err
            .get_ref()
            .is_some_and(|inner| inner.downcast_ref::<rustls::Error>().is_some()) =>
        {
            ErrorKind::Tls
        }
        _ => ErrorKind::Other,
    };
    UpstreamError::new(kind, format!("{context}: {err}"))
}

/// Maps a hyper error by walking its source chain.
pub(crate) fn map_hyper(err: &hyper::Error) -> UpstreamError {
    let mut source: Option<&(dyn std::error::Error + 'static)> = std::error::Error::source(err);
    while let Some(inner) = source {
        if let Some(io) = inner.downcast_ref::<io::Error>() {
            return map_io(io, "the origin connection failed");
        }
        source = inner.source();
    }
    let kind = if err.is_timeout() {
        ErrorKind::Timeout
    } else if err.is_parse() || err.is_parse_status() {
        ErrorKind::Protocol
    } else if err.is_incomplete_message() || err.is_closed() || err.is_canceled() {
        ErrorKind::ConnectionReset
    } else {
        ErrorKind::Other
    };
    UpstreamError::new(kind, format!("the origin connection failed: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn localhost_resolves_to_both_loopbacks() {
        let addrs = resolve("LOCALHOST", 80).await.unwrap();
        assert_eq!(addrs.len(), 2);
        assert!(addrs[0].is_ipv4());
        assert_eq!(resolve("::1", 8).await.unwrap()[0].port(), 8);
    }

    #[test]
    fn maps_io_errors() {
        let refused = io::Error::from(io::ErrorKind::ConnectionRefused);
        assert_eq!(map_io(&refused, "x").kind, ErrorKind::ConnectionRefused);
        let reset = io::Error::from(io::ErrorKind::ConnectionReset);
        assert_eq!(map_io(&reset, "x").kind, ErrorKind::ConnectionReset);
        let tls = io::Error::new(io::ErrorKind::InvalidData, rustls::Error::DecryptError);
        assert_eq!(map_io(&tls, "x").kind, ErrorKind::Tls);
    }
}
