//! OpenAPI from traffic: an OpenAPI 3.1 description inferred from requests the
//! inspector captured: paths with parameters (`/users/123` → `/users/{id}`), methods,
//! query and header parameters, request and response bodies as JSON Schemas merged
//! across samples, status codes and the authentication seen. For docs and agents.
//!
//! It reads the masked captures (the history holds no secrets), copies no values
//! into the document (no examples), and leaves out what isn't an API: pages, scripts,
//! styles, images and fonts, answers Teitunnel gave itself (paused, sign-in, stubs,
//! faults), `OPTIONS` and `HEAD`.

mod paths;
mod schema;

use std::collections::{BTreeMap, BTreeSet};

use http::HeaderMap;
use lens::{BodyRecord, Exchange, ExchangeKind, Responder, decode_body};
use serde::Serialize;
use serde_json::{Map, Value, json};

pub use paths::{Segment, Templates, looks_dynamic};
pub use schema::{Learned, Scalar, conforms};

/// Request headers that say nothing about the API (sent by every client or proxy).
const COMMON_HEADERS: &[&str] = &[
    "accept",
    "accept-encoding",
    "accept-language",
    "authorization",
    "cache-control",
    "cdn-loop",
    "connection",
    "content-length",
    "content-type",
    "cookie",
    "dnt",
    "host",
    "origin",
    "pragma",
    "priority",
    "referer",
    "te",
    "traceparent",
    "tracestate",
    "true-client-ip",
    "upgrade",
    "upgrade-insecure-requests",
    "user-agent",
    "via",
    "x-real-ip",
    "x-requested-with",
];

fn is_common_header(name: &str) -> bool {
    COMMON_HEADERS.contains(&name)
        || name.starts_with("cf-")
        || name.starts_with("x-forwarded-")
        || name.starts_with("sec-")
        || name.starts_with("if-")
        || name.starts_with("x-teitunnel")
}

/// What to describe.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Only requests to this host (`None`: all).
    pub host: Option<String>,
    /// The document's title (`None`: from the hosts).
    pub title: Option<String>,
}

/// What went into a description.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    /// Requests used.
    pub requests: u32,
    /// Requests left out (pages, assets, Teitunnel's own answers…).
    pub skipped: u32,
    /// Paths described.
    pub paths: u32,
    /// Operations (path and method) described.
    pub operations: u32,
    /// Hosts seen.
    pub hosts: Vec<String>,
}

fn media_type(headers: &HeaderMap) -> Option<String> {
    headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(';').next())
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
}

fn is_json(media: &str) -> bool {
    media == "application/json" || media.ends_with("+json")
}

/// A response that's a page or an asset, not an API answer.
fn is_asset(media: Option<&str>, path: &str) -> bool {
    if let Some(media) = media
        && (media == "text/html"
            || media == "text/css"
            || media.contains("javascript")
            || media.starts_with("image/")
            || media.starts_with("font/")
            || media.starts_with("video/")
            || media.starts_with("audio/"))
    {
        return true;
    }
    let last = path
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    [
        ".js", ".mjs", ".css", ".map", ".png", ".jpg", ".jpeg", ".gif", ".svg", ".ico", ".webp",
        ".avif", ".woff", ".woff2", ".ttf", ".html", ".htm",
    ]
    .iter()
    .any(|ext| last.ends_with(ext))
}

/// A body's JSON value, or form fields, when complete and parseable.
enum BodySample {
    Json(Value),
    Form(Vec<String>),
    Other,
}

fn sample(headers: &HeaderMap, body: &BodyRecord, media: Option<&str>) -> Option<BodySample> {
    if body.size == 0 {
        return None;
    }
    let media = media?;
    if body.truncated || !body.complete {
        return Some(BodySample::Other);
    }
    let decoded = decode_body(headers, &body.data).ok()?;
    if is_json(media) {
        return Some(
            serde_json::from_slice::<Value>(&decoded).map_or(BodySample::Other, BodySample::Json),
        );
    }
    if media == "application/x-www-form-urlencoded" {
        let text = String::from_utf8_lossy(&decoded);
        let names = text
            .split('&')
            .filter_map(|pair| pair.split('=').next())
            .filter(|name| !name.is_empty())
            .map(str::to_owned)
            .collect();
        return Some(BodySample::Form(names));
    }
    Some(BodySample::Other)
}

/// A body per media type.
#[derive(Debug, Default)]
struct Content {
    by_media: BTreeMap<String, ContentSchema>,
}

#[derive(Debug, Default)]
struct ContentSchema {
    json: Learned,
    form_fields: BTreeSet<String>,
    samples: u64,
}

impl Content {
    fn add(&mut self, media: &str, sample: BodySample) {
        let entry = self.by_media.entry(media.to_owned()).or_default();
        entry.samples += 1;
        match sample {
            BodySample::Json(value) => entry.json.add(&value),
            BodySample::Form(names) => entry.form_fields.extend(names),
            BodySample::Other => {}
        }
    }

    fn render(&self) -> Value {
        let mut content = Map::new();
        for (media, body) in &self.by_media {
            let schema = if !body.json.is_empty() {
                body.json.schema()
            } else if !body.form_fields.is_empty() {
                let properties: Map<String, Value> = body
                    .form_fields
                    .iter()
                    .map(|name| (name.clone(), json!({ "type": "string" })))
                    .collect();
                json!({ "type": "object", "properties": properties })
            } else if media.starts_with("text/") {
                json!({ "type": "string" })
            } else if is_json(media) {
                json!({})
            } else {
                json!({ "type": "string", "contentMediaType": media })
            };
            content.insert(media.clone(), json!({ "schema": schema }));
        }
        Value::Object(content)
    }
}

#[derive(Debug, Default)]
struct Operation {
    count: u64,
    query: BTreeMap<String, (u64, Scalar)>,
    headers: BTreeMap<String, (u64, Scalar)>,
    path_params: BTreeMap<usize, Scalar>,
    request: Content,
    request_count: u64,
    responses: BTreeMap<u16, (Content, bool)>,
    bearer: bool,
    basic: bool,
}

fn reason(status: u16) -> String {
    http::StatusCode::from_u16(status)
        .ok()
        .and_then(|s| s.canonical_reason())
        .map_or_else(|| format!("Status {status}"), str::to_owned)
}

impl Operation {
    fn add(&mut self, exchange: &Exchange, path: &[String], template: &[Segment]) {
        self.count += 1;
        let request = &exchange.request;
        for (index, segment) in template.iter().enumerate() {
            if *segment == Segment::Param
                && let Some(value) = path.get(index)
            {
                self.path_params.entry(index).or_default().add(value);
            }
        }
        let mut seen = BTreeSet::new();
        for pair in request.query().unwrap_or_default().split('&') {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            if name.is_empty() || !seen.insert(name.to_owned()) {
                continue;
            }
            let entry = self.query.entry(name.to_owned()).or_default();
            entry.0 += 1;
            entry.1.add(value);
        }
        let mut seen = BTreeSet::new();
        for (name, value) in &request.headers {
            let name = name.as_str().to_ascii_lowercase();
            if is_common_header(&name) || !seen.insert(name.clone()) {
                continue;
            }
            let entry = self.headers.entry(name).or_default();
            entry.0 += 1;
            entry.1.add(value.to_str().unwrap_or_default());
        }
        if let Some(auth) = request
            .headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
        {
            let scheme = auth.split_whitespace().next().unwrap_or_default();
            self.bearer |= scheme.eq_ignore_ascii_case("bearer");
            self.basic |= scheme.eq_ignore_ascii_case("basic");
        }
        let media = media_type(&request.headers);
        if let Some(body) = sample(&request.headers, &request.body, media.as_deref()) {
            self.request_count += 1;
            self.request
                .add(media.as_deref().unwrap_or("application/octet-stream"), body);
        }
        if let Some(response) = &exchange.response {
            let status = response.status.as_u16();
            let media = media_type(&response.headers);
            let entry = self.responses.entry(status).or_default();
            if let Some(body) = sample(&response.headers, &response.body, media.as_deref()) {
                entry
                    .0
                    .add(media.as_deref().unwrap_or("application/octet-stream"), body);
                entry.1 = true;
            }
        }
    }

    fn render(&self, template: &[Segment], method: &str) -> Value {
        let names = paths::param_names(template);
        let mut parameters = Vec::new();
        let mut names = names.into_iter();
        for (index, segment) in template.iter().enumerate() {
            if *segment == Segment::Param {
                let name = names.next().unwrap_or_else(|| "id".into());
                let schema = self
                    .path_params
                    .get(&index)
                    .map_or_else(|| json!({ "type": "string" }), Scalar::schema);
                parameters.push(json!({
                    "name": name, "in": "path", "required": true, "schema": schema
                }));
            }
        }
        for (location, list) in [("query", &self.query), ("header", &self.headers)] {
            for (name, (count, scalar)) in list {
                let mut parameter =
                    json!({ "name": name, "in": location, "schema": scalar.schema() });
                if *count == self.count {
                    parameter["required"] = json!(true);
                }
                parameters.push(parameter);
            }
        }
        let mut operation = Map::new();
        operation.insert(
            "summary".into(),
            json!(format!(
                "{} {}",
                method.to_ascii_uppercase(),
                paths::render(template)
            )),
        );
        if !parameters.is_empty() {
            operation.insert("parameters".into(), Value::Array(parameters));
        }
        if self.request_count > 0 {
            let mut body = json!({ "content": self.request.render() });
            if self.request_count == self.count {
                body["required"] = json!(true);
            }
            operation.insert("requestBody".into(), body);
        }
        let mut responses = Map::new();
        for (status, (content, has_body)) in &self.responses {
            let mut response = json!({ "description": reason(*status) });
            if *has_body {
                response["content"] = content.render();
            }
            responses.insert(status.to_string(), response);
        }
        if responses.is_empty() {
            responses.insert("default".into(), json!({ "description": "Not observed" }));
        }
        operation.insert("responses".into(), Value::Object(responses));
        let mut security = Vec::new();
        if self.bearer {
            security.push(json!({ "bearerAuth": [] }));
        }
        if self.basic {
            security.push(json!({ "basicAuth": [] }));
        }
        if !security.is_empty() {
            operation.insert("security".into(), Value::Array(security));
        }
        operation.insert("x-observed-requests".into(), json!(self.count));
        Value::Object(operation)
    }
}

/// Whether an exchange describes the API (see the module docs).
fn usable(exchange: &Exchange, host: Option<&str>) -> bool {
    let request = &exchange.request;
    let method = request.method.as_str();
    if exchange.kind != ExchangeKind::Http
        || !matches!(exchange.responder, Responder::Upstream)
        || exchange.response.is_none()
        || matches!(method, "OPTIONS" | "HEAD" | "CONNECT" | "TRACE")
        || exchange.replay_of.is_some()
    {
        return false;
    }
    if host.is_some_and(|h| !request.host.eq_ignore_ascii_case(h)) {
        return false;
    }
    let media = exchange
        .response
        .as_ref()
        .and_then(|r| media_type(&r.headers));
    !is_asset(media.as_deref(), request.path())
}

/// Infers an OpenAPI 3.1 document from `exchanges`.
pub fn infer(exchanges: &[Exchange], options: &Options) -> (Value, Summary) {
    let host = options
        .host
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty());
    let (used, skipped): (Vec<&Exchange>, Vec<&Exchange>) =
        exchanges.iter().partition(|e| usable(e, host));
    let split: Vec<Vec<String>> = used
        .iter()
        .map(|e| paths::split(e.request.path()))
        .collect();
    let templates = Templates::learn(split.iter().map(Vec::as_slice));
    let mut operations: BTreeMap<(Vec<Segment>, String), Operation> = BTreeMap::new();
    let mut hosts = BTreeSet::new();
    let mut schemes: BTreeMap<String, String> = BTreeMap::new();
    for (exchange, path) in used.iter().zip(&split) {
        let Some(template) = templates.find(path) else {
            continue;
        };
        hosts.insert(exchange.request.host.to_ascii_lowercase());
        schemes
            .entry(exchange.request.host.to_ascii_lowercase())
            .or_insert_with(|| exchange.request.scheme.clone());
        let method = exchange.request.method.as_str().to_ascii_lowercase();
        operations
            .entry((template.clone(), method))
            .or_default()
            .add(exchange, path, template);
    }
    let mut paths_out: BTreeMap<String, Map<String, Value>> = BTreeMap::new();
    let (mut bearer, mut basic) = (false, false);
    for ((template, method), operation) in &operations {
        bearer |= operation.bearer;
        basic |= operation.basic;
        paths_out
            .entry(paths::render(template))
            .or_default()
            .insert(method.clone(), operation.render(template, method));
    }
    let hosts: Vec<String> = hosts.into_iter().collect();
    let title = options
        .title
        .clone()
        .unwrap_or_else(|| match hosts.as_slice() {
            [one] => format!("{one} API"),
            _ => "Observed API".to_owned(),
        });
    let mut document = json!({
        "openapi": "3.1.0",
        "info": {
            "title": title,
            "version": "observed",
            "description": format!(
                "Inferred by Teitunnel from {} captured requests. Review it before publishing: only what was observed is described.",
                used.len()
            ),
        },
        "servers": hosts
            .iter()
            .map(|h| json!({ "url": format!("{}://{h}", schemes.get(h).map_or("https", String::as_str)) }))
            .collect::<Vec<_>>(),
        "paths": paths_out,
    });
    if bearer || basic {
        let mut security = Map::new();
        if bearer {
            security.insert(
                "bearerAuth".into(),
                json!({ "type": "http", "scheme": "bearer" }),
            );
        }
        if basic {
            security.insert(
                "basicAuth".into(),
                json!({ "type": "http", "scheme": "basic" }),
            );
        }
        document["components"] = json!({ "securitySchemes": security });
    }
    let summary = Summary {
        requests: u32::try_from(used.len()).unwrap_or(u32::MAX),
        skipped: u32::try_from(skipped.len()).unwrap_or(u32::MAX),
        paths: u32::try_from(paths_out_len(&document)).unwrap_or(u32::MAX),
        operations: u32::try_from(operations.len()).unwrap_or(u32::MAX),
        hosts,
    };
    (document, summary)
}

fn paths_out_len(document: &Value) -> usize {
    document["paths"].as_object().map_or(0, Map::len)
}

/// Exchanges read for one description at most (the newest).
pub const MAX_EXCHANGES: usize = 5_000;

/// Describes the traffic this process's `inspector` captured (live and restored) or,
/// without one, the history every process keeps in `store` (masked).
///
/// # Errors
/// The history can't be read.
pub async fn describe(
    inspector: Option<&crate::inspect::Inspector>,
    store: Option<&crate::store::Store>,
    options: &Options,
) -> Result<(Value, Summary), crate::store::StoreError> {
    let filter = lens::Filter {
        host: options.host.clone(),
        ..lens::Filter::default()
    };
    let exchanges: Vec<Exchange> = match (inspector, store) {
        (Some(inspector), _) if inspector.running().is_some() => {
            let mut out = Vec::new();
            let mut before = None;
            while out.len() < MAX_EXCHANGES {
                let page = inspector.list_raw(&lens::Query {
                    filter: filter.clone(),
                    limit: Some(lens::MAX_PAGE),
                    before,
                });
                out.extend(page.items.iter().map(|e| (**e).clone()));
                match page.next {
                    Some(next) => before = Some(next),
                    None => break,
                }
            }
            out
        }
        (_, Some(store)) => {
            crate::inspect::history(
                store,
                crate::inspect::HistoryQuery {
                    filter,
                    limit: MAX_EXCHANGES,
                },
            )
            .await?
        }
        _ => Vec::new(),
    };
    Ok(infer(&exchanges, options))
}

/// A document as text: YAML when `yaml`, else pretty JSON.
///
/// # Errors
/// The YAML writer failed (it never should for JSON values).
pub fn render(document: &Value, yaml: bool) -> Result<String, String> {
    if yaml {
        serde_saphyr::to_string(document).map_err(|e| e.to_string())
    } else {
        serde_json::to_string_pretty(document).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests;
