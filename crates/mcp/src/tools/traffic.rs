//! Traffic tools over the inspector's captures ([`TrafficSource`]): list, inspect,
//! replay, wait for a request (webhook testing without polling), stats and export.
//! Credentials in headers are masked and bodies redacted unless the server allows
//! secrets.

use std::time::Duration;

use rmcp::model::JsonObject;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{Hints, spec};
use crate::{
    limits, redaction,
    registry::{
        Approval, ApprovalRequest, ToolClass, ToolContext, ToolError, ToolOutput, ToolResult,
        ToolSpec, arguments,
    },
    traffic::{
        Body, Exchange, ExchangeSummary, HttpMessage, ReplayEdits, ReplayResult, TrafficFilter,
        TrafficFormat, TrafficSource, TrafficStats,
    },
};

/// Longest wait_for_request waits.
const MAX_WAIT: u64 = 600;
/// Bodies returned by traffic_get are cut here unless asked for more.
const BODY_LIMIT: usize = 16 * 1024;
/// The most asked for.
const MAX_BODY_LIMIT: usize = 256 * 1024;

pub(super) fn specs() -> Vec<ToolSpec> {
    vec![
        spec::<ListArgs, ListResult>(
            "traffic_list",
            "List captured requests",
            "List HTTP requests captured by Teitunnel's inspector on shares and routes, newest first: method, path, status, duration and sizes. Filter by share or route (`scope`), method, path text, status (`404`, `5xx`), slowness, text in headers or bodies, or time.\n\
             \n\
             Use it to see what a webhook sender or browser actually sent, then traffic_get for the full request. To wait for a request that hasn't arrived yet, use wait_for_request instead of polling this.\n\
             \n\
             Example: {\"scope\": \"demo.example.com\", \"status\": \"5xx\"}",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<GetArgs, Exchange>(
            "traffic_get",
            "Inspect a request",
            "Get one captured request and its response in full: headers, bodies (text, or base64 for binary; cut at `bodyLimit` bytes), timing and any error reaching the local service. Credentials (Authorization, Cookie, webhook signatures…) are masked unless the person started the server with --allow-secrets.\n\
             \n\
             Example: {\"id\": \"ex_01J8…\"}",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<ReplayArgs, ReplayOut>(
            "traffic_replay",
            "Replay a request",
            "Send a captured request to the local service again, optionally edited (method, path, headers, body), up to 20 times. The replays are captured like any request, so traffic_get shows their responses. This runs the request again on the person's service (a replayed webhook may, say, create an order twice): in `ask` mode the person approves first.\n\
             \n\
             Use it after fixing code to check the same webhook now succeeds, without asking the sender to resend.\n\
             \n\
             Example: {\"id\": \"ex_01J8…\"} · {\"id\": \"ex_01J8…\", \"edits\": {\"setHeaders\": [[\"X-Debug\", \"1\"]]}}",
            ToolClass::Change,
            Hints {
                read_only: false,
                destructive: true,
                idempotent: false,
                open_world: false,
            },
            Duration::from_secs(120),
        ),
        spec::<WaitArgs, WaitResult>(
            "wait_for_request",
            "Wait for a request",
            "Block until a request matching the filter arrives on a share or route (or `timeoutSeconds` passes), then return it, in full by default. Progress is reported while waiting, and the call can be cancelled.\n\
             \n\
             This is how to test webhooks: share the port (share_port), register the URL with the sender or trigger it, then call this with e.g. `pathContains: \"/webhooks\"` and `method: \"POST\"`; inspect it, fix the handler, and replay with traffic_replay. A request that arrived after `sinceMs` but before this call also counts, so nothing is missed between calls (default: from now).\n\
             \n\
             Example: {\"scope\": \"demo.example.com\", \"method\": \"POST\", \"pathContains\": \"/webhooks/stripe\", \"timeoutSeconds\": 300}",
            ToolClass::Wait,
            Hints::READ_LOCAL,
            Duration::from_secs(MAX_WAIT + 30),
        ),
        spec::<TrafficFilter, TrafficStats>(
            "traffic_stats",
            "Traffic numbers",
            "Numbers over captured requests matching the filter: count, status classes, latency percentiles (p50/p95/p99), bytes, and the busiest paths. Use it to spot errors or slow endpoints before looking at single requests.\n\
             \n\
             Example: {\"scope\": \"app.example.com\"}",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
        spec::<ExportArgs, ExportOut>(
            "traffic_export",
            "Export requests",
            "Export captured requests as `curl` commands (to resend them from a terminal), a HAR file (for browser dev tools and other tools), or Markdown (for an issue, a pull request or a chat). Credentials are masked unless the server allows secrets.\n\
             \n\
             Example: {\"ids\": [\"ex_01J8…\"], \"format\": \"curl\"}",
            ToolClass::Read,
            Hints::READ_LOCAL,
            super::DEFAULT_TIMEOUT,
        ),
    ]
}

/// A filter and paging.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListArgs {
    /// Which requests.
    #[serde(flatten)]
    filter: TrafficFilter,
    /// From a previous answer's `nextCursor`.
    #[serde(default)]
    cursor: Option<String>,
    /// At most this many (default 50, up to 200).
    #[serde(default)]
    limit: Option<usize>,
}

/// Captured requests.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ListResult {
    /// Newest first.
    exchanges: Vec<ExchangeSummary>,
    /// Pass as `cursor` for the next page.
    next_cursor: Option<String>,
}

/// The id and paths redacted.
fn clean_summary(mut s: ExchangeSummary, allow_secrets: bool) -> ExchangeSummary {
    s.path = redaction::text(&s.path, allow_secrets);
    s
}

fn clean_body(body: Body, allow_secrets: bool) -> Body {
    if body.encoding == "utf8" {
        Body {
            data: redaction::text(&body.data, allow_secrets),
            ..body
        }
    } else {
        body
    }
}

fn clean_message(message: HttpMessage, allow_secrets: bool) -> HttpMessage {
    HttpMessage {
        headers: message
            .headers
            .into_iter()
            .map(|(n, v)| {
                let v = redaction::header_value(&n, &v, allow_secrets);
                (n, v)
            })
            .collect(),
        body: clean_body(message.body, allow_secrets),
    }
}

/// An exchange with credentials masked (unless allowed).
pub(crate) fn clean(exchange: Exchange, allow_secrets: bool) -> Exchange {
    Exchange {
        summary: clean_summary(exchange.summary, allow_secrets),
        request: clean_message(exchange.request, allow_secrets),
        response: exchange.response.map(|r| clean_message(r, allow_secrets)),
        ttfb_ms: exchange.ttfb_ms,
        error: exchange.error.map(|e| redaction::text(&e, allow_secrets)),
    }
}

pub(super) async fn list(
    source: &dyn TrafficSource,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: ListArgs = arguments(args)?;
    let limit = args
        .limit
        .unwrap_or(limits::DEFAULT_PAGE)
        .clamp(1, limits::MAX_PAGE);
    let page = source
        .list(&args.filter, args.cursor.as_deref(), limit)
        .await?;
    Ok(ToolOutput::new(&ListResult {
        exchanges: page
            .exchanges
            .into_iter()
            .map(|s| clean_summary(s, ctx.allow_secrets()))
            .collect(),
        next_cursor: page.next_cursor,
    }))
}

/// Which request.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GetArgs {
    /// The request's id, from traffic_list or wait_for_request.
    id: String,
    /// Cut each body at this many bytes (default 16384, up to 262144).
    #[serde(default)]
    body_limit: Option<usize>,
}

pub(super) async fn get(
    source: &dyn TrafficSource,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: GetArgs = arguments(args)?;
    let limit = args
        .body_limit
        .unwrap_or(BODY_LIMIT)
        .clamp(1, MAX_BODY_LIMIT);
    let exchange = source.get(args.id.trim(), limit).await?;
    Ok(ToolOutput::new(&clean(exchange, ctx.allow_secrets())))
}

/// What to replay.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ReplayArgs {
    /// The request's id.
    id: String,
    /// Changes to make first.
    #[serde(default)]
    edits: ReplayEdits,
    /// How many times (1 to 20, default 1).
    #[serde(default)]
    #[schemars(range(min = 1, max = 20))]
    times: Option<u32>,
    /// Only when this server can't ask the person itself (the previous call answered
    /// `needsApproval`): the person agreed.
    #[serde(default)]
    confirmed: bool,
}

/// Replays.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReplayOut {
    /// `replayed`, `needsApproval` or `declined`.
    outcome: String,
    /// What happened.
    message: String,
    /// The new requests.
    replays: Vec<ReplayResult>,
}

pub(super) async fn replay(
    source: &dyn TrafficSource,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: ReplayArgs = arguments(args)?;
    let times = args.times.unwrap_or(1).clamp(1, 20);
    let original = source.get(args.id.trim(), 256).await?;
    let method = args
        .edits
        .method
        .as_deref()
        .unwrap_or(&original.summary.method);
    let path = args.edits.path.as_deref().unwrap_or(&original.summary.path);
    let details = format!(
        "Send {method} {path} to the local service behind {} again{}{}.",
        original.summary.host,
        if times > 1 {
            format!(", {times} times")
        } else {
            String::new()
        },
        if args.edits == ReplayEdits::default() {
            ""
        } else {
            ", with edits"
        },
    );
    match ctx
        .approve(&ApprovalRequest {
            title: format!("Replay {method} {path}"),
            details: details.clone(),
            confirmed: args.confirmed,
        })
        .await
    {
        Approval::Granted { .. } => {}
        Approval::NeedsConfirmation => {
            return Ok(ToolOutput::new(&ReplayOut {
                outcome: "needsApproval".into(),
                message: format!(
                    "Nothing was sent. Show the person this and call again with \"confirmed\": true if they agree:\n{details}"
                ),
                replays: Vec::new(),
            }));
        }
        Approval::Declined(why) => {
            return Ok(ToolOutput::new(&ReplayOut {
                outcome: "declined".into(),
                message: format!("{why} Nothing was sent."),
                replays: Vec::new(),
            }));
        }
    }
    let replays = source.replay(args.id.trim(), &args.edits, times).await?;
    let replays: Vec<ReplayResult> = replays
        .into_iter()
        .map(|r| ReplayResult {
            exchange: clean_summary(r.exchange, ctx.allow_secrets()),
        })
        .collect();
    let statuses: Vec<String> = replays
        .iter()
        .map(|r| {
            r.exchange
                .status
                .map_or_else(|| "no response".into(), |s| s.to_string())
        })
        .collect();
    Ok(ToolOutput::new(&ReplayOut {
        outcome: "replayed".into(),
        message: format!(
            "Replayed {} time(s): {}.",
            replays.len(),
            statuses.join(", ")
        ),
        replays,
    }))
}

/// What to wait for.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WaitArgs {
    /// Which request.
    #[serde(flatten)]
    filter: TrafficFilter,
    /// Give up after this many seconds (1 to 600, default 120).
    #[serde(default)]
    #[schemars(range(min = 1, max = 600))]
    timeout_seconds: Option<u64>,
    /// Return the full request and response (default true), not only the summary.
    #[serde(default = "yes")]
    include_details: bool,
}

fn yes() -> bool {
    true
}

/// What arrived.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WaitResult {
    /// `received`, `timeout` or `cancelled`.
    outcome: String,
    /// The request, once received.
    exchange: Option<ExchangeSummary>,
    /// In full, when asked.
    details: Option<Exchange>,
    /// What happened.
    message: String,
    /// The time the wait started from (pass as `sinceMs` to wait again without missing
    /// anything).
    since_ms: u64,
}

fn now_ms() -> u64 {
    teitunnel_core::domain_shares::now_ms()
}

pub(super) async fn wait_for_request(
    source: &dyn TrafficSource,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: WaitArgs = arguments(args)?;
    let timeout = Duration::from_secs(args.timeout_seconds.unwrap_or(120).clamp(1, MAX_WAIT));
    let mut filter = args.filter;
    let since = *filter.since_ms.get_or_insert_with(now_ms);
    let waiting = source.next_matching(&filter);
    tokio::pin!(waiting);
    let started = tokio::time::Instant::now();
    let mut beat = tokio::time::interval(Duration::from_secs(5));
    beat.tick().await;
    let found = loop {
        tokio::select! {
            found = &mut waiting => break Some(found?),
            () = tokio::time::sleep_until(started + timeout) => break None,
            () = ctx.cancelled().cancelled() => {
                return Ok(ToolOutput::new(&WaitResult {
                    outcome: "cancelled".into(),
                    exchange: None,
                    details: None,
                    message: "Stopped waiting.".into(),
                    since_ms: since,
                }));
            }
            _ = beat.tick() => {
                let waited = started.elapsed().as_secs();
                ctx.progress(waited as f64, Some(timeout.as_secs() as f64), format!("Waiting for a matching request ({waited} s)…")).await;
            }
        }
    };
    let Some(summary) = found else {
        return Ok(ToolOutput::new(&WaitResult {
            outcome: "timeout".into(),
            exchange: None,
            details: None,
            message: format!(
                "No matching request arrived in {} s. Check the sender points at the share's URL (list_shares), then wait again with the same sinceMs so an arrival in between isn't missed.",
                timeout.as_secs()
            ),
            since_ms: since,
        }));
    };
    let details = if args.include_details {
        match source.get(&summary.id, BODY_LIMIT).await {
            Ok(exchange) => Some(clean(exchange, ctx.allow_secrets())),
            Err(err) => return Err(ToolError::from(err)),
        }
    } else {
        None
    };
    let message = format!(
        "Received {} {} → {}.",
        summary.method,
        summary.path,
        summary
            .status
            .map_or_else(|| "no response yet".into(), |s| s.to_string())
    );
    Ok(ToolOutput::new(&WaitResult {
        outcome: "received".into(),
        exchange: Some(clean_summary(summary, ctx.allow_secrets())),
        details,
        message,
        since_ms: since,
    }))
}

pub(super) async fn stats(source: &dyn TrafficSource, args: JsonObject) -> ToolResult {
    let filter: TrafficFilter = arguments(args)?;
    Ok(ToolOutput::new(&source.stats(&filter).await?))
}

/// What to export.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ExportArgs {
    /// Request ids (1 to 100).
    ids: Vec<String>,
    /// `curl`, `har` or `markdown`.
    format: TrafficFormat,
}

/// An export.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ExportOut {
    /// The format.
    format: TrafficFormat,
    /// The exported text.
    contents: String,
}

pub(super) async fn export(
    source: &dyn TrafficSource,
    args: JsonObject,
    ctx: &ToolContext,
) -> ToolResult {
    let args: ExportArgs = arguments(args)?;
    if args.ids.is_empty() || args.ids.len() > 100 {
        return Err(ToolError::new("Pass between 1 and 100 request ids."));
    }
    let allow = ctx.allow_secrets();
    let mask = move |name: &str, value: &str| redaction::header_value(name, value, allow);
    let contents = source.export(&args.ids, args.format, &mask).await?;
    Ok(ToolOutput::new(&ExportOut {
        format: args.format,
        contents: limits::truncate(redaction::text(&contents, allow)),
    }))
}
