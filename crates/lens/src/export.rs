//! Exports of captured exchanges: cURL, HTTPie, `fetch`, raw HTTP/1.1, HAR 1.2, JSON
//! and Markdown. Pure functions; every one takes a [`Redaction`] (masked by default in
//! every caller-facing path). Bodies are exported decoded (no `Content-Encoding`).

use std::fmt::Write as _;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{
    Exchange, Redaction,
    capture::{BodyView, ContentKind, HeaderView},
    redact::{is_sensitive_key, mask_text},
    util::iso8601,
};

/// Bodies longer than this are clipped in Markdown (issues and agent context).
const MARKDOWN_BODY_LIMIT: usize = 64 * 1024;

/// Export formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ExportFormat {
    /// A `curl` command.
    Curl,
    /// An HTTPie (`http`) command.
    Httpie,
    /// A JavaScript `fetch` call.
    Fetch,
    /// Raw HTTP/1.1 request and response.
    Raw,
    /// HAR 1.2 (one entry).
    Har,
    /// The exchange view as JSON.
    Json,
    /// Markdown for issues and AI agents.
    Markdown,
}

/// Exports one exchange in `format`.
pub fn export(format: ExportFormat, exchange: &Exchange, redaction: &Redaction) -> String {
    match format {
        ExportFormat::Curl => curl(exchange, redaction),
        ExportFormat::Httpie => httpie(exchange, redaction),
        ExportFormat::Fetch => fetch(exchange, redaction),
        ExportFormat::Raw => raw_http(exchange, redaction),
        ExportFormat::Har => har_string(&[exchange], redaction),
        ExportFormat::Json => json_string(exchange, redaction),
        ExportFormat::Markdown => markdown(exchange, redaction),
    }
}

/// Headers never worth exporting in a request someone will re-run.
fn skip_request_header(name: &str, host_matches_url: bool) -> bool {
    matches!(
        name,
        "content-length"
            | "connection"
            | "keep-alive"
            | "transfer-encoding"
            | "content-encoding"
            | "proxy-connection"
            | "te"
            | "upgrade"
    ) || (name == "host" && host_matches_url)
}

/// The request's headers and body for exports.
struct RequestParts {
    method: String,
    url: String,
    headers: Vec<HeaderView>,
    body: BodyView,
}

fn request_parts(exchange: &Exchange, redaction: &Redaction) -> RequestParts {
    let request = exchange.view(redaction).request;
    let host = request.host.clone();
    let headers = request
        .headers
        .into_iter()
        .filter(|h| !skip_request_header(&h.name, h.name == "host" && h.value == host))
        .collect();
    RequestParts {
        method: request.method,
        url: request.url,
        headers,
        body: request.body,
    }
}

/// Single-quotes for POSIX shells.
fn sh(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn truncation_note(body: &BodyView, comment: &str) -> String {
    if body.truncated || !body.complete {
        format!(
            "{comment} Note: the captured body is incomplete ({} of {} bytes).\n",
            body.captured, body.size
        )
    } else {
        String::new()
    }
}

/// A `curl` command reproducing the request.
pub fn curl(exchange: &Exchange, redaction: &Redaction) -> String {
    let parts = request_parts(exchange, redaction);
    let mut out = truncation_note(&parts.body, "#");
    let mut args = vec!["curl".to_owned()];
    if parts.method != "GET" || parts.body.text.is_some() || parts.body.base64.is_some() {
        args.push(format!("-X {}", parts.method));
    }
    args.push(sh(&parts.url));
    for header in &parts.headers {
        args.push(format!(
            "-H {}",
            sh(&format!("{}: {}", header.name, header.value))
        ));
    }
    match (&parts.body.text, &parts.body.base64) {
        (Some(text), _) => args.push(format!("--data-raw {}", sh(text))),
        (None, Some(b64)) => {
            let _ = write!(out, "printf '%s' {} | base64 --decode | ", sh(b64));
            args.push("--data-binary @-".to_owned());
        }
        (None, None) => {}
    }
    out.push_str(&args.join(" \\\n  "));
    out.push('\n');
    out
}

/// An HTTPie command reproducing the request.
pub fn httpie(exchange: &Exchange, redaction: &Redaction) -> String {
    let parts = request_parts(exchange, redaction);
    let mut out = truncation_note(&parts.body, "#");
    let mut args = vec!["http".to_owned()];
    if let (Some(text), None) = (&parts.body.text, &parts.body.base64) {
        args.push(format!("--raw={}", sh(text)));
    }
    args.push(parts.method.clone());
    args.push(sh(&parts.url));
    for header in &parts.headers {
        let item = if header.value.is_empty() {
            format!("{};", header.name)
        } else {
            format!("{}:{}", header.name, header.value)
        };
        args.push(sh(&item));
    }
    if let Some(b64) = &parts.body.base64 {
        let _ = write!(out, "printf '%s' {} | base64 --decode | ", sh(b64));
    }
    out.push_str(&args.join(" \\\n  "));
    out.push('\n');
    out
}

/// Headers `fetch` refuses or manages itself.
fn fetch_forbidden(name: &str) -> bool {
    matches!(
        name,
        "accept-charset"
            | "accept-encoding"
            | "access-control-request-headers"
            | "access-control-request-method"
            | "connection"
            | "content-length"
            | "cookie2"
            | "date"
            | "dnt"
            | "expect"
            | "host"
            | "keep-alive"
            | "origin"
            | "referer"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
            | "via"
    ) || name.starts_with("proxy-")
        || name.starts_with("sec-")
}

/// A JavaScript `fetch` call reproducing the request.
pub fn fetch(exchange: &Exchange, redaction: &Redaction) -> String {
    let parts = request_parts(exchange, redaction);
    let js = |text: &str| serde_json::to_string(text).unwrap_or_else(|_| "\"\"".into());
    let mut out = truncation_note(&parts.body, "//");
    let _ = writeln!(out, "await fetch({}, {{", js(&parts.url));
    let _ = writeln!(out, "  method: {},", js(&parts.method));
    let headers: Vec<&HeaderView> = parts
        .headers
        .iter()
        .filter(|h| !fetch_forbidden(&h.name))
        .collect();
    if !headers.is_empty() {
        out.push_str("  headers: {\n");
        for (i, header) in headers.iter().enumerate() {
            let comma = if i + 1 < headers.len() { "," } else { "" };
            let _ = writeln!(
                out,
                "    {}: {}{comma}",
                js(&header.name),
                js(&header.value)
            );
        }
        out.push_str("  },\n");
    }
    let bodyless = matches!(parts.method.as_str(), "GET" | "HEAD");
    match (&parts.body.text, &parts.body.base64) {
        (Some(text), _) if !bodyless => {
            let _ = writeln!(out, "  body: {},", js(text));
        }
        (None, Some(b64)) if !bodyless => {
            let _ = writeln!(
                out,
                "  body: Uint8Array.from(atob({}), (c) => c.charCodeAt(0)),",
                js(b64)
            );
        }
        _ => {}
    }
    out.push_str("});\n");
    out
}

fn body_text(body: &BodyView) -> Option<String> {
    match (&body.text, &body.base64) {
        (Some(text), _) => Some(text.clone()),
        (None, Some(_)) => Some(format!("[binary body, {} bytes]", body.size)),
        (None, None) => None,
    }
}

/// Raw HTTP/1.1: the request, a blank line, then the response.
pub fn raw_http(exchange: &Exchange, redaction: &Redaction) -> String {
    let view = exchange.view(redaction);
    let request = &view.request;
    let target = match &request.query {
        Some(query) => format!("{}?{query}", request.path),
        None => request.path.clone(),
    };
    let mut out = format!("{} {target} HTTP/1.1\r\n", request.method);
    if !request.headers.iter().any(|h| h.name == "host") {
        let _ = write!(out, "host: {}\r\n", request.host);
    }
    for header in &request.headers {
        let _ = write!(out, "{}: {}\r\n", header.name, header.value);
    }
    out.push_str("\r\n");
    if let Some(body) = body_text(&request.body) {
        out.push_str(&body);
        out.push_str("\r\n");
    }
    if let Some(response) = &view.response {
        let _ = write!(
            out,
            "\r\nHTTP/1.1 {} {}\r\n",
            response.status, response.status_text
        );
        for header in &response.headers {
            let _ = write!(out, "{}: {}\r\n", header.name, header.value);
        }
        out.push_str("\r\n");
        if let Some(body) = body_text(&response.body) {
            out.push_str(&body);
            out.push_str("\r\n");
        }
    }
    out
}

/// The exchange view as pretty JSON.
pub fn json_string(exchange: &Exchange, redaction: &Redaction) -> String {
    serde_json::to_string_pretty(&exchange.view(redaction)).unwrap_or_default()
}

/// A code fence longer than any backtick run in `text`.
fn fence_for(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    "`".repeat((longest + 1).max(3))
}

fn markdown_body(out: &mut String, body: &BodyView) {
    let Some(text) = body_text(body) else {
        return;
    };
    let (language, mut text) = match body.kind {
        ContentKind::Json => (
            "json",
            serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| serde_json::to_string_pretty(&value).ok())
                .unwrap_or(text),
        ),
        ContentKind::Html => ("html", text),
        ContentKind::Xml => ("xml", text),
        _ => ("", text),
    };
    let mut note = String::new();
    if text.len() > MARKDOWN_BODY_LIMIT {
        let mut cut = MARKDOWN_BODY_LIMIT;
        while !text.is_char_boundary(cut) {
            cut -= 1;
        }
        text.truncate(cut);
        note = format!(
            "\n_Body clipped to {MARKDOWN_BODY_LIMIT} bytes of {}._\n",
            body.size
        );
    } else if body.truncated || !body.complete {
        note = format!("\n_Captured {} of {} bytes._\n", body.captured, body.size);
    }
    let fence = fence_for(&text);
    let _ = write!(out, "\n{fence}{language}\n{text}\n{fence}\n{note}");
}

/// Markdown with the request and response in fenced blocks, for GitHub issues and AI
/// agents.
pub fn markdown(exchange: &Exchange, redaction: &Redaction) -> String {
    let view = exchange.view(redaction);
    let request = &view.request;
    let target = match &request.query {
        Some(query) => format!("{}?{query}", request.path),
        None => request.path.clone(),
    };
    let status = view.response.as_ref().map_or_else(
        || "no response".to_owned(),
        |r| format!("{} {}", r.status, r.status_text),
    );
    let duration = view
        .duration_ms
        .map(|ms| format!(" · {ms:.0} ms"))
        .unwrap_or_default();
    let mut out = format!(
        "### `{} {}` → {status}{duration}\n\n`{}` at {}\n",
        request.method,
        mask_text(&target, redaction).replace('`', "'"),
        request.url.replace('`', "'"),
        iso8601(view.started_at_ms)
    );
    if let Some(error) = &view.error {
        let _ = writeln!(out, "\n**Error** ({:?}): {}", error.kind, error.message);
    }
    let mut head = format!("{} {target} {}\n", request.method, request.http_version);
    for header in &request.headers {
        let _ = writeln!(head, "{}: {}", header.name, header.value);
    }
    let fence = fence_for(&head);
    let _ = write!(out, "\n**Request**\n\n{fence}http\n{head}{fence}\n");
    markdown_body(&mut out, &request.body);
    if let Some(response) = &view.response {
        let mut head = format!(
            "{} {} {}\n",
            response.http_version, response.status, response.status_text
        );
        for header in &response.headers {
            let _ = writeln!(head, "{}: {}", header.name, header.value);
        }
        let fence = fence_for(&head);
        let _ = write!(out, "\n**Response**\n\n{fence}http\n{head}{fence}\n");
        markdown_body(&mut out, &response.body);
    }
    if let Some(stream) = &view.stream {
        let _ = writeln!(
            out,
            "\n**Stream**: {} messages from the client, {} from the server.",
            stream.client.count, stream.server.count
        );
    }
    if redaction.mask {
        out.push_str("\n_Secrets are redacted._\n");
    }
    out
}

fn har_headers(headers: &[HeaderView]) -> Value {
    Value::Array(
        headers
            .iter()
            .map(|h| json!({ "name": h.name, "value": h.value }))
            .collect(),
    )
}

fn har_cookies(headers: &[HeaderView], name: &str) -> Value {
    let cookies = headers
        .iter()
        .filter(|h| h.name == name)
        .flat_map(|h| {
            let pairs: Vec<&str> = if name == "cookie" {
                h.value.split(';').collect()
            } else {
                h.value.split(';').take(1).collect()
            };
            pairs
                .into_iter()
                .filter_map(|pair| pair.trim().split_once('='))
                .map(|(k, v)| json!({ "name": k, "value": v }))
                .collect::<Vec<_>>()
        })
        .collect();
    Value::Array(cookies)
}

fn har_query(query: Option<&str>, redaction: &Redaction) -> Value {
    let Some(query) = query else {
        return Value::Array(Vec::new());
    };
    let decode = |s: &str| {
        percent_encoding::percent_decode_str(&s.replace('+', " "))
            .decode_utf8_lossy()
            .into_owned()
    };
    Value::Array(
        query
            .split('&')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
                let key = decode(key);
                let value = if redaction.mask && !value.is_empty() && is_sensitive_key(&key) {
                    crate::redact::MASK.to_owned()
                } else {
                    mask_text(&decode(value), redaction).into_owned()
                };
                json!({ "name": key, "value": value })
            })
            .collect(),
    )
}

#[allow(clippy::cast_precision_loss)]
fn micros_to_ms(us: u64) -> f64 {
    us as f64 / 1_000.0
}

fn har_entry(exchange: &Exchange, redaction: &Redaction) -> Value {
    let view = exchange.view(redaction);
    let request = &view.request;
    let t = &exchange.timings;
    let connect = t.upstream_connected_us.map(micros_to_ms);
    let first = t.first_byte_us.map_or(0.0, micros_to_ms);
    let complete = t.complete_us.map_or(first, micros_to_ms);
    let wait = (first - connect.unwrap_or(0.0)).max(0.0);
    let receive = (complete - first).max(0.0);
    let total = connect.unwrap_or(0.0) + wait + receive;

    let request_body = &request.body;
    let mut request_json = json!({
        "method": request.method,
        "url": request.url,
        "httpVersion": request.http_version,
        "cookies": har_cookies(&request.headers, "cookie"),
        "headers": har_headers(&request.headers),
        "queryString": har_query(exchange.request.query(), redaction),
        "headersSize": -1,
        "bodySize": request_body.size,
    });
    if let (Some(text), Some(object)) = (
        request_body.text.as_ref().or(request_body.base64.as_ref()),
        request_json.as_object_mut(),
    ) {
        object.insert(
            "postData".into(),
            json!({
                "mimeType": request_body.content_type.clone().unwrap_or_default(),
                "text": text,
            }),
        );
    }

    let response_json = match &view.response {
        Some(response) => {
            let body = &response.body;
            let mut content = json!({
                "size": body.size,
                "mimeType": body.content_type.clone().unwrap_or_default(),
            });
            if let Some(object) = content.as_object_mut() {
                if let Some(text) = &body.text {
                    object.insert("text".into(), json!(text));
                } else if let Some(b64) = &body.base64 {
                    object.insert("text".into(), json!(b64));
                    object.insert("encoding".into(), json!("base64"));
                }
                if body.truncated {
                    object.insert(
                        "comment".into(),
                        json!(format!("captured {} of {} bytes", body.captured, body.size)),
                    );
                }
            }
            let redirect = response
                .headers
                .iter()
                .find(|h| h.name == "location")
                .map(|h| h.value.clone())
                .unwrap_or_default();
            json!({
                "status": response.status,
                "statusText": response.status_text,
                "httpVersion": response.http_version,
                "cookies": har_cookies(&response.headers, "set-cookie"),
                "headers": har_headers(&response.headers),
                "content": content,
                "redirectURL": redirect,
                "headersSize": -1,
                "bodySize": body.size,
            })
        }
        None => json!({
            "status": 0,
            "statusText": "",
            "httpVersion": "",
            "cookies": [],
            "headers": [],
            "content": { "size": 0, "mimeType": "" },
            "redirectURL": "",
            "headersSize": -1,
            "bodySize": -1,
            "_error": view.error.as_ref().map(|e| e.message.clone()),
        }),
    };

    json!({
        "startedDateTime": iso8601(view.started_at_ms),
        "time": total,
        "request": request_json,
        "response": response_json,
        "cache": {},
        "timings": {
            "blocked": -1,
            "dns": -1,
            "connect": connect.map_or(json!(-1), |c| json!(c)),
            "send": 0,
            "wait": wait,
            "receive": receive,
            "ssl": -1,
        },
        "_id": view.id,
        "_tap": view.tap,
        "_kind": view.kind,
        "_replayOf": view.replay_of,
    })
}

/// A HAR 1.2 document with one entry per exchange (in the order given).
pub fn har(exchanges: &[&Exchange], redaction: &Redaction) -> Value {
    json!({
        "log": {
            "version": "1.2",
            "creator": { "name": "Teitunnel", "version": env!("CARGO_PKG_VERSION") },
            "entries": exchanges.iter().map(|e| har_entry(e, redaction)).collect::<Vec<_>>(),
        }
    })
}

/// [`har`] as pretty JSON text.
pub fn har_string(exchanges: &[&Exchange], redaction: &Redaction) -> String {
    serde_json::to_string_pretty(&har(exchanges, redaction)).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use http::{HeaderValue, Method};
    use proptest::prelude::*;

    use super::*;
    use crate::{BodyRecord, store::tests::sample};

    fn webhook() -> Exchange {
        let mut ex = sample("t", 1, Method::POST, "/hooks/stripe?token=abc&page=1", 200);
        let h = &mut ex.request.headers;
        h.insert("content-type", HeaderValue::from_static("application/json"));
        h.insert(
            "stripe-signature",
            HeaderValue::from_static("t=1,v1=deadbeef"),
        );
        h.insert("content-length", HeaderValue::from_static("17"));
        h.insert(
            "cookie",
            HeaderValue::from_static("sid=secret1; theme=dark"),
        );
        ex.request.body = BodyRecord::full(Bytes::from_static(
            br#"{"password":"hunter2","it's":"quoted"}"#,
        ));
        let response = ex.response.as_mut().unwrap();
        response
            .headers
            .insert("content-type", HeaderValue::from_static("application/json"));
        response.headers.insert(
            "set-cookie",
            HeaderValue::from_static("sid=secret2; Path=/"),
        );
        response.body = BodyRecord::full(Bytes::from_static(br#"{"received":true}"#));
        ex.timings.upstream_connected_us = Some(1_000);
        ex.timings.first_byte_us = Some(5_000);
        ex.timings.complete_us = Some(6_000);
        ex
    }

    const SECRETS: [&str; 5] = ["topsecret", "hunter2", "deadbeef", "secret1", "secret2"];

    fn assert_masked(text: &str) {
        for secret in SECRETS {
            assert!(!text.contains(secret), "{secret} leaked in:\n{text}");
        }
    }

    #[test]
    fn curl_export() {
        let out = curl(&webhook(), &Redaction::masked());
        assert_masked(&out);
        assert!(out.starts_with(
            "curl \\\n  -X POST \\\n  'https://app.example.com/hooks/stripe?token=[redacted]&page=1'"
        ));
        assert!(out.contains("-H 'stripe-signature: [redacted]'"));
        assert!(!out.contains("content-length"));
        assert!(!out.contains("-H 'host:"), "host matches the URL");
        assert!(out.contains(r#"--data-raw '{"password":"[redacted]","it'\''s":"quoted"}'"#));
        let raw = curl(&webhook(), &Redaction::revealed());
        assert!(raw.contains("hunter2") && raw.contains("token=abc"));
    }

    #[test]
    fn curl_binary_and_truncated() {
        let mut ex = webhook();
        ex.request.headers.insert(
            "content-type",
            HeaderValue::from_static("application/octet-stream"),
        );
        ex.request.body = BodyRecord {
            data: Bytes::from_static(&[0, 1, 2, 255]),
            size: 10,
            truncated: true,
            complete: true,
        };
        let out = curl(&ex, &Redaction::masked());
        assert!(out.starts_with(
            "# Note: the captured body is incomplete (4 of 10 bytes).\nprintf '%s' 'AAEC/w==' | base64 --decode | curl"
        ));
        assert!(out.contains("--data-binary @-"));
    }

    #[test]
    fn httpie_export() {
        let out = httpie(&webhook(), &Redaction::masked());
        assert_masked(&out);
        assert!(out.starts_with("http \\\n  --raw="));
        assert!(out.contains(
            "  POST \\\n  'https://app.example.com/hooks/stripe?token=[redacted]&page=1'"
        ));
        assert!(out.contains("'content-type:application/json'"));
    }

    #[test]
    fn fetch_export() {
        let out = fetch(&webhook(), &Redaction::masked());
        assert_masked(&out);
        assert!(out.starts_with(
            "await fetch(\"https://app.example.com/hooks/stripe?token=[redacted]&page=1\", {\n  method: \"POST\",\n  headers: {\n"
        ));
        assert!(out.contains("    \"content-type\": \"application/json\""));
        assert!(!out.contains("\"host\""));
        assert!(out.contains(r#"  body: "{\"password\":\"[redacted]\",\"it's\":\"quoted\"}","#));
        assert!(out.ends_with("});\n"));
        let mut get = webhook();
        get.request.method = Method::GET;
        assert!(!fetch(&get, &Redaction::masked()).contains("body:"));
    }

    #[test]
    fn raw_export() {
        let out = raw_http(&webhook(), &Redaction::masked());
        assert_masked(&out);
        assert!(out.starts_with("POST /hooks/stripe?token=[redacted]&page=1 HTTP/1.1\r\n"));
        assert!(out.contains("\r\n\r\nHTTP/1.1 200 OK\r\n"));
        assert!(out.contains("set-cookie: sid=[redacted]; Path=/\r\n"));
        assert!(out.ends_with("{\"received\":true}\r\n"));
    }

    #[test]
    fn json_export() {
        let out = json_string(&webhook(), &Redaction::masked());
        assert_masked(&out);
        let value: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(value["request"]["method"], "POST");
        assert_eq!(value["response"]["status"], 200);
        assert_eq!(value["redacted"], true);
    }

    #[test]
    fn markdown_export() {
        let mut ex = webhook();
        ex.response.as_mut().unwrap().body =
            BodyRecord::full(Bytes::from_static(b"{\"code\":\"```\"}"));
        let out = markdown(&ex, &Redaction::masked());
        assert_masked(&out);
        assert!(
            out.starts_with("### `POST /hooks/stripe?token=[redacted]&page=1` → 200 OK · 6 ms\n")
        );
        assert!(out.contains(
            "**Request**\n\n```http\nPOST /hooks/stripe?token=[redacted]&page=1 HTTP/1.1\n"
        ));
        assert!(out.contains("```json\n{\n"));
        assert!(out.contains("  \"password\": \"[redacted]\""));
        assert!(
            out.contains("````json\n{\n  \"code\": \"```\"\n}\n````"),
            "{out}"
        );
        assert!(out.ends_with("_Secrets are redacted._\n"));
    }

    /// Structural validation against the HAR 1.2 spec's required fields and types.
    fn validate_har(value: &Value) {
        let log = &value["log"];
        assert_eq!(log["version"], "1.2");
        assert!(log["creator"]["name"].is_string());
        assert!(log["creator"]["version"].is_string());
        for entry in log["entries"].as_array().unwrap() {
            assert!(entry["startedDateTime"].as_str().unwrap().ends_with('Z'));
            assert!(entry["time"].is_number());
            let request = &entry["request"];
            for key in ["method", "url", "httpVersion"] {
                assert!(request[key].is_string(), "request.{key}");
            }
            for key in ["cookies", "headers", "queryString"] {
                assert!(request[key].is_array(), "request.{key}");
            }
            assert!(request["headersSize"].is_i64() && request["bodySize"].is_number());
            let response = &entry["response"];
            assert!(response["status"].is_number());
            for key in ["statusText", "httpVersion", "redirectURL"] {
                assert!(response[key].is_string(), "response.{key}");
            }
            for key in ["cookies", "headers"] {
                assert!(response[key].is_array(), "response.{key}");
            }
            assert!(response["content"]["size"].is_number());
            assert!(response["content"]["mimeType"].is_string());
            assert!(entry["cache"].is_object());
            let timings = &entry["timings"];
            for key in ["send", "wait", "receive"] {
                assert!(timings[key].as_f64().unwrap() >= 0.0, "timings.{key}");
            }
            for header in request["headers"].as_array().unwrap() {
                assert!(header["name"].is_string() && header["value"].is_string());
            }
        }
    }

    #[test]
    fn har_export() {
        let first = webhook();
        let mut failed = sample("t", 2, Method::GET, "/", 200);
        failed.response = None;
        let value = har(&[&first, &failed], &Redaction::masked());
        validate_har(&value);
        assert_masked(&value.to_string());
        let entry = &value["log"]["entries"][0];
        assert_eq!(entry["time"], 6.0);
        assert_eq!(entry["timings"]["connect"], 1.0);
        assert_eq!(entry["timings"]["wait"], 4.0);
        assert_eq!(entry["timings"]["receive"], 1.0);
        assert_eq!(entry["request"]["queryString"][0]["value"], "[redacted]");
        assert_eq!(entry["request"]["cookies"][0]["name"], "sid");
        assert_eq!(entry["response"]["cookies"][0]["value"], "[redacted]");
        assert_eq!(entry["request"]["postData"]["mimeType"], "application/json");
        assert_eq!(value["log"]["entries"].as_array().unwrap().len(), 2);
        let single: Value =
            serde_json::from_str(&export(ExportFormat::Har, &first, &Redaction::masked())).unwrap();
        validate_har(&single);
    }

    #[test]
    fn har_binary_response_is_base64() {
        let mut ex = webhook();
        let response = ex.response.as_mut().unwrap();
        response
            .headers
            .insert("content-type", HeaderValue::from_static("image/png"));
        response.body = BodyRecord::full(Bytes::from_static(b"\x89PNG\r\n\x1a\n"));
        let value = har(&[&ex], &Redaction::masked());
        let content = &value["log"]["entries"][0]["response"]["content"];
        assert_eq!(content["encoding"], "base64");
        assert_eq!(content["text"], "iVBORw0KGgo=");
    }

    proptest! {
        #[test]
        fn exports_never_panic(
            path in "/[ -~]{0,40}",
            body in proptest::collection::vec(any::<u8>(), 0..300),
            header in "[ -~]{0,40}",
        ) {
            let mut ex = sample("t", 1, Method::POST, "/", 200);
            if let Ok(uri) = path.replace(' ', "%20").parse() {
                ex.request.uri = uri;
            }
            if let Ok(value) = HeaderValue::from_str(&header) {
                ex.request.headers.insert("x-any", value.clone());
                ex.request.headers.insert("content-encoding", value);
            }
            ex.request.body = BodyRecord::full(Bytes::from(body));
            for format in [
                ExportFormat::Curl, ExportFormat::Httpie, ExportFormat::Fetch, ExportFormat::Raw,
                ExportFormat::Har, ExportFormat::Json, ExportFormat::Markdown,
            ] {
                for redaction in [Redaction::masked(), Redaction::revealed()] {
                    let _ = export(format, &ex, &redaction);
                }
            }
            let har_text = har_string(&[&ex], &Redaction::masked());
            prop_assert!(serde_json::from_str::<Value>(&har_text).is_ok());
        }
    }
}
