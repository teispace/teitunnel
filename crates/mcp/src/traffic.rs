//! Captured HTTP traffic, as the traffic tools need it.
//!
//! The inspector (`crates/lens`, M12-02) captures each request/response exchange of a
//! share or route. This module is the seam between it and the MCP server:
//! [`TrafficSource`] is what the `traffic_*` and `wait_for_request` tools call, and
//! [`NoTraffic`] stands in until the inspector is wired up (every call says the
//! inspector isn't running). Secrets are masked by the server (not the source), so a
//! source returns headers and bodies as captured.

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::backend::BoxFuture;

/// Why a traffic call failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TrafficError {
    /// Nothing is being captured (the inspector isn't running for anything).
    #[error("{0}")]
    NotRunning(String),
    /// No exchange with this id (it may have left the capture ring).
    #[error(
        "No captured request with id {0}. It may have been dropped from the capture (only the most recent are kept); list them again with traffic_list."
    )]
    NotFound(String),
    /// The request is invalid (e.g. an edit the origin can't take).
    #[error("{0}")]
    Invalid(String),
    /// Anything else, in a sentence.
    #[error("{0}")]
    Other(String),
}

/// Which exchanges.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct TrafficFilter {
    /// Only this share or route: a Quick Share id, a share or route hostname, or a
    /// public URL. Omit for all captured traffic.
    pub scope: Option<String>,
    /// HTTP method, e.g. `POST`.
    pub method: Option<String>,
    /// Text the path (with query string) must contain, e.g. `/webhooks/stripe`.
    pub path_contains: Option<String>,
    /// An exact status (`404`) or a class (`2xx`, `4xx`, `5xx`).
    pub status: Option<String>,
    /// Only exchanges that took at least this long, in milliseconds.
    pub min_duration_ms: Option<u64>,
    /// Text that must appear in a header or body (case-insensitive).
    pub text: Option<String>,
    /// Only exchanges that started at or after this time (milliseconds since the epoch).
    pub since_ms: Option<u64>,
}

impl TrafficFilter {
    /// Whether `summary` matches the fields a summary can answer (everything but
    /// `text`, which needs the full exchange).
    pub fn matches_summary(&self, summary: &ExchangeSummary) -> bool {
        let method = self
            .method
            .as_deref()
            .is_none_or(|m| m.eq_ignore_ascii_case(&summary.method));
        let path = self
            .path_contains
            .as_deref()
            .is_none_or(|p| summary.path.contains(p));
        let status = self.status.as_deref().is_none_or(|wanted| {
            let Some(status) = summary.status else {
                return false;
            };
            let wanted = wanted.trim().to_ascii_lowercase();
            match wanted.strip_suffix("xx") {
                Some(class) => class.parse::<u16>().is_ok_and(|c| status / 100 == c),
                None => wanted.parse::<u16>().is_ok_and(|s| s == status),
            }
        });
        let duration = self
            .min_duration_ms
            .is_none_or(|min| summary.duration_ms.is_some_and(|d| d >= min));
        let since = self
            .since_ms
            .is_none_or(|since| summary.started_at_ms >= since);
        method && path && status && duration && since
    }
}

/// One exchange in a list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeSummary {
    /// Pass to `traffic_get`, `traffic_replay` and `traffic_export`.
    pub id: String,
    /// The share or route it came through (hostname or share id).
    pub scope: String,
    /// When the request arrived (milliseconds since the epoch).
    pub started_at_ms: u64,
    /// HTTP method.
    pub method: String,
    /// The public hostname it was sent to.
    pub host: String,
    /// Path with query string.
    pub path: String,
    /// Response status, once there is one.
    pub status: Option<u16>,
    /// Total time in milliseconds, once complete.
    pub duration_ms: Option<u64>,
    /// Request body size in bytes.
    pub request_bytes: u64,
    /// Response body size in bytes.
    pub response_bytes: u64,
    /// `complete`, `pending` (no response yet) or `error` (the origin failed).
    pub state: String,
}

/// A body, possibly cut short.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Body {
    /// `utf8` (text as is) or `base64` (binary).
    pub encoding: String,
    /// The captured bytes.
    pub data: String,
    /// The whole body's size in bytes.
    pub size: u64,
    /// Only the first part was captured or returned.
    pub truncated: bool,
}

/// A request or response.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HttpMessage {
    /// Headers in order, as `[name, value]` pairs.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Body,
}

/// A full exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Exchange {
    /// The summary.
    pub summary: ExchangeSummary,
    /// What the visitor sent.
    pub request: HttpMessage,
    /// What the origin answered, if it did.
    pub response: Option<HttpMessage>,
    /// Time to the origin's first byte, in milliseconds.
    pub ttfb_ms: Option<u64>,
    /// Why it failed, when the origin didn't answer.
    pub error: Option<String>,
}

/// Changes to make when replaying.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", default)]
pub struct ReplayEdits {
    /// Another method.
    pub method: Option<String>,
    /// Another path (with query string).
    pub path: Option<String>,
    /// Headers to set (replacing any with the same name).
    pub set_headers: Vec<(String, String)>,
    /// Headers to remove, by name.
    pub remove_headers: Vec<String>,
    /// Another body (UTF-8 text).
    pub body: Option<String>,
}

/// A replay's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ReplayResult {
    /// The new exchange (captured like any other).
    pub exchange: ExchangeSummary,
}

/// Numbers over a set of exchanges.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TrafficStats {
    /// Exchanges counted.
    pub count: u64,
    /// By status class: `2xx`, `3xx`, `4xx`, `5xx`, `error`.
    pub by_status: Vec<(String, u64)>,
    /// Median time in milliseconds.
    pub p50_ms: Option<u64>,
    /// 95th percentile.
    pub p95_ms: Option<u64>,
    /// 99th percentile.
    pub p99_ms: Option<u64>,
    /// Bytes received and sent.
    pub request_bytes: u64,
    /// Bytes answered.
    pub response_bytes: u64,
    /// Most requested paths with counts, most first.
    pub top_paths: Vec<(String, u64)>,
}

/// Export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TrafficFormat {
    /// `curl` commands that resend the requests.
    Curl,
    /// An HTTP Archive (HAR 1.2) JSON document.
    Har,
    /// Markdown, for an issue or a chat.
    Markdown,
}

/// One page of exchanges.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrafficPage {
    /// Newest first.
    pub exchanges: Vec<ExchangeSummary>,
    /// Pass back to get the next page.
    pub next_cursor: Option<String>,
}

/// Where captured traffic comes from (the inspector). Implement this to give agents the
/// traffic tools; every method may fail with [`TrafficError::NotRunning`].
pub trait TrafficSource: Send + Sync + 'static {
    /// Exchanges matching `filter`, newest first, at most `limit`, after `cursor`.
    fn list<'a>(
        &'a self,
        filter: &'a TrafficFilter,
        cursor: Option<&'a str>,
        limit: usize,
    ) -> BoxFuture<'a, Result<TrafficPage, TrafficError>>;

    /// One exchange in full, bodies cut at `body_limit` bytes each.
    fn get<'a>(
        &'a self,
        id: &'a str,
        body_limit: usize,
    ) -> BoxFuture<'a, Result<Exchange, TrafficError>>;

    /// Sends a captured request to the origin again (with `edits`), `times` times in a
    /// row. The replays are captured like any other request.
    fn replay<'a>(
        &'a self,
        id: &'a str,
        edits: &'a ReplayEdits,
        times: u32,
    ) -> BoxFuture<'a, Result<Vec<ReplayResult>, TrafficError>>;

    /// Resolves with the first exchange matching `filter` that starts at or after
    /// `filter.since_ms` (the caller sets it; an exchange captured before the call
    /// but after `since_ms` counts). Never resolves if none arrives: the caller bounds
    /// the wait and can cancel it.
    fn next_matching<'a>(
        &'a self,
        filter: &'a TrafficFilter,
    ) -> BoxFuture<'a, Result<ExchangeSummary, TrafficError>>;

    /// Numbers over the exchanges matching `filter`.
    fn stats<'a>(
        &'a self,
        filter: &'a TrafficFilter,
    ) -> BoxFuture<'a, Result<TrafficStats, TrafficError>>;

    /// An OpenAPI 3.1 description of the captured traffic (to `host`, or all), and what
    /// went into it.
    fn openapi<'a>(
        &'a self,
        _host: Option<&'a str>,
        _title: Option<&'a str>,
    ) -> BoxFuture<'a, Result<(serde_json::Value, teitunnel_core::openapi::Summary), TrafficError>>
    {
        not_running()
    }

    /// The exchanges `ids` as `format`. `mask(name, value)` returns the header value to
    /// write (it masks credentials unless the server allows secrets). The default renders
    /// from [`TrafficSource::get`]; a source with richer exporters may override it.
    fn export<'a>(
        &'a self,
        ids: &'a [String],
        format: TrafficFormat,
        mask: &'a (dyn Fn(&str, &str) -> String + Send + Sync),
    ) -> BoxFuture<'a, Result<String, TrafficError>> {
        Box::pin(async move {
            let mut exchanges = Vec::with_capacity(ids.len());
            for id in ids {
                exchanges.push(self.get(id, EXPORT_BODY_LIMIT).await?);
            }
            Ok(render(&exchanges, format, mask))
        })
    }
}

/// Bodies in exports are cut here.
pub const EXPORT_BODY_LIMIT: usize = 64 * 1024;

/// The stand-in until the inspector runs: every call explains it isn't running.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTraffic;

/// What [`NoTraffic`] says.
pub const NOT_RUNNING: &str = "The traffic inspector isn't running, so no requests are captured yet. Traffic is captured for shares and routes with inspection on (Teitunnel's inspector); share with share_port and try again once inspection is available.";

fn not_running<T: Send + 'static>() -> BoxFuture<'static, Result<T, TrafficError>> {
    Box::pin(async { Err(TrafficError::NotRunning(NOT_RUNNING.to_owned())) })
}

impl TrafficSource for NoTraffic {
    fn list<'a>(
        &'a self,
        _filter: &'a TrafficFilter,
        _cursor: Option<&'a str>,
        _limit: usize,
    ) -> BoxFuture<'a, Result<TrafficPage, TrafficError>> {
        not_running()
    }

    fn get<'a>(
        &'a self,
        _id: &'a str,
        _body_limit: usize,
    ) -> BoxFuture<'a, Result<Exchange, TrafficError>> {
        not_running()
    }

    fn replay<'a>(
        &'a self,
        _id: &'a str,
        _edits: &'a ReplayEdits,
        _times: u32,
    ) -> BoxFuture<'a, Result<Vec<ReplayResult>, TrafficError>> {
        not_running()
    }

    fn next_matching<'a>(
        &'a self,
        _filter: &'a TrafficFilter,
    ) -> BoxFuture<'a, Result<ExchangeSummary, TrafficError>> {
        not_running()
    }

    fn stats<'a>(
        &'a self,
        _filter: &'a TrafficFilter,
    ) -> BoxFuture<'a, Result<TrafficStats, TrafficError>> {
        not_running()
    }
}

fn header<'a>(message: &'a HttpMessage, name: &str) -> Option<&'a str> {
    message
        .headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

/// Single-quotes `value` for a POSIX shell.
fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

fn body_text(body: &Body) -> Option<&str> {
    (body.encoding == "utf8" && !body.data.is_empty()).then_some(body.data.as_str())
}

/// Renders exchanges as `format`, masking header values with `mask`.
pub fn render(
    exchanges: &[Exchange],
    format: TrafficFormat,
    mask: &(dyn Fn(&str, &str) -> String + Send + Sync),
) -> String {
    match format {
        TrafficFormat::Curl => exchanges
            .iter()
            .map(|e| curl(e, mask))
            .collect::<Vec<_>>()
            .join("\n\n"),
        TrafficFormat::Markdown => exchanges
            .iter()
            .map(|e| markdown(e, mask))
            .collect::<Vec<_>>()
            .join("\n---\n\n"),
        TrafficFormat::Har => har(exchanges, mask),
    }
}

fn curl(exchange: &Exchange, mask: &(dyn Fn(&str, &str) -> String + Send + Sync)) -> String {
    let s = &exchange.summary;
    let mut out = format!(
        "curl -X {} {}",
        s.method,
        quote(&format!("https://{}{}", s.host, s.path))
    );
    for (name, value) in &exchange.request.headers {
        if name.eq_ignore_ascii_case("host") || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        let _ = write!(
            out,
            " \\\n  -H {}",
            quote(&format!("{name}: {}", mask(name, value)))
        );
    }
    if let Some(body) = body_text(&exchange.request.body) {
        let _ = write!(out, " \\\n  --data-raw {}", quote(body));
    }
    out
}

fn markdown(exchange: &Exchange, mask: &(dyn Fn(&str, &str) -> String + Send + Sync)) -> String {
    let s = &exchange.summary;
    let status = s
        .status
        .map_or_else(|| "no response".to_owned(), |st| st.to_string());
    let mut out = format!(
        "### {} {} → {status}\n\nHost: `{}` · {} ms\n\n**Request headers**\n\n```http\n",
        s.method,
        s.path,
        s.host,
        s.duration_ms
            .map_or_else(|| "?".to_owned(), |d| d.to_string()),
    );
    for (name, value) in &exchange.request.headers {
        let _ = writeln!(out, "{name}: {}", mask(name, value));
    }
    out.push_str("```\n");
    if let Some(body) = body_text(&exchange.request.body) {
        let _ = write!(out, "\n**Request body**\n\n```\n{body}\n```\n");
    }
    if let Some(response) = &exchange.response {
        out.push_str("\n**Response headers**\n\n```http\n");
        for (name, value) in &response.headers {
            let _ = writeln!(out, "{name}: {}", mask(name, value));
        }
        out.push_str("```\n");
        if let Some(body) = body_text(&response.body) {
            let _ = write!(out, "\n**Response body**\n\n```\n{body}\n```\n");
        }
    }
    if let Some(error) = &exchange.error {
        let _ = write!(out, "\nError: {error}\n");
    }
    out
}

fn har(exchanges: &[Exchange], mask: &(dyn Fn(&str, &str) -> String + Send + Sync)) -> String {
    let headers = |message: &HttpMessage| -> Vec<serde_json::Value> {
        message
            .headers
            .iter()
            .map(|(n, v)| serde_json::json!({ "name": n, "value": mask(n, v) }))
            .collect()
    };
    let entries: Vec<serde_json::Value> = exchanges
        .iter()
        .map(|e| {
            let s = &e.summary;
            let response = e.response.as_ref();
            serde_json::json!({
                "startedDateTime": s.started_at_ms,
                "time": s.duration_ms.unwrap_or(0),
                "request": {
                    "method": s.method,
                    "url": format!("https://{}{}", s.host, s.path),
                    "httpVersion": "HTTP/1.1",
                    "headers": headers(&e.request),
                    "queryString": [],
                    "cookies": [],
                    "headersSize": -1,
                    "bodySize": s.request_bytes,
                    "postData": body_text(&e.request.body).map(|text| serde_json::json!({
                        "mimeType": header(&e.request, "content-type").unwrap_or(""),
                        "text": text,
                    })),
                },
                "response": {
                    "status": s.status.unwrap_or(0),
                    "statusText": "",
                    "httpVersion": "HTTP/1.1",
                    "headers": response.map(headers).unwrap_or_default(),
                    "cookies": [],
                    "content": {
                        "size": s.response_bytes,
                        "mimeType": response.and_then(|r| header(r, "content-type")).unwrap_or(""),
                        "text": response.and_then(|r| body_text(&r.body)).unwrap_or(""),
                    },
                    "redirectURL": "",
                    "headersSize": -1,
                    "bodySize": s.response_bytes,
                },
                "cache": {},
                "timings": { "send": 0, "wait": e.ttfb_ms.unwrap_or(0), "receive": 0 },
            })
        })
        .collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "Teitunnel", "version": env!("CARGO_PKG_VERSION") },
            "entries": entries,
        }
    }))
    .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn exchange() -> Exchange {
        Exchange {
            summary: ExchangeSummary {
                id: "ex-1".into(),
                scope: "demo.xyz.com".into(),
                started_at_ms: 1_000,
                method: "POST".into(),
                host: "demo.xyz.com".into(),
                path: "/webhooks/stripe?x=1".into(),
                status: Some(500),
                duration_ms: Some(42),
                request_bytes: 13,
                response_bytes: 5,
                state: "complete".into(),
            },
            request: HttpMessage {
                headers: vec![
                    ("Content-Type".into(), "application/json".into()),
                    ("Stripe-Signature".into(), "t=1,v1=abc".into()),
                ],
                body: Body {
                    encoding: "utf8".into(),
                    data: r#"{"it's":true}"#.into(),
                    size: 13,
                    truncated: false,
                },
            },
            response: Some(HttpMessage {
                headers: vec![("Content-Type".into(), "text/plain".into())],
                body: Body {
                    encoding: "utf8".into(),
                    data: "oops!".into(),
                    size: 5,
                    truncated: false,
                },
            }),
            ttfb_ms: Some(40),
            error: None,
        }
    }

    fn mask(name: &str, value: &str) -> String {
        crate::redaction::header_value(name, value, false)
    }

    #[test]
    fn filters_summaries() {
        let s = exchange().summary;
        let filter = |f: TrafficFilter| f.matches_summary(&s);
        assert!(filter(TrafficFilter::default()));
        assert!(filter(TrafficFilter {
            method: Some("post".into()),
            ..Default::default()
        }));
        assert!(filter(TrafficFilter {
            status: Some("5xx".into()),
            ..Default::default()
        }));
        assert!(!filter(TrafficFilter {
            status: Some("2xx".into()),
            ..Default::default()
        }));
        assert!(filter(TrafficFilter {
            status: Some("500".into()),
            ..Default::default()
        }));
        assert!(filter(TrafficFilter {
            path_contains: Some("/webhooks".into()),
            ..Default::default()
        }));
        assert!(!filter(TrafficFilter {
            since_ms: Some(2_000),
            ..Default::default()
        }));
        assert!(!filter(TrafficFilter {
            min_duration_ms: Some(100),
            ..Default::default()
        }));
    }

    #[test]
    fn renders_curl_with_masked_secrets() {
        let out = render(&[exchange()], TrafficFormat::Curl, &mask);
        assert!(
            out.starts_with("curl -X POST 'https://demo.xyz.com/webhooks/stripe?x=1'"),
            "{out}"
        );
        assert!(out.contains("-H 'Stripe-Signature: [masked]'"), "{out}");
        assert!(out.contains(r#"--data-raw '{"it'\''s":true}'"#), "{out}");
    }

    #[test]
    fn renders_markdown_and_har() {
        let md = render(&[exchange()], TrafficFormat::Markdown, &mask);
        assert!(
            md.starts_with("### POST /webhooks/stripe?x=1 → 500"),
            "{md}"
        );
        assert!(!md.contains("v1=abc"));
        let har: serde_json::Value =
            serde_json::from_str(&render(&[exchange()], TrafficFormat::Har, &mask)).unwrap();
        assert_eq!(har["log"]["version"], "1.2");
        assert_eq!(har["log"]["entries"][0]["response"]["status"], 500);
        assert_eq!(
            har["log"]["entries"][0]["request"]["headers"][1]["value"],
            "[masked]"
        );
    }

    #[tokio::test]
    async fn the_stand_in_says_the_inspector_isnt_running() {
        let source = NoTraffic;
        let err = source
            .list(&TrafficFilter::default(), None, 10)
            .await
            .unwrap_err();
        assert!(matches!(err, TrafficError::NotRunning(_)));
        let err = source
            .export(&["x".to_owned()], TrafficFormat::Curl, &mask)
            .await
            .unwrap_err();
        assert!(matches!(err, TrafficError::NotRunning(_)));
    }
}
