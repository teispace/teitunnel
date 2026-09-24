//! The inspector as an analytics source: exact numbers for a route or share it's in
//! front of, from its captures (status classes and codes, p50/p95/p99 time to first
//! byte, bytes, paths, countries). The most precise source, so it's asked first.

use std::collections::HashMap;

use futures_util::future::BoxFuture;
use lens::{Exchange, Filter, Query, Redaction};

use super::{Inspector, TapScope};
use crate::analytics::{
    AnalyticsError, AnalyticsRange, AnalyticsSource, Percentiles, Ranked, RouteRef, RouteStats,
    SourceKind, StatsPart, StatsSeries, StatusClasses,
};

/// Top entries kept per breakdown.
const TOP: usize = 10;

/// This process's inspector, answering for the routes and shares it inspects.
#[derive(Debug, Clone)]
pub struct LensSource {
    inspector: Inspector,
}

impl LensSource {
    /// A source over `inspector`.
    pub fn new(inspector: Inspector) -> Self {
        Self { inspector }
    }

    /// Whether a running tap inspects `hostname` (a route, or a Quick Share's host).
    fn sees(&self, hostname: &str) -> bool {
        self.inspector.taps().iter().any(|tap| match &tap.scope {
            TapScope::Route { hostname: h, .. } => h.eq_ignore_ascii_case(hostname),
            TapScope::QuickShare { .. } => tap
                .public_url
                .as_deref()
                .and_then(|u| u.strip_prefix("https://"))
                .is_some_and(|h| h.eq_ignore_ascii_case(hostname)),
        })
    }
}

fn ranked(counts: HashMap<String, u64>) -> Vec<Ranked> {
    let mut out: Vec<Ranked> = counts
        .into_iter()
        .map(|(key, requests)| Ranked { key, requests })
        .collect();
    out.sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| a.key.cmp(&b.key)));
    out.truncate(TOP);
    out
}

fn percentile(sorted: &[f64], p: f64) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rank = ((p / 100.0) * (sorted.len() as f64)).ceil() as usize;
    sorted
        .get(rank.saturating_sub(1).min(sorted.len() - 1))
        .copied()
}

/// Statistics over captured exchanges (already filtered to the route and range).
pub(crate) fn stats(
    exchanges: &[&Exchange],
    route: &RouteRef,
    range: AnalyticsRange,
    now_ms: f64,
) -> RouteStats {
    let bucket_ms = range.bucket().seconds() * 1_000;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let now = now_ms as u64;
    let start = now.saturating_sub(range.seconds() * 1_000);
    let first_bucket = start / bucket_ms;
    let last_bucket = now / bucket_ms;
    let buckets = usize::try_from(last_bucket - first_bucket + 1).unwrap_or(1);
    let (mut requests, mut client, mut server, mut bytes) = (
        vec![0u64; buckets],
        vec![0u64; buckets],
        vec![0u64; buckets],
        vec![0u64; buckets],
    );
    let mut classes = StatusClasses::default();
    let (mut statuses, mut paths, mut countries) = (HashMap::new(), HashMap::new(), HashMap::new());
    let mut ttfb = Vec::new();
    let mut total_bytes = 0;
    for exchange in exchanges {
        let index = usize::try_from(exchange.started_at_ms / bucket_ms - first_bucket)
            .unwrap_or(0)
            .min(buckets - 1);
        requests[index] += 1;
        let size = exchange.response.as_ref().map_or(0, |r| r.body.size);
        bytes[index] += size;
        total_bytes += size;
        if let Some(status) = exchange.status() {
            let code = status.as_u16();
            match code / 100 {
                1 | 2 => classes.ok += 1,
                3 => classes.redirects += 1,
                4 => {
                    classes.client_errors += 1;
                    client[index] += 1;
                }
                _ => {
                    classes.server_errors += 1;
                    server[index] += 1;
                }
            }
            *statuses.entry(code.to_string()).or_insert(0) += 1;
        }
        *paths
            .entry(lens::mask_text(exchange.request.path(), &Redaction::masked()).into_owned())
            .or_insert(0) += 1;
        if let Some(country) = &exchange.client.country {
            *countries.entry(country.clone()).or_insert(0) += 1;
        }
        if let Some(first) = exchange.timings.first_byte_us {
            #[allow(clippy::cast_precision_loss)]
            ttfb.push(first as f64 / 1_000.0);
        }
    }
    ttfb.sort_by(f64::total_cmp);
    let mut series = StatsSeries::default();
    #[allow(clippy::cast_precision_loss)]
    for i in 0..buckets {
        let end = (first_bucket + i as u64 + 1) * bucket_ms;
        series.at.push(end as f64);
        series.span.push((bucket_ms / 1_000) as f64);
        series
            .requests
            .push(u32::try_from(requests[i]).unwrap_or(u32::MAX));
        series
            .client_errors
            .push(u32::try_from(client[i]).unwrap_or(u32::MAX));
        series
            .server_errors
            .push(u32::try_from(server[i]).unwrap_or(u32::MAX));
        series.bytes.push(bytes[i] as f64);
    }
    // The ring keeps the most recent exchanges: when full, older ones are missing.
    let oldest = exchanges.iter().map(|e| e.started_at_ms).min();
    #[allow(clippy::cast_precision_loss)]
    let available_from = oldest
        .filter(|oldest| exchanges.len() >= lens::DEFAULT_CAPACITY && *oldest > start)
        .map(|oldest| oldest as f64);
    RouteStats {
        source: SourceKind::Proxy,
        route: route.clone(),
        range,
        series,
        requests: exchanges.len() as u64,
        bytes: total_bytes,
        classes,
        statuses: ranked(statuses),
        paths: ranked(paths),
        countries: ranked(countries),
        browsers: Vec::new(),
        bots: Vec::new(),
        cache: Vec::new(),
        origin_ms: Some(Percentiles {
            p50: percentile(&ttfb, 50.0),
            p95: percentile(&ttfb, 95.0),
            p99: percentile(&ttfb, 99.0),
        })
        .filter(|p| p.p50.is_some()),
        ttfb_ms: None,
        available_from,
        unavailable: vec![StatsPart::Browsers, StatsPart::Bots, StatsPart::Cache],
        fetched_at: now_ms,
    }
}

impl AnalyticsSource for LensSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Proxy
    }

    fn route_stats<'a>(
        &'a self,
        route: &'a RouteRef,
        range: AnalyticsRange,
    ) -> BoxFuture<'a, Result<Option<RouteStats>, AnalyticsError>> {
        Box::pin(async move {
            if !self.sees(&route.hostname) {
                return Ok(None);
            }
            let now = crate::analytics::now_ms();
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let since = (now as u64).saturating_sub(range.seconds() * 1_000);
            let mut query = Query {
                filter: Filter {
                    host: Some(route.hostname.clone()),
                    since_ms: Some(since),
                    ..Filter::default()
                },
                limit: Some(lens::MAX_PAGE),
                before: None,
            };
            let mut all = Vec::new();
            loop {
                let page = self.inspector.list_raw(&query);
                all.extend(page.items.into_iter().filter(|e| {
                    e.replay_of.is_none()
                        && e.request.host.eq_ignore_ascii_case(&route.hostname)
                        && route
                            .path
                            .as_deref()
                            .is_none_or(|p| e.request.path().starts_with(p))
                }));
                match page.next {
                    Some(next) => query.before = Some(next),
                    None => break,
                }
            }
            let refs: Vec<&Exchange> = all.iter().map(AsRef::as_ref).collect();
            Ok(Some(stats(&refs, route, range, now)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::record::tests::sample;

    #[test]
    fn computes_classes_percentiles_and_breakdowns() {
        let now = crate::domain_shares::now_ms();
        let exchanges: Vec<Exchange> = (1..=10)
            .map(|i| {
                let mut e = sample("t", i, if i % 2 == 0 { "/a" } else { "/b" }, 200);
                if i > 8 {
                    e = sample("t", i, "/c", 503);
                }
                e.started_at_ms = now - 1_000 * i;
                e.timings.first_byte_us = Some(i * 1_000);
                e
            })
            .collect();
        let refs: Vec<&Exchange> = exchanges.iter().collect();
        let route = RouteRef {
            hostname: "app.example.com".into(),
            path: None,
        };
        #[allow(clippy::cast_precision_loss)]
        let stats = stats(&refs, &route, AnalyticsRange::Hour, now as f64);
        assert_eq!(stats.source, SourceKind::Proxy);
        assert_eq!(stats.requests, 10);
        assert_eq!(stats.classes.ok, 8);
        assert_eq!(stats.classes.server_errors, 2);
        assert_eq!(stats.series.requests.iter().sum::<u32>(), 10);
        assert_eq!(stats.statuses[0].key, "200");
        assert_eq!(stats.countries[0].key, "NL");
        let origin = stats.origin_ms.unwrap();
        assert_eq!(origin.p50, Some(5.0));
        assert_eq!(origin.p99, Some(10.0));
        assert!(stats.available_from.is_none());
    }
}
