//! The traffic tools over this process's inspector (Lens): what the host's shares and
//! inspected routes captured. Credentials are masked by Lens unless the server was
//! started with `--allow-secrets` (the server masks again either way).

use std::{collections::HashMap, str::FromStr, sync::Arc, time::Duration};

use base64::Engine as _;
use teitunnel_core::inspect::{
    Inspector, ReplayInput,
    lens::{
        self, Exchange as Captured, ExchangeId, ExchangeState, Filter, Query, Redaction, TapId,
    },
};

use crate::{
    backend::BoxFuture,
    traffic::{
        Body, Exchange, ExchangeSummary, HttpMessage, ReplayEdits, ReplayResult, TrafficError,
        TrafficFilter, TrafficFormat, TrafficPage, TrafficSource, TrafficStats,
    },
};

/// Exchanges looked at for statistics at most.
const STATS_LIMIT: usize = 5_000;

/// [`TrafficSource`] over an [`Inspector`].
#[derive(Debug, Clone)]
pub struct InspectorTraffic {
    inspector: Inspector,
    allow_secrets: bool,
}

impl InspectorTraffic {
    /// An OpenAPI description of what the inspector captured (live, restored and, with
    /// a database, other processes' history when the inspector hasn't started).
    async fn describe(
        &self,
        host: Option<&str>,
        title: Option<&str>,
    ) -> Result<(serde_json::Value, teitunnel_core::openapi::Summary), TrafficError> {
        let options = teitunnel_core::openapi::Options {
            host: host.map(str::to_owned),
            title: title.map(str::to_owned),
        };
        teitunnel_core::openapi::describe(Some(&self.inspector), self.inspector.store(), &options)
            .await
            .map_err(|e| TrafficError::Other(e.to_string()))
    }

    /// Traffic captured by `inspector`; `allow_secrets` shows credentials (the server's
    /// `--allow-secrets`).
    pub fn new(inspector: Inspector, allow_secrets: bool) -> Self {
        Self {
            inspector,
            allow_secrets,
        }
    }

    fn redaction(&self) -> Redaction {
        if self.allow_secrets {
            Redaction::revealed()
        } else {
            Redaction::masked()
        }
    }

    /// Lens's filter for an agent's filter.
    fn filter(&self, filter: &TrafficFilter) -> Filter {
        let mut out = Filter {
            methods: filter.method.iter().cloned().collect(),
            path: filter.path_contains.clone().filter(|p| !p.is_empty()),
            text: filter.text.clone().filter(|t| !t.is_empty()),
            min_duration_ms: filter.min_duration_ms,
            since_ms: filter.since_ms,
            ..Filter::default()
        };
        if let Some(status) = filter.status.as_deref().map(str::trim) {
            let lower = status.to_ascii_lowercase();
            match lower.strip_suffix("xx") {
                Some(class) => out.status_classes.extend(class.parse::<u8>().ok()),
                None => out.statuses.extend(lower.parse::<u16>().ok()),
            }
        }
        if let Some(scope) = filter
            .scope
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match TapId::new(scope)
                .ok()
                .filter(|tap| self.inspector.tap_name(tap).is_some())
            {
                Some(tap) => out.tap = Some(tap),
                None => {
                    let host = scope
                        .trim_start_matches("https://")
                        .trim_start_matches("http://")
                        .split('/')
                        .next()
                        .unwrap_or(scope);
                    out.host = Some(host.to_owned());
                }
            }
        }
        out
    }

    fn summary(&self, exchange: &Captured) -> ExchangeSummary {
        let request = &exchange.request;
        let redaction = self.redaction();
        let path = match request.query() {
            Some(query) => format!(
                "{}?{}",
                lens::mask_text(request.path(), &redaction),
                lens::mask_query(query, &redaction)
            ),
            None => lens::mask_text(request.path(), &redaction).into_owned(),
        };
        ExchangeSummary {
            id: exchange.id.to_string(),
            scope: self
                .inspector
                .tap_name(&exchange.tap)
                .unwrap_or_else(|| exchange.tap.to_string()),
            started_at_ms: exchange.started_at_ms,
            method: request.method.to_string(),
            host: request.host.clone(),
            path,
            status: exchange.status().map(|s| s.as_u16()),
            duration_ms: exchange
                .duration()
                .map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
            request_bytes: request.body.size,
            response_bytes: exchange.response.as_ref().map_or(0, |r| r.body.size),
            state: match exchange.state {
                ExchangeState::Failed => "error",
                ExchangeState::Pending | ExchangeState::Streaming => "pending",
                ExchangeState::Complete => "complete",
            }
            .to_owned(),
        }
    }

    fn captured(&self, id: &str) -> Result<Arc<Captured>, TrafficError> {
        ExchangeId::from_str(id.trim())
            .ok()
            .and_then(|id| self.inspector.exchange(id))
            .ok_or_else(|| TrafficError::NotFound(id.to_owned()))
    }

    /// Every capture matching `filter`, newest first, up to `limit`.
    fn matching(&self, filter: Filter, limit: usize) -> Vec<Arc<Captured>> {
        let mut query = Query {
            filter,
            limit: Some(lens::MAX_PAGE.min(limit)),
            before: None,
        };
        let mut out = Vec::new();
        loop {
            let page = self.inspector.list_raw(&query);
            out.extend(page.items);
            match page.next {
                Some(next) if out.len() < limit => query.before = Some(next),
                _ => break,
            }
        }
        out.truncate(limit);
        out
    }
}

fn message(view: &lens::RequestView, body_limit: usize) -> HttpMessage {
    HttpMessage {
        headers: view
            .headers
            .iter()
            .map(|h| (h.name.clone(), h.value.clone()))
            .collect(),
        body: body(&view.body, body_limit),
    }
}

fn body(view: &lens::BodyView, limit: usize) -> Body {
    let (encoding, data) = match (&view.text, &view.base64) {
        (Some(text), _) => ("utf8", text.clone()),
        (None, Some(b64)) => ("base64", b64.clone()),
        (None, None) => ("utf8", String::new()),
    };
    let mut data = data;
    let mut truncated = view.truncated;
    if data.len() > limit {
        if encoding == "base64" {
            // Cut the decoded bytes, then encode again.
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(&data)
                .unwrap_or_default();
            data =
                base64::engine::general_purpose::STANDARD.encode(&bytes[..bytes.len().min(limit)]);
        } else {
            let mut cut = limit;
            while !data.is_char_boundary(cut) {
                cut -= 1;
            }
            data.truncate(cut);
        }
        truncated = true;
    }
    Body {
        encoding: encoding.to_owned(),
        data,
        size: view.size,
        truncated,
    }
}

fn percentile(sorted: &[u64], p: f64) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted
        .get(rank.saturating_sub(1).min(sorted.len() - 1))
        .copied()
}

impl TrafficSource for InspectorTraffic {
    fn openapi<'a>(
        &'a self,
        host: Option<&'a str>,
        title: Option<&'a str>,
    ) -> BoxFuture<'a, Result<(serde_json::Value, teitunnel_core::openapi::Summary), TrafficError>>
    {
        Box::pin(self.describe(host, title))
    }

    fn list<'a>(
        &'a self,
        filter: &'a TrafficFilter,
        cursor: Option<&'a str>,
        limit: usize,
    ) -> BoxFuture<'a, Result<TrafficPage, TrafficError>> {
        Box::pin(async move {
            let before = match cursor {
                Some(cursor) => Some(
                    ExchangeId::from_str(cursor.trim())
                        .map_err(|_| TrafficError::Invalid(format!("{cursor} isn't a cursor")))?,
                ),
                None => None,
            };
            let page = self.inspector.list_raw(&Query {
                filter: self.filter(filter),
                limit: Some(limit),
                before,
            });
            Ok(TrafficPage {
                exchanges: page.items.iter().map(|e| self.summary(e)).collect(),
                next_cursor: page.next.map(|id| id.to_string()),
            })
        })
    }

    fn get<'a>(
        &'a self,
        id: &'a str,
        body_limit: usize,
    ) -> BoxFuture<'a, Result<Exchange, TrafficError>> {
        Box::pin(async move {
            let captured = self.captured(id)?;
            let view = captured.view(&self.redaction());
            Ok(Exchange {
                summary: self.summary(&captured),
                request: message(&view.request, body_limit),
                response: view.response.as_ref().map(|response| HttpMessage {
                    headers: response
                        .headers
                        .iter()
                        .map(|h| (h.name.clone(), h.value.clone()))
                        .collect(),
                    body: body(&response.body, body_limit),
                }),
                ttfb_ms: captured.timings.first_byte_us.map(|us| us / 1_000),
                error: captured.error.as_ref().map(|e| e.message.clone()),
            })
        })
    }

    fn replay<'a>(
        &'a self,
        id: &'a str,
        edits: &'a ReplayEdits,
        times: u32,
    ) -> BoxFuture<'a, Result<Vec<ReplayResult>, TrafficError>> {
        Box::pin(async move {
            let captured = self.captured(id)?;
            let rows = self
                .inspector
                .replay(
                    captured.id,
                    ReplayInput {
                        method: edits.method.clone(),
                        path: edits.path.clone(),
                        set_headers: edits.set_headers.clone(),
                        remove_headers: edits.remove_headers.clone(),
                        body: edits.body.clone(),
                        times: Some(times),
                        resign: false,
                    },
                )
                .await
                .map_err(|e| TrafficError::Invalid(e.to_string()))?;
            Ok(rows
                .iter()
                .filter_map(|row| self.inspector.exchange(row.id))
                .map(|e| ReplayResult {
                    exchange: self.summary(&e),
                })
                .collect())
        })
    }

    fn next_matching<'a>(
        &'a self,
        filter: &'a TrafficFilter,
    ) -> BoxFuture<'a, Result<ExchangeSummary, TrafficError>> {
        Box::pin(async move {
            let since = filter
                .since_ms
                .unwrap_or_else(teitunnel_core::domain_shares::now_ms);
            let lens_filter = self.filter(filter);
            // The caller bounds the wait; a day is "forever" here.
            let found = self
                .inspector
                .wait_for(&lens_filter, since, Duration::from_secs(24 * 3_600))
                .await
                .map_err(|e| TrafficError::Other(e.to_string()))?;
            Ok(self.summary(&found))
        })
    }

    fn stats<'a>(
        &'a self,
        filter: &'a TrafficFilter,
    ) -> BoxFuture<'a, Result<TrafficStats, TrafficError>> {
        Box::pin(async move {
            let all = self.matching(self.filter(filter), STATS_LIMIT);
            let mut by_status: HashMap<String, u64> = HashMap::new();
            let mut paths: HashMap<String, u64> = HashMap::new();
            let mut durations = Vec::new();
            let (mut request_bytes, mut response_bytes) = (0, 0);
            for exchange in &all {
                let class = match exchange.status() {
                    Some(status) => format!("{}xx", status.as_u16() / 100),
                    None if exchange.error.is_some() => "error".to_owned(),
                    None => continue,
                };
                *by_status.entry(class).or_default() += 1;
                *paths
                    .entry(
                        lens::mask_text(exchange.request.path(), &Redaction::masked()).into_owned(),
                    )
                    .or_default() += 1;
                if let Some(duration) = exchange.duration() {
                    durations.push(u64::try_from(duration.as_millis()).unwrap_or(u64::MAX));
                }
                request_bytes += exchange.request.body.size;
                response_bytes += exchange.response.as_ref().map_or(0, |r| r.body.size);
            }
            durations.sort_unstable();
            let mut by_status: Vec<(String, u64)> = by_status.into_iter().collect();
            by_status.sort();
            let mut top_paths: Vec<(String, u64)> = paths.into_iter().collect();
            top_paths.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
            top_paths.truncate(10);
            Ok(TrafficStats {
                count: all.len() as u64,
                by_status,
                p50_ms: percentile(&durations, 50.0),
                p95_ms: percentile(&durations, 95.0),
                p99_ms: percentile(&durations, 99.0),
                request_bytes,
                response_bytes,
                top_paths,
            })
        })
    }

    fn export<'a>(
        &'a self,
        ids: &'a [String],
        format: TrafficFormat,
        _mask: &'a (dyn Fn(&str, &str) -> String + Send + Sync),
    ) -> BoxFuture<'a, Result<String, TrafficError>> {
        Box::pin(async move {
            let exchanges = ids
                .iter()
                .map(|id| self.captured(id))
                .collect::<Result<Vec<_>, _>>()?;
            let format = match format {
                TrafficFormat::Curl => lens::export::ExportFormat::Curl,
                TrafficFormat::Har => lens::export::ExportFormat::Har,
                TrafficFormat::Markdown => lens::export::ExportFormat::Markdown,
            };
            Ok(teitunnel_core::inspect::export(
                &exchanges,
                format,
                &self.redaction(),
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use teitunnel_core::inspect::{TapScope, TapSpec};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn origin() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 8192];
                    let _ = socket.read(&mut buf).await;
                    let _ = socket
                        .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                        .await;
                });
            }
        });
        format!("http://{addr}")
    }

    async fn post(tap: &str, path: &str) {
        reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .post(format!("{tap}{path}"))
            .header("host", "hooks.example.com")
            .header("authorization", "Bearer s3cr3t-token-value")
            .body(r#"{"type":"payment.succeeded"}"#)
            .send()
            .await
            .unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn lists_gets_waits_and_exports_masked() {
        let origin = origin().await;
        let inspector = Inspector::new(None, None, "app");
        let tap = inspector
            .start(TapSpec::new(
                TapScope::QuickShare {
                    share_id: "qs-mcp1".into(),
                },
                "https://hooks.example.com",
                &origin,
            ))
            .await
            .unwrap();
        let source = InspectorTraffic::new(inspector.clone(), false);

        // Waiting resolves when the request arrives.
        let filter = TrafficFilter {
            method: Some("POST".into()),
            path_contains: Some("/webhooks".into()),
            since_ms: Some(teitunnel_core::domain_shares::now_ms()),
            ..TrafficFilter::default()
        };
        let waiting = tokio::spawn({
            let source = source.clone();
            let filter = filter.clone();
            async move { source.next_matching(&filter).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        post(&tap.address, "/other").await;
        post(&tap.address, "/webhooks/stripe").await;
        let found = tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(found.path, "/webhooks/stripe");
        assert_eq!(found.status, Some(201));

        let page = source
            .list(&TrafficFilter::default(), None, 10)
            .await
            .unwrap();
        assert_eq!(page.exchanges.len(), 2);
        let scoped = source
            .list(
                &TrafficFilter {
                    scope: Some("qs-mcp1".into()),
                    status: Some("2xx".into()),
                    ..TrafficFilter::default()
                },
                None,
                10,
            )
            .await
            .unwrap();
        assert_eq!(scoped.exchanges.len(), 2);

        let full = source.get(&found.id, 1024).await.unwrap();
        let auth = full
            .request
            .headers
            .iter()
            .find(|(n, _)| n == "authorization")
            .unwrap();
        assert!(!auth.1.contains("s3cr3t"), "{auth:?}");
        assert!(full.request.body.data.contains("payment.succeeded"));

        let stats = source.stats(&TrafficFilter::default()).await.unwrap();
        assert_eq!(stats.count, 2);
        assert_eq!(stats.by_status, [("2xx".to_owned(), 2)]);

        let mask = |_: &str, v: &str| v.to_owned();
        let har = source
            .export(std::slice::from_ref(&found.id), TrafficFormat::Har, &mask)
            .await
            .unwrap();
        assert!(har.contains("\"entries\"") && !har.contains("s3cr3t"));

        let replays = source
            .replay(&found.id, &ReplayEdits::default(), 2)
            .await
            .unwrap();
        assert_eq!(replays.len(), 2);

        // The agent's tool resolves as soon as a matching request arrives.
        let tools = crate::tools::CoreTools::new(
            crate::tools::tests::FakeBackend::new(),
            Arc::new(source.clone()),
            Arc::new(crate::plans::Plans::default()),
        );
        let ctx = crate::registry::ToolContext::detached(
            crate::tools::tests::settings(crate::config::Mode::ReadOnly),
            crate::tools::tests::actor(),
        );
        let waiting = tokio::spawn(async move {
            use crate::registry::ToolProvider as _;
            let args = serde_json::json!({ "pathContains": "/late", "timeoutSeconds": 30 });
            tools
                .call("wait_for_request", args.as_object().cloned().unwrap(), &ctx)
                .await
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        post(&tap.address, "/late").await;
        let out = tokio::time::timeout(Duration::from_secs(5), waiting)
            .await
            .unwrap()
            .unwrap()
            .unwrap()
            .structured;
        assert_eq!(out["outcome"], "received");
        assert_eq!(out["exchange"]["path"], "/late");
        assert!(
            !out.to_string().contains("s3cr3t-token-value"),
            "masked for the agent"
        );

        // The API, described from what was captured.
        let (document, summary) = source.openapi(None, Some("Demo")).await.unwrap();
        assert_eq!(document["info"]["title"], "Demo");
        assert!(summary.requests >= 3, "{summary:?}");
        assert!(document["paths"].as_object().is_some_and(|p| !p.is_empty()));
        assert!(!document.to_string().contains("s3cr3t"));

        // With --allow-secrets, credentials show.
        let open = InspectorTraffic::new(inspector.clone(), true);
        let full = open.get(&found.id, 1024).await.unwrap();
        assert!(
            full.request
                .headers
                .iter()
                .any(|(_, v)| v.contains("s3cr3t-token-value"))
        );
        inspector.shutdown().await;
    }
}
