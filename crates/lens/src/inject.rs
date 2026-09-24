//! HTML injection and the reserved `/__teitunnel/` path.
//!
//! When a tap has an [`Injection`], page navigations (`Sec-Fetch-Dest: document`, or an
//! `Accept` that prefers HTML) are sent upstream without `Accept-Encoding`, and HTML
//! responses stream through an injector that inserts the snippet before the **last**
//! `</body>` (a `</body>` inside an inline script string must not be the target). The
//! injector holds back only the bytes from the latest `</body>` onwards, so pages still
//! stream. If the origin compresses anyway, a body up to the size cap is decompressed,
//! injected and sent uncompressed; larger or undecodable bodies pass through untouched.

use std::{
    fmt,
    future::Future,
    net::IpAddr,
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use http::{HeaderMap, HeaderValue, Method, Response, StatusCode, Uri, header};
use http_body::{Body, Frame};
use http_body_util::BodyExt;
use serde::{Deserialize, Serialize};

use crate::{
    LensBody, TapId,
    body::{BoxError, full},
    capture::decode_body,
    util::rfind_ascii_ci,
};

/// Paths under this prefix are answered by Lens, never forwarded.
pub const RESERVED_PREFIX: &str = "/__teitunnel/";

const CLOSE_BODY: &[u8] = b"</body>";

/// A snippet injected into HTML pages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Injection {
    /// HTML inserted before `</body>`, e.g. `<script src="/__teitunnel/overlay.js" defer></script>`.
    pub snippet: String,
    /// Pages larger than this (after decompression, or held back while streaming) are
    /// left untouched.
    pub max_html_bytes: usize,
}

impl Injection {
    /// Injects `snippet` into pages up to 4 MiB.
    pub fn new(snippet: impl Into<String>) -> Self {
        Self {
            snippet: snippet.into(),
            max_html_bytes: 4 * 1024 * 1024,
        }
    }
}

/// A request for a reserved `/__teitunnel/…` path, after the tap's gates admitted it.
#[derive(Debug, Clone)]
pub struct ReservedRequest {
    /// The tap.
    pub tap: TapId,
    /// Method.
    pub method: Method,
    /// Full request URI (path starts with [`RESERVED_PREFIX`]).
    pub uri: Uri,
    /// Headers.
    pub headers: HeaderMap,
    /// Body (at most 1 MiB; larger bodies are refused with 413 before the handler).
    pub body: Bytes,
    /// Client IP (per the tap's `trust_cf_connecting_ip`).
    pub client_ip: IpAddr,
}

/// A future returned by a [`ReservedHandler`].
pub type HandlerFuture = Pin<Box<dyn Future<Output = Response<LensBody>> + Send>>;

/// Answers reserved `/__teitunnel/…` paths (overlay scripts, the comments API…).
pub trait ReservedHandler: Send + Sync + fmt::Debug {
    /// Handles one request.
    fn handle(&self, request: ReservedRequest) -> HandlerFuture;
}

/// Largest body passed to a reserved handler.
pub(crate) const MAX_RESERVED_BODY: usize = 1024 * 1024;

/// Whether the request is a page navigation that should get the snippet.
pub(crate) fn wants_injection(method: &Method, headers: &HeaderMap) -> bool {
    if method != Method::GET || headers.contains_key("hx-request") {
        return false;
    }
    match headers
        .get("sec-fetch-dest")
        .and_then(|value| value.to_str().ok())
    {
        Some(dest) => dest.eq_ignore_ascii_case("document") || dest.eq_ignore_ascii_case("iframe"),
        None => headers
            .get(header::ACCEPT)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|accept| accept.contains("text/html")),
    }
}

fn is_html(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            let mime = value.split(';').next().unwrap_or_default().trim();
            mime.eq_ignore_ascii_case("text/html")
        })
}

fn is_attachment(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_DISPOSITION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| {
            value
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("attachment")
        })
}

fn is_encoded(headers: &HeaderMap) -> bool {
    headers
        .get(header::CONTENT_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| !value.trim().eq_ignore_ascii_case("identity"))
}

/// Applies `injection` to a response when it's an injectable HTML page.
pub(crate) async fn apply(
    injection: &Injection,
    status: StatusCode,
    mut response: Response<LensBody>,
) -> Response<LensBody> {
    let headers = response.headers();
    let has_page = (status.is_success() || status.is_client_error() || status.is_server_error())
        && status != StatusCode::NO_CONTENT;
    if !has_page || !is_html(headers) || is_attachment(headers) {
        return response;
    }
    let snippet = Bytes::from(injection.snippet.clone());
    if is_encoded(headers) {
        let declared = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok());
        if declared.is_none_or(|len| len > injection.max_html_bytes) {
            return response;
        }
        return inject_buffered(injection, &snippet, response).await;
    }
    let headers = response.headers_mut();
    headers.remove(header::CONTENT_LENGTH);
    weaken_etag(headers);
    let body = std::mem::replace(response.body_mut(), crate::body::empty());
    *response.body_mut() = InjectBody::new(body, snippet, injection.max_html_bytes).boxed_unsync();
    response
}

/// A changed body can't keep a strong validator.
fn weaken_etag(headers: &mut HeaderMap) {
    if let Some(etag) = headers.get(header::ETAG).and_then(|v| v.to_str().ok())
        && !etag.starts_with("W/")
        && let Ok(weak) = HeaderValue::from_str(&format!("W/{etag}"))
    {
        headers.insert(header::ETAG, weak);
    }
}

async fn inject_buffered(
    injection: &Injection,
    snippet: &Bytes,
    response: Response<LensBody>,
) -> Response<LensBody> {
    let (mut parts, body) = response.into_parts();
    let collected = match crate::body::read_limited(body, injection.max_html_bytes).await {
        Ok((data, true)) => data,
        // Content-Length said it fits; a body that doesn't, or fails, is cut off.
        Ok((data, false)) => {
            return Response::from_parts(parts, full(data));
        }
        Err(err) => {
            tracing::debug!(error = %err, "couldn't read an HTML body to inject into");
            return Response::from_parts(parts, crate::body::empty());
        }
    };
    let decoded = match decode_body(&parts.headers, &collected) {
        Ok(decoded) => decoded.into_owned(),
        Err(_) => return Response::from_parts(parts, full(collected)),
    };
    let Some(at) = rfind_ascii_ci(&decoded, CLOSE_BODY) else {
        parts.headers.remove(header::CONTENT_ENCODING);
        parts
            .headers
            .insert(header::CONTENT_LENGTH, HeaderValue::from(decoded.len()));
        return Response::from_parts(parts, full(decoded));
    };
    let mut out = Vec::with_capacity(decoded.len() + snippet.len());
    out.extend_from_slice(&decoded[..at]);
    out.extend_from_slice(snippet);
    out.extend_from_slice(&decoded[at..]);
    parts.headers.remove(header::CONTENT_ENCODING);
    parts
        .headers
        .insert(header::CONTENT_LENGTH, HeaderValue::from(out.len()));
    weaken_etag(&mut parts.headers);
    Response::from_parts(parts, full(out))
}

/// Streams HTML, inserting `snippet` before the last `</body>`.
struct InjectBody {
    inner: LensBody,
    snippet: Option<Bytes>,
    /// Bytes from the latest `</body>` (or a possible partial match) onwards.
    held: BytesMut,
    max_held: usize,
    /// Gave up (too much after `</body>`): pass everything through.
    passthrough: bool,
    ended: bool,
    trailers: Option<HeaderMap>,
}

impl InjectBody {
    fn new(inner: LensBody, snippet: Bytes, max_held: usize) -> Self {
        Self {
            inner,
            snippet: Some(snippet),
            held: BytesMut::new(),
            max_held,
            passthrough: false,
            ended: false,
            trailers: None,
        }
    }

    /// Takes the bytes that can be sent now, keeping what might precede the snippet.
    fn release(&mut self) -> Option<Bytes> {
        if self.passthrough {
            return (!self.held.is_empty()).then(|| self.held.split().freeze());
        }
        let keep_from = match rfind_ascii_ci(&self.held, CLOSE_BODY) {
            Some(at) => at,
            // Keep a tail that could be the start of `</body>` split across chunks.
            None => self.held.len().saturating_sub(CLOSE_BODY.len() - 1),
        };
        if self.held.len() - keep_from > self.max_held {
            self.passthrough = true;
            self.snippet = None;
            return (!self.held.is_empty()).then(|| self.held.split().freeze());
        }
        (keep_from > 0).then(|| self.held.split_to(keep_from).freeze())
    }

    /// The final bytes, with the snippet when a `</body>` is held.
    fn finish(&mut self) -> Option<Bytes> {
        let held = self.held.split().freeze();
        match self.snippet.take() {
            Some(snippet)
                if !self.passthrough
                    && held.len() >= CLOSE_BODY.len()
                    && held[..CLOSE_BODY.len()].eq_ignore_ascii_case(CLOSE_BODY) =>
            {
                let mut out = BytesMut::with_capacity(snippet.len() + held.len());
                out.extend_from_slice(&snippet);
                out.extend_from_slice(&held);
                Some(out.freeze())
            }
            _ => (!held.is_empty()).then_some(held),
        }
    }
}

impl Body for InjectBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = &mut *self;
        loop {
            if this.ended {
                if let Some(rest) = this.finish() {
                    return Poll::Ready(Some(Ok(Frame::data(rest))));
                }
                return Poll::Ready(this.trailers.take().map(|t| Ok(Frame::trailers(t))));
            }
            match Pin::new(&mut this.inner).poll_frame(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(None) => this.ended = true,
                Poll::Ready(Some(Err(err))) => return Poll::Ready(Some(Err(err))),
                Poll::Ready(Some(Ok(frame))) => match frame.into_data() {
                    Ok(data) => {
                        this.held.extend_from_slice(&data);
                        if let Some(ready) = this.release() {
                            return Poll::Ready(Some(Ok(Frame::data(ready))));
                        }
                    }
                    Err(frame) => {
                        if let Ok(trailers) = frame.into_trailers() {
                            this.trailers = Some(trailers);
                        }
                        this.ended = true;
                    }
                },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use http_body_util::{BodyExt, StreamBody};

    use super::*;

    const SNIPPET: &str = "<script src=\"/__teitunnel/o.js\"></script>";

    fn chunked(chunks: Vec<&'static [u8]>) -> LensBody {
        let frames: Vec<Result<Frame<Bytes>, BoxError>> = chunks
            .into_iter()
            .map(|c| Ok(Frame::data(Bytes::from_static(c))))
            .collect();
        StreamBody::new(futures_util::stream::iter(frames)).boxed_unsync()
    }

    fn html_response(body: LensBody) -> Response<LensBody> {
        let mut response = Response::new(body);
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        response
            .headers_mut()
            .insert(header::ETAG, HeaderValue::from_static("\"abc\""));
        response
    }

    async fn text(response: Response<LensBody>) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn injects_before_last_body_across_chunks() {
        let body = chunked(vec![
            b"<html><body><script>var s='</body>';</script>",
            b"<p>hi</p></bo",
            b"dy>\n</html>",
        ]);
        let response = apply(
            &Injection::new(SNIPPET),
            StatusCode::OK,
            html_response(body),
        )
        .await;
        assert_eq!(response.headers()[header::ETAG], "W/\"abc\"");
        let out = text(response).await;
        assert_eq!(
            out,
            format!(
                "<html><body><script>var s='</body>';</script><p>hi</p>{SNIPPET}</body>\n</html>"
            )
        );
    }

    #[tokio::test]
    async fn streams_before_the_end() {
        let body = chunked(vec![
            b"<html><body>first chunk that is long enough",
            b"</body></html>",
        ]);
        let mut injected = InjectBody::new(body, Bytes::from_static(b"X"), 1024);
        let first = injected
            .frame()
            .await
            .unwrap()
            .unwrap()
            .into_data()
            .unwrap();
        assert!(first.starts_with(b"<html><body>first chunk"));
    }

    #[tokio::test]
    async fn leaves_non_html_and_pages_without_body_tag() {
        let mut response = Response::new(full("{\"a\":1}"));
        response.headers_mut().insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        let out = text(apply(&Injection::new(SNIPPET), StatusCode::OK, response).await).await;
        assert_eq!(out, "{\"a\":1}");

        let body = chunked(vec![b"<p>fragment</p>"]);
        let out = text(
            apply(
                &Injection::new(SNIPPET),
                StatusCode::OK,
                html_response(body),
            )
            .await,
        )
        .await;
        assert_eq!(out, "<p>fragment</p>");
    }

    #[tokio::test]
    async fn gives_up_when_too_much_follows_body() {
        let body = chunked(vec![
            b"<body></body>",
            b"0123456789012345678901234567890123456789",
        ]);
        let injection = Injection {
            snippet: SNIPPET.into(),
            max_html_bytes: 16,
        };
        let out = text(apply(&injection, StatusCode::OK, html_response(body)).await).await;
        assert_eq!(out, "<body></body>0123456789012345678901234567890123456789");
    }

    #[tokio::test]
    async fn decompresses_gzip_and_injects() {
        let page = b"<html><body><h1>Hi</h1></body></html>";
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(page).unwrap();
        let gz = encoder.finish().unwrap();
        let mut response = html_response(full(gz.clone()));
        response
            .headers_mut()
            .insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        response
            .headers_mut()
            .insert(header::CONTENT_LENGTH, HeaderValue::from(gz.len()));
        let response = apply(&Injection::new(SNIPPET), StatusCode::OK, response).await;
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
        let len: usize = response.headers()[header::CONTENT_LENGTH]
            .to_str()
            .unwrap()
            .parse()
            .unwrap();
        let out = text(response).await;
        assert_eq!(len, out.len());
        assert_eq!(
            out,
            format!("<html><body><h1>Hi</h1>{SNIPPET}</body></html>")
        );
    }

    #[test]
    fn navigation_detection() {
        let mut headers = HeaderMap::new();
        headers.insert("sec-fetch-dest", HeaderValue::from_static("document"));
        assert!(wants_injection(&Method::GET, &headers));
        assert!(!wants_injection(&Method::POST, &headers));
        headers.insert("sec-fetch-dest", HeaderValue::from_static("empty"));
        headers.insert(header::ACCEPT, HeaderValue::from_static("text/html"));
        assert!(!wants_injection(&Method::GET, &headers));
        let mut old = HeaderMap::new();
        old.insert(header::ACCEPT, HeaderValue::from_static("text/html,*/*"));
        assert!(wants_injection(&Method::GET, &old));
        old.insert("hx-request", HeaderValue::from_static("true"));
        assert!(!wants_injection(&Method::GET, &old));
    }
}
