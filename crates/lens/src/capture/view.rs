//! Serializable, redaction-aware views of captured exchanges (for UIs, IPC and agents).

use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, header};
use serde::Serialize;

use super::{
    BodyRecord, ClientInfo, ContentKind, Exchange, ExchangeError, ExchangeKind, ExchangeState,
    Responder, StreamStats, Timings, content_kind, decode_body,
};
use crate::{
    ExchangeId, TapId,
    redact::{mask_header, mask_json, mask_query, mask_text},
};

/// How secrets are treated when captured data is read.
///
/// The default (and [`Redaction::masked`]) masks credentials, signatures and token-like
/// values; [`Redaction::revealed`] shows raw data and should only follow an explicit
/// user action ("click to reveal", "untick redact").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redaction {
    /// Whether to mask at all.
    pub mask: bool,
    /// Also mask email addresses in text.
    pub emails: bool,
    /// Extra header names to mask (case-insensitive).
    pub extra_headers: Vec<String>,
}

impl Redaction {
    /// Mask secrets (the default).
    pub fn masked() -> Self {
        Self {
            mask: true,
            emails: false,
            extra_headers: Vec::new(),
        }
    }

    /// Show raw data.
    pub fn revealed() -> Self {
        Self {
            mask: false,
            emails: false,
            extra_headers: Vec::new(),
        }
    }
}

impl Default for Redaction {
    fn default() -> Self {
        Self::masked()
    }
}

/// One header in a view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeaderView {
    /// Name, lowercase.
    pub name: String,
    /// Value (lossy UTF-8), masked per the redaction.
    pub value: String,
}

/// A body in a view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyView {
    /// Bytes on the wire.
    pub size: u64,
    /// Bytes captured.
    pub captured: u64,
    /// Whether the capture is shorter than the body.
    pub truncated: bool,
    /// Whether the body has finished.
    pub complete: bool,
    /// Classification of the decoded body.
    pub kind: ContentKind,
    /// `Content-Type`, if any.
    pub content_type: Option<String>,
    /// `Content-Encoding`, if any.
    pub encoding: Option<String>,
    /// Text bodies: decoded, lossy UTF-8, masked.
    pub text: Option<String>,
    /// Binary bodies: decoded, base64. Never masked (binary can't be masked reliably).
    pub base64: Option<String>,
    /// Why decompression failed, if it did (then `base64` holds the raw bytes).
    pub decode_error: Option<String>,
}

/// The request part of a view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestView {
    /// Method.
    pub method: String,
    /// Full URL, masked.
    pub url: String,
    /// Path, masked.
    pub path: String,
    /// Query string, masked.
    pub query: Option<String>,
    /// Host.
    pub host: String,
    /// `HTTP/1.1`, `HTTP/2`…
    pub http_version: String,
    /// Headers in order.
    pub headers: Vec<HeaderView>,
    /// Body.
    pub body: BodyView,
}

/// The response part of a view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResponseView {
    /// Status code.
    pub status: u16,
    /// Canonical reason phrase.
    pub status_text: String,
    /// `HTTP/1.1`, `HTTP/2`…
    pub http_version: String,
    /// Headers in order.
    pub headers: Vec<HeaderView>,
    /// Body.
    pub body: BodyView,
}

/// A serializable, redaction-aware copy of an [`Exchange`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeView {
    /// Id.
    pub id: ExchangeId,
    /// Per-tap sequence number.
    pub seq: u64,
    /// Tap.
    pub tap: TapId,
    /// Kind.
    pub kind: ExchangeKind,
    /// State.
    pub state: ExchangeState,
    /// Unix milliseconds.
    pub started_at_ms: u64,
    /// Duration in milliseconds, when known.
    pub duration_ms: Option<f64>,
    /// Timings.
    pub timings: Timings,
    /// Client.
    pub client: ClientInfo,
    /// Request.
    pub request: RequestView,
    /// Response.
    pub response: Option<ResponseView>,
    /// Who answered.
    pub responder: Responder,
    /// Error.
    pub error: Option<ExchangeError>,
    /// Stream summary (previews masked).
    pub stream: Option<StreamStats>,
    /// Replayed exchange.
    pub replay_of: Option<ExchangeId>,
    /// Whether secrets are masked in this view.
    pub redacted: bool,
}

pub(super) fn build(exchange: &Exchange, redaction: &Redaction) -> ExchangeView {
    let request = &exchange.request;
    let path = mask_text(request.path(), redaction).into_owned();
    let query = request
        .query()
        .map(|query| mask_query(query, redaction).into_owned());
    let url = format!(
        "{}://{}{}{}",
        request.scheme,
        request.host,
        path,
        query
            .as_deref()
            .map(|q| format!("?{q}"))
            .unwrap_or_default()
    );
    let stream = exchange.stream.as_ref().map(|stream| {
        let mut stream = stream.clone();
        for preview in &mut stream.previews {
            preview.preview = mask_json(&preview.preview, redaction).into_owned();
        }
        stream
    });
    ExchangeView {
        id: exchange.id,
        seq: exchange.seq,
        tap: exchange.tap.clone(),
        kind: exchange.kind,
        state: exchange.state,
        started_at_ms: exchange.started_at_ms,
        duration_ms: exchange
            .duration()
            .map(|duration| duration.as_secs_f64() * 1_000.0),
        timings: exchange.timings,
        client: exchange.client.clone(),
        request: RequestView {
            method: request.method.to_string(),
            url,
            path,
            query,
            host: request.host.clone(),
            http_version: version_text(request.version).to_owned(),
            headers: headers_view(&request.headers, redaction),
            body: body_view(&request.headers, &request.body, redaction),
        },
        response: exchange.response.as_ref().map(|response| ResponseView {
            status: response.status.as_u16(),
            status_text: response
                .status
                .canonical_reason()
                .unwrap_or_default()
                .to_owned(),
            http_version: version_text(response.version).to_owned(),
            headers: headers_view(&response.headers, redaction),
            body: body_view(&response.headers, &response.body, redaction),
        }),
        responder: exchange.responder.clone(),
        error: exchange.error.clone(),
        stream,
        replay_of: exchange.replay_of,
        redacted: redaction.mask,
    }
}

/// `HTTP/1.1`, `HTTP/2`, …
pub(crate) fn version_text(version: http::Version) -> &'static str {
    match version {
        http::Version::HTTP_09 => "HTTP/0.9",
        http::Version::HTTP_10 => "HTTP/1.0",
        http::Version::HTTP_2 => "HTTP/2",
        http::Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/1.1",
    }
}

pub(crate) fn headers_view(headers: &HeaderMap, redaction: &Redaction) -> Vec<HeaderView> {
    headers
        .iter()
        .map(|(name, value)| {
            let raw = String::from_utf8_lossy(value.as_bytes());
            HeaderView {
                name: name.as_str().to_owned(),
                value: mask_header(name.as_str(), &raw, redaction).into_owned(),
            }
        })
        .collect()
}

/// A body decoded and masked for display. Also used by exports.
pub(crate) fn body_view(headers: &HeaderMap, body: &BodyRecord, redaction: &Redaction) -> BodyView {
    let header_text = |name: header::HeaderName| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let (decoded, decode_error) = match decode_body(headers, &body.data) {
        Ok(decoded) => (decoded, None),
        Err(err) => (
            std::borrow::Cow::Borrowed(&body.data[..]),
            Some(err.to_string()),
        ),
    };
    let kind = if decode_error.is_some() {
        ContentKind::Binary
    } else {
        content_kind(headers, &decoded)
    };
    let (text, base64) = if kind == ContentKind::Empty {
        (None, None)
    } else if kind.is_text() {
        let raw = String::from_utf8_lossy(&decoded);
        let masked = if kind == ContentKind::Form {
            mask_query(&raw, redaction).into_owned()
        } else {
            mask_json(&raw, redaction).into_owned()
        };
        (Some(masked), None)
    } else {
        (None, Some(STANDARD.encode(&decoded)))
    };
    BodyView {
        size: body.size,
        captured: body.data.len() as u64,
        truncated: body.truncated,
        complete: body.complete,
        kind,
        content_type: header_text(header::CONTENT_TYPE),
        encoding: header_text(header::CONTENT_ENCODING),
        text,
        base64,
        decode_error,
    }
}
