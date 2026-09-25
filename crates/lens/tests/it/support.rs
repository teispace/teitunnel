//! Test helpers: a tiny origin server, a raw HTTP/1.1 client, and a Lens with one tap.

use std::{
    convert::Infallible,
    future::Future,
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode};
use http_body_util::{BodyExt, Full};
use hyper::{body::Incoming, service::service_fn};
use hyper_util::{
    rt::{TokioExecutor, TokioIo},
    server::conn::auto,
};
use lens::{
    Exchange, Filter, Lens, LensBody, LensOptions, TapConfig, TapHandle, Upstream, WaitOptions,
};
use tokio::net::{TcpListener, TcpStream};

/// A running test origin.
pub struct Origin {
    pub addr: SocketAddr,
    pub url: String,
    /// Connections accepted so far.
    pub connections: Arc<AtomicUsize>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Origin {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Starts an origin answering every request with `handler`.
pub async fn origin<F, Fut>(handler: F) -> Origin
where
    F: Fn(Request<Incoming>) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = Response<LensBody>> + Send + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let connections = Arc::new(AtomicUsize::new(0));
    let accepted = Arc::clone(&connections);
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            accepted.fetch_add(1, Ordering::SeqCst);
            let handler = handler.clone();
            tokio::spawn(async move {
                let service = service_fn(move |request| {
                    let handler = handler.clone();
                    async move { Ok::<_, Infallible>(handler(request).await) }
                });
                let _ = auto::Builder::new(TokioExecutor::new())
                    .serve_connection_with_upgrades(TokioIo::new(stream), service)
                    .await;
            });
        }
    });
    Origin {
        addr,
        url: format!("http://{addr}"),
        connections,
        task,
    }
}

/// An address nothing listens on.
pub async fn closed_port() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    format!("http://{addr}")
}

/// A Lens with one tap in front of `upstream`, tweaked by `configure`.
pub async fn lens_with(
    upstream: Upstream,
    configure: impl FnOnce(&mut TapConfig),
) -> (Lens, TapHandle) {
    let lens = Lens::new(LensOptions::default()).unwrap();
    let mut config = TapConfig::new(upstream);
    configure(&mut config);
    let tap = lens.start_tap(config).await.unwrap();
    (lens, tap)
}

/// A Lens forwarding to `origin_url`.
pub async fn lens_for(origin_url: &str) -> (Lens, TapHandle) {
    lens_with(Upstream::origin(origin_url).unwrap(), |_| {}).await
}

/// A complete response.
pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: Bytes,
}

impl Reply {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// Sends one request over a new HTTP/1.1 connection.
pub async fn send(addr: SocketAddr, request: Request<LensBody>) -> Response<Incoming> {
    let stream = TcpStream::connect(addr).await.unwrap();
    let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
        .await
        .unwrap();
    tokio::spawn(async move {
        let _ = conn.with_upgrades().await;
    });
    sender.send_request(request).await.unwrap()
}

/// Sends a request and reads the whole response.
pub async fn fetch(addr: SocketAddr, request: Request<LensBody>) -> Reply {
    let response = send(addr, request).await;
    let status = response.status();
    let headers = response.headers().clone();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    Reply {
        status,
        headers,
        body,
    }
}

/// A request builder with `Host: app.test` set.
pub fn request(method: Method, path: &str) -> http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "app.test")
}

/// `GET path`.
pub async fn get(addr: SocketAddr, path: &str) -> Reply {
    fetch(
        addr,
        request(Method::GET, path).body(lens::empty()).unwrap(),
    )
    .await
}

/// A response with a text body.
pub fn text_response(status: u16, body: impl Into<Bytes>) -> Response<LensBody> {
    let mut response = Response::new(lens::full(body.into()));
    *response.status_mut() = StatusCode::from_u16(status).unwrap();
    response
        .headers_mut()
        .insert("content-type", HeaderValue::from_static("text/plain"));
    response
}

/// Waits for the next finished exchange on `tap` (since the start of the test).
pub async fn next_exchange(lens: &Lens, filter: Filter) -> Arc<Exchange> {
    lens.wait_for(
        &filter,
        &WaitOptions {
            timeout: Duration::from_secs(20),
            since_ms: Some(0),
            ..WaitOptions::default()
        },
    )
    .await
    .unwrap()
}

/// Waits for a finished exchange whose path is `path`.
pub async fn exchange_for(lens: &Lens, path: &str) -> Arc<Exchange> {
    let filter = Filter {
        path: Some(path.to_owned()),
        ..Filter::default()
    };
    next_exchange(lens, filter).await
}

/// A full body.
pub fn body(data: impl Into<Bytes>) -> LensBody {
    Full::new(data.into())
        .map_err(|never| match never {})
        .boxed_unsync()
}
