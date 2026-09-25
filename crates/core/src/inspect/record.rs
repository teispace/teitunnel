//! Captured exchanges as database rows, for the history kept across restarts.
//!
//! The local database holds no secrets (SECURITY_MODEL), so a row is the *masked*
//! capture: credential headers keep only their scheme or cookie names, secret-named
//! query and form values and token-like strings are replaced, and text bodies are
//! stored decoded and masked (their `Content-Encoding` header is dropped). Binary bodies
//! are kept as captured. Bodies are cut at [`PERSIST_BODY_BYTES`].

use std::str::FromStr;

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, Version};
use lens::{
    BodyRecord, ClientInfo, ContentKind, Exchange, ExchangeError, ExchangeId, ExchangeKind,
    ExchangeState, FaultRecord, Redaction, RequestRecord, Responder, ResponseRecord, StreamStats,
    TapId, Timings, content_kind, decode_body, mask_header, mask_json, mask_query, mask_text,
};
use serde::{Deserialize, Serialize};

/// Bytes of each body kept in the history.
pub const PERSIST_BODY_BYTES: usize = 64 * 1024;

/// One exchange as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    pub(crate) id: String,
    pub(crate) tap: String,
    pub(crate) seq: i64,
    pub(crate) started_at: i64,
    pub(crate) method: String,
    pub(crate) host: String,
    pub(crate) path: String,
    pub(crate) status: Option<i64>,
    pub(crate) kind: String,
    pub(crate) meta: String,
    pub(crate) request_body: Vec<u8>,
    pub(crate) response_body: Vec<u8>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BodyMeta {
    size: u64,
    truncated: bool,
    complete: bool,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ResponseMeta {
    status: u16,
    version: String,
    headers: Vec<(String, String)>,
    body: BodyMeta,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    kind: ExchangeKind,
    state: ExchangeState,
    timings: Timings,
    client: ClientInfo,
    uri: String,
    scheme: String,
    host: String,
    version: String,
    request_headers: Vec<(String, String)>,
    request_body: BodyMeta,
    response: Option<ResponseMeta>,
    responder: Responder,
    error: Option<ExchangeError>,
    stream: Option<StreamStats>,
    replay_of: Option<ExchangeId>,
    fault: Option<FaultRecord>,
}

fn version_text(version: Version) -> &'static str {
    match version {
        Version::HTTP_09 => "HTTP/0.9",
        Version::HTTP_10 => "HTTP/1.0",
        Version::HTTP_2 => "HTTP/2",
        Version::HTTP_3 => "HTTP/3",
        _ => "HTTP/1.1",
    }
}

fn version_of(text: &str) -> Version {
    match text {
        "HTTP/0.9" => Version::HTTP_09,
        "HTTP/1.0" => Version::HTTP_10,
        "HTTP/2" => Version::HTTP_2,
        "HTTP/3" => Version::HTTP_3,
        _ => Version::HTTP_11,
    }
}

fn kind_text(kind: ExchangeKind) -> &'static str {
    match kind {
        ExchangeKind::Http => "http",
        ExchangeKind::WebSocket => "webSocket",
        ExchangeKind::Sse => "sse",
        ExchangeKind::Upgrade => "upgrade",
    }
}

/// Masked headers; `decoded` drops `Content-Encoding` (the body is stored decoded).
fn masked_headers(headers: &HeaderMap, decoded: bool) -> Vec<(String, String)> {
    let redaction = Redaction::masked();
    headers
        .iter()
        .filter(|(name, _)| !(decoded && *name == http::header::CONTENT_ENCODING))
        .map(|(name, value)| {
            let raw = String::from_utf8_lossy(value.as_bytes());
            (
                name.as_str().to_owned(),
                mask_header(name.as_str(), &raw, &redaction).into_owned(),
            )
        })
        .collect()
}

/// The body to store: decoded and masked text, or the captured bytes; and whether it
/// was decoded.
fn stored_body(headers: &HeaderMap, body: &BodyRecord) -> (Vec<u8>, bool, bool) {
    let redaction = Redaction::masked();
    let (data, decoded) = match decode_body(headers, &body.data) {
        Ok(decoded) => {
            let kind = content_kind(headers, &decoded);
            if kind.is_text() && kind != ContentKind::Empty {
                let text = String::from_utf8_lossy(&decoded);
                let masked = if kind == ContentKind::Form {
                    mask_query(&text, &redaction).into_owned()
                } else {
                    mask_json(&text, &redaction).into_owned()
                };
                let changed = !matches!(decoded, std::borrow::Cow::Borrowed(_));
                (masked.into_bytes(), changed)
            } else {
                (body.data.to_vec(), false)
            }
        }
        Err(_) => (body.data.to_vec(), false),
    };
    let cut = data.len() > PERSIST_BODY_BYTES;
    let mut data = data;
    data.truncate(PERSIST_BODY_BYTES);
    (data, decoded, cut)
}

fn masked_uri(request: &RequestRecord) -> String {
    let redaction = Redaction::masked();
    let path = mask_text(request.path(), &redaction).into_owned();
    match request.query() {
        Some(query) => format!("{path}?{}", mask_query(query, &redaction)),
        None => path,
    }
}

/// The masked row for `exchange`.
pub(crate) fn to_row(exchange: &Exchange) -> Row {
    let request = &exchange.request;
    let (request_body, request_decoded, request_cut) = stored_body(&request.headers, &request.body);
    let response = exchange.response.as_ref().map(|response| {
        let (body, decoded, cut) = stored_body(&response.headers, &response.body);
        (response, body, decoded, cut)
    });
    let uri = masked_uri(request);
    let meta = Meta {
        kind: exchange.kind,
        state: exchange.state,
        timings: exchange.timings,
        client: exchange.client.clone(),
        uri: uri.clone(),
        scheme: request.scheme.clone(),
        host: request.host.clone(),
        version: version_text(request.version).to_owned(),
        request_headers: masked_headers(&request.headers, request_decoded),
        request_body: BodyMeta {
            size: request.body.size,
            truncated: request.body.truncated || request_cut,
            complete: request.body.complete,
        },
        response: response
            .as_ref()
            .map(|(response, _, decoded, cut)| ResponseMeta {
                status: response.status.as_u16(),
                version: version_text(response.version).to_owned(),
                headers: masked_headers(&response.headers, *decoded),
                body: BodyMeta {
                    size: response.body.size,
                    truncated: response.body.truncated || *cut,
                    complete: response.body.complete,
                },
            }),
        responder: exchange.responder.clone(),
        error: exchange.error.clone(),
        stream: exchange.stream.as_ref().map(|stream| {
            let mut stream = stream.clone();
            let redaction = Redaction::masked();
            for preview in &mut stream.previews {
                preview.preview = mask_json(&preview.preview, &redaction).into_owned();
            }
            for frame in &mut stream.frames {
                if let Some(preview) = frame.preview.as_mut() {
                    *preview = mask_json(preview, &redaction).into_owned();
                }
            }
            stream
        }),
        replay_of: exchange.replay_of,
        fault: exchange.fault.clone(),
    };
    let path = uri.split('?').next().unwrap_or("/").to_owned();
    Row {
        id: exchange.id.to_string(),
        tap: exchange.tap.to_string(),
        seq: i64::try_from(exchange.seq).unwrap_or(i64::MAX),
        started_at: i64::try_from(exchange.started_at_ms).unwrap_or(i64::MAX),
        method: request.method.to_string(),
        host: request.host.clone(),
        path,
        status: exchange.status().map(|s| i64::from(s.as_u16())),
        kind: kind_text(exchange.kind).to_owned(),
        meta: serde_json::to_string(&meta).unwrap_or_default(),
        request_body,
        response_body: response.map(|(_, body, _, _)| body).unwrap_or_default(),
    }
}

fn headers_of(pairs: &[(String, String)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        if let (Ok(name), Ok(value)) = (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(value),
        ) {
            headers.append(name, value);
        }
    }
    headers
}

/// Parses a stored request target, percent-encoding what `Uri` refuses (masks contain
/// brackets).
fn uri_of(text: &str) -> Uri {
    if let Ok(uri) = Uri::from_str(text) {
        return uri;
    }
    let encoded: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "/?&=-._~%+,;:@!$'()*".contains(c) {
                c.to_string()
            } else {
                let mut buf = [0u8; 4];
                c.encode_utf8(&mut buf)
                    .bytes()
                    .map(|b| format!("%{b:02X}"))
                    .collect()
            }
        })
        .collect();
    Uri::from_str(&encoded).unwrap_or_else(|_| Uri::from_static("/"))
}

/// The exchange a row holds (masked, as stored).
pub(crate) fn from_row(row: &Row) -> Option<Exchange> {
    let meta: Meta = serde_json::from_str(&row.meta).ok()?;
    let id = ExchangeId::from_str(&row.id).ok()?;
    let tap = TapId::new(&row.tap).ok()?;
    let body = |data: &[u8], meta: &BodyMeta| BodyRecord {
        data: Bytes::copy_from_slice(data),
        size: meta.size,
        truncated: meta.truncated,
        complete: meta.complete,
    };
    let response = meta.response.as_ref().map(|response| ResponseRecord {
        status: StatusCode::from_u16(response.status).unwrap_or(StatusCode::OK),
        version: version_of(&response.version),
        headers: headers_of(&response.headers),
        body: body(&row.response_body, &response.body),
    });
    Some(Exchange {
        id,
        seq: u64::try_from(row.seq).unwrap_or_default(),
        tap,
        kind: meta.kind,
        state: meta.state,
        started_at_ms: u64::try_from(row.started_at).unwrap_or_default(),
        timings: meta.timings,
        client: meta.client,
        request: RequestRecord {
            method: Method::from_bytes(row.method.as_bytes()).unwrap_or(Method::GET),
            uri: uri_of(&meta.uri),
            scheme: meta.scheme,
            host: meta.host,
            version: version_of(&meta.version),
            headers: headers_of(&meta.request_headers),
            body: body(&row.request_body, &meta.request_body),
        },
        response,
        responder: meta.responder,
        error: meta.error,
        stream: meta.stream,
        replay_of: meta.replay_of,
        fault: meta.fault,
    })
}

/// Whether a header value was masked in the history (such headers are left out of a
/// replay from history).
pub fn is_masked(value: &str) -> bool {
    value.contains(lens::MASK)
}

#[cfg(test)]
pub(crate) mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::*;

    pub(crate) fn sample(tap: &str, seq: u64, path: &str, status: u16) -> Exchange {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("app.example.com"));
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer topsecret-token"),
        );
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        Exchange {
            id: ExchangeId::from(uuid::Uuid::now_v7()),
            seq,
            tap: TapId::new(tap).unwrap(),
            kind: ExchangeKind::Http,
            state: ExchangeState::Complete,
            started_at_ms: crate::domain_shares::now_ms(),
            timings: Timings {
                first_byte_us: Some(2_000),
                complete_us: Some(seq * 1_000 + 3_000),
                ..Timings::default()
            },
            client: ClientInfo {
                ip: IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7)),
                peer: SocketAddr::from(([127, 0, 0, 1], 5000)),
                cf_ray: Some("abc-AMS".into()),
                country: Some("NL".into()),
            },
            request: RequestRecord {
                method: Method::POST,
                uri: path.parse::<Uri>().unwrap(),
                scheme: "https".into(),
                host: "app.example.com".into(),
                version: Version::HTTP_11,
                headers,
                body: BodyRecord::full(Bytes::from_static(
                    br#"{"hello":"world","password":"hunter2"}"#,
                )),
            },
            response: Some(ResponseRecord {
                status: StatusCode::from_u16(status).unwrap(),
                version: Version::HTTP_11,
                headers: HeaderMap::new(),
                body: BodyRecord::full(Bytes::from_static(b"ok")),
            }),
            responder: Responder::Upstream,
            error: None,
            stream: None,
            replay_of: None,
            fault: None,
        }
    }

    #[test]
    fn rows_are_masked_and_round_trip() {
        let exchange = sample("t1", 3, "/hooks?token=abc123&x=1", 201);
        let row = to_row(&exchange);
        assert_eq!(row.method, "POST");
        assert_eq!(row.path, "/hooks");
        assert_eq!(row.status, Some(201));
        let stored = format!("{}{}", row.meta, String::from_utf8_lossy(&row.request_body));
        for secret in ["topsecret-token", "hunter2", "abc123"] {
            assert!(!stored.contains(secret), "{secret} leaked: {stored}");
        }
        let back = from_row(&row).unwrap();
        assert_eq!(back.id, exchange.id);
        assert_eq!(back.seq, 3);
        assert_eq!(back.request.method, Method::POST);
        assert_eq!(back.status(), Some(StatusCode::CREATED));
        assert_eq!(back.request.host, "app.example.com");
        assert!(back.request.query().unwrap().contains("x=1"));
        let auth = back.request.headers["authorization"].to_str().unwrap();
        assert!(auth.starts_with("Bearer ") && is_masked(auth), "{auth}");
        assert!(
            String::from_utf8_lossy(&back.request.body.data).contains("world"),
            "the rest of the body stays"
        );
        assert_eq!(back.client.ip, exchange.client.ip);
    }

    #[test]
    fn long_bodies_are_cut() {
        let mut exchange = sample("t1", 1, "/", 200);
        let big = "a".repeat(PERSIST_BODY_BYTES * 2);
        exchange.request.body = BodyRecord::full(Bytes::from(big));
        let row = to_row(&exchange);
        assert_eq!(row.request_body.len(), PERSIST_BODY_BYTES);
        let back = from_row(&row).unwrap();
        assert!(back.request.body.truncated);
        assert_eq!(back.request.body.size, (PERSIST_BODY_BYTES * 2) as u64);
    }
}
