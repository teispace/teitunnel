//! Traffic analytics per route: how many requests, which answers, how fast, from where.
//!
//! Numbers come from an [`AnalyticsSource`]. Today there are two:
//! - [`edge::EdgeSource`]: Cloudflare's GraphQL Analytics (the edge's view of every
//!   request, whichever machine served it; needs Zone ▸ Analytics ▸ Read), and
//! - [`connector::ConnectorSource`]: this machine's cloudflared metrics (precise and
//!   free, but per tunnel, not per hostname, and only while a connector runs here).
//!
//! A local inspecting proxy can add a third (per route, exact latencies) by implementing
//! the same trait. Edge results are cached per account, hostname set and range
//! ([`AnalyticsRange::ttl`]), so views polling them and the alert checks share queries
//! and stay far inside Cloudflare's budget.

pub mod connector;
pub mod edge;

use std::{collections::BTreeMap, sync::Arc, time::Duration};

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};

use crate::{
    accounts::AccountError,
    text::{Text, UserText, english_display, msg},
};

pub use edge::Analytics;

/// How far back to look.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum AnalyticsRange {
    /// The last hour, per minute.
    Hour,
    /// The last 24 hours, per 15 minutes.
    Day,
    /// The last 7 days, per hour.
    Week,
    /// The last 30 days, per day.
    Month,
}

impl AnalyticsRange {
    /// Seconds covered.
    pub const fn seconds(self) -> u64 {
        match self {
            Self::Hour => 3_600,
            Self::Day => 86_400,
            Self::Week => 7 * 86_400,
            Self::Month => 30 * 86_400,
        }
    }

    /// Series bucket size.
    pub const fn bucket(self) -> cf_api::Bucket {
        match self {
            Self::Hour => cf_api::Bucket::Minute,
            Self::Day => cf_api::Bucket::FifteenMinutes,
            Self::Week => cf_api::Bucket::Hour,
            Self::Month => cf_api::Bucket::Day,
        }
    }

    /// How long an edge answer is reused. Cloudflare's numbers settle over a few
    /// minutes anyway, and the budget is 300 queries per 5 minutes per user.
    pub const fn ttl(self) -> Duration {
        match self {
            Self::Hour => Duration::from_secs(60),
            Self::Day => Duration::from_secs(5 * 60),
            Self::Week => Duration::from_secs(15 * 60),
            Self::Month => Duration::from_secs(30 * 60),
        }
    }

    /// Parses `hour`, `day`, `week` or `month` (the CLI's `--range`).
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "hour" | "1h" => Some(Self::Hour),
            "day" | "24h" | "1d" => Some(Self::Day),
            "week" | "7d" => Some(Self::Week),
            "month" | "30d" => Some(Self::Month),
            _ => None,
        }
    }
}

/// Where numbers come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    /// Cloudflare's edge (GraphQL Analytics), per hostname, any connector.
    Edge,
    /// This machine's cloudflared metrics, per tunnel.
    Connector,
    /// A local inspecting proxy in front of the origin, per route.
    Proxy,
}

/// A part of the numbers that may be missing (not on the plan, or not measured).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum StatsPart {
    /// 4xx/5xx over time.
    Errors,
    /// Requests per status code.
    Statuses,
    /// Top paths.
    Paths,
    /// Top countries.
    Countries,
    /// Top browsers.
    Browsers,
    /// Verified bots.
    Bots,
    /// Cache status.
    Cache,
    /// Response time percentiles.
    Latency,
}

impl From<cf_api::Part> for StatsPart {
    fn from(part: cf_api::Part) -> Self {
        use cf_api::Part as P;
        match part {
            P::Errors => Self::Errors,
            P::Statuses => Self::Statuses,
            P::Paths => Self::Paths,
            P::Countries => Self::Countries,
            P::Browsers => Self::Browsers,
            P::Bots => Self::Bots,
            P::Cache => Self::Cache,
            P::Latency => Self::Latency,
        }
    }
}

/// A route to measure: a hostname and, for path rules, the path they start with.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteRef {
    /// Public hostname.
    pub hostname: String,
    /// Literal path prefix (e.g. `/api`), when the route has a path rule.
    pub path: Option<String>,
}

impl RouteRef {
    /// `app.xyz.com` or `app.xyz.com/api`: the key uptime and alerts use.
    pub fn key(&self) -> String {
        match &self.path {
            Some(path) => format!("{}{path}", self.hostname),
            None => self.hostname.clone(),
        }
    }
}

/// The literal path a path rule's regex starts with (`^/api/.*` → `/api/`), or `None`
/// when it doesn't start with one (`\.(png|jpg)$`): such a rule can't be probed or
/// filtered by prefix.
pub fn path_prefix(rule: &str) -> Option<String> {
    let rest = rule.strip_prefix('^')?;
    let prefix: String = rest
        .chars()
        .take_while(|c| !".*+?()[]{}|$\\^".contains(*c))
        .collect();
    prefix.starts_with('/').then_some(prefix)
}

/// Requests over time, as columns (entry `i` of every column is one bucket).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct StatsSeries {
    /// End of each bucket, milliseconds since the epoch; ascending.
    #[cfg_attr(feature = "specta", specta(type = Vec<u32>))]
    pub at: Vec<f64>,
    /// Seconds each bucket covers.
    #[cfg_attr(feature = "specta", specta(type = Vec<u32>))]
    pub span: Vec<f64>,
    /// Requests.
    pub requests: Vec<u32>,
    /// 4xx responses.
    pub client_errors: Vec<u32>,
    /// 5xx responses.
    pub server_errors: Vec<u32>,
    /// Bytes sent to visitors.
    #[cfg_attr(feature = "specta", specta(type = Vec<u32>))]
    pub bytes: Vec<f64>,
}

impl StatsSeries {
    fn push(&mut self, at_ms: f64, span: f64, requests: u64, client: u64, server: u64, bytes: u64) {
        self.at.push(at_ms);
        self.span.push(span);
        self.requests.push(clamp(requests));
        self.client_errors.push(clamp(client));
        self.server_errors.push(clamp(server));
        #[allow(clippy::cast_precision_loss)]
        self.bytes.push(bytes as f64);
    }
}

/// A value of a breakdown and its requests.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Ranked {
    /// The value: a path, a country, a status code, a browser, a cache status.
    pub key: String,
    /// Requests.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub requests: u64,
}

/// Percentiles in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Percentiles {
    /// Median.
    pub p50: Option<f64>,
    /// 95th percentile.
    pub p95: Option<f64>,
    /// 99th percentile.
    pub p99: Option<f64>,
}

impl Percentiles {
    fn from_array([p50, p95, p99]: [Option<f64>; 3]) -> Option<Self> {
        (p50.is_some() || p95.is_some() || p99.is_some()).then_some(Self { p50, p95, p99 })
    }
}

/// Responses by class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct StatusClasses {
    /// 1xx and 2xx.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub ok: u64,
    /// 3xx.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub redirects: u64,
    /// 4xx.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub client_errors: u64,
    /// 5xx.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub server_errors: u64,
}

/// Everything known about one route's traffic over a range.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteStats {
    /// Where the numbers come from.
    pub source: SourceKind,
    /// The route.
    pub route: RouteRef,
    /// The range asked for.
    pub range: AnalyticsRange,
    /// Requests over time.
    pub series: StatsSeries,
    /// Requests in the range.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub requests: u64,
    /// Bytes sent to visitors.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub bytes: u64,
    /// Responses by class (only 4xx and 5xx are known without [`StatsPart::Statuses`]).
    pub classes: StatusClasses,
    /// Requests per status code.
    pub statuses: Vec<Ranked>,
    /// Top paths.
    pub paths: Vec<Ranked>,
    /// Top countries.
    pub countries: Vec<Ranked>,
    /// Top browsers.
    pub browsers: Vec<Ranked>,
    /// Verified bot categories (the empty key: people and unverified bots).
    pub bots: Vec<Ranked>,
    /// Cache statuses.
    pub cache: Vec<Ranked>,
    /// Origin response time.
    pub origin_ms: Option<Percentiles>,
    /// Edge time to first byte.
    pub ttfb_ms: Option<Percentiles>,
    /// Data starts here, not at the range's start (the plan keeps less history);
    /// milliseconds since the epoch.
    pub available_from: Option<f64>,
    /// Parts the plan or the source doesn't offer.
    pub unavailable: Vec<StatsPart>,
    /// When the numbers were fetched, milliseconds since the epoch.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub fetched_at: f64,
}

/// One route's line in the overview.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct HostSummary {
    /// Public hostname.
    pub hostname: String,
    /// Requests in the range.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub requests: u64,
    /// Bytes sent.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub bytes: u64,
    /// Share of 5xx answers (0–1), when there were requests and it's known.
    pub error_rate: Option<f64>,
    /// Origin response time P95 (Pro and up).
    pub p95_ms: Option<f64>,
    /// Requests per bucket, for a sparkline.
    pub spark: Vec<u32>,
    /// 5xx answers per bucket (zeros when [`StatsPart::Errors`] is unavailable).
    pub spark_errors: Vec<u32>,
}

/// Every route of an account, side by side.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AnalyticsSummary {
    /// The range asked for.
    pub range: AnalyticsRange,
    /// One entry per hostname asked for, in the order asked.
    pub hosts: Vec<HostSummary>,
    /// Data starts here when the plan keeps less than the range.
    pub available_from: Option<f64>,
    /// Parts not on the plan.
    pub unavailable: Vec<StatsPart>,
    /// End of the last bucket of every sparkline, milliseconds since the epoch.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub ends_at: f64,
    /// Seconds per sparkline bucket.
    pub bucket_seconds: u32,
    /// When fetched.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub fetched_at: f64,
}

impl AnalyticsSummary {
    /// One host's sparklines as a series (for windowed sums).
    pub fn series(&self, host: &HostSummary) -> StatsSeries {
        let bucket = f64::from(self.bucket_seconds);
        let n = host.spark.len();
        #[allow(clippy::cast_precision_loss)]
        let at = (0..n)
            .map(|i| self.ends_at - (n - 1 - i) as f64 * bucket * 1000.0)
            .collect();
        StatsSeries {
            at,
            span: vec![bucket; n],
            requests: host.spark.clone(),
            client_errors: vec![0; n],
            server_errors: host.spark_errors.clone(),
            bytes: vec![0.0; n],
        }
    }
}

/// Why analytics couldn't be shown.
#[derive(Debug, thiserror::Error)]
pub enum AnalyticsError {
    /// The credential can't read analytics (Zone ▸ Analytics ▸ Read).
    Permission,
    /// Cloudflare's query budget is used up for a few minutes.
    RateLimited,
    /// The zone's plan doesn't offer HTTP analytics.
    NotOnPlan,
    /// None of the account's domains holds this hostname.
    NoZone(String),
    /// The account is gone or its credential can't be read.
    Account(#[from] AccountError),
    /// Cloudflare answered with an error.
    Api(cf_api::Error),
    /// The local database failed.
    Store(#[from] crate::store::StoreError),
}

impl From<cf_api::Error> for AnalyticsError {
    fn from(err: cf_api::Error) -> Self {
        if err.is_rate_limited() {
            Self::RateLimited
        } else if err.is_auth() {
            Self::Permission
        } else {
            Self::Api(err)
        }
    }
}

impl UserText for AnalyticsError {
    fn text(&self) -> Text {
        use msg::error::analytics as m;
        match self {
            Self::Permission => m::permission(),
            Self::RateLimited => m::rate_limited(),
            Self::NotOnPlan => m::not_on_plan(),
            Self::NoZone(hostname) => m::no_zone(hostname),
            Self::Account(err) => err.text(),
            Self::Api(err) => err.text(),
            Self::Store(err) => err.text(),
        }
    }
}

english_display!(AnalyticsError);

/// A source of route statistics. Implementations answer `Ok(None)` for a route they
/// can't see (a connector for a hostname on another machine, a proxy that isn't in
/// front of it).
pub trait AnalyticsSource: Send + Sync {
    /// What this source is.
    fn kind(&self) -> SourceKind;

    /// The route's numbers over `range`.
    fn route_stats<'a>(
        &'a self,
        route: &'a RouteRef,
        range: AnalyticsRange,
    ) -> BoxFuture<'a, Result<Option<RouteStats>, AnalyticsError>>;
}

/// The first source that can see the route, in the order given (most precise first).
///
/// # Errors
/// The first error, if no source could answer.
pub async fn first_answer(
    sources: &[Arc<dyn AnalyticsSource>],
    route: &RouteRef,
    range: AnalyticsRange,
) -> Result<Option<RouteStats>, AnalyticsError> {
    let mut first_error = None;
    for source in sources {
        match source.route_stats(route, range).await {
            Ok(Some(stats)) => return Ok(Some(stats)),
            Ok(None) => {}
            Err(err) => {
                first_error.get_or_insert(err);
            }
        }
    }
    first_error.map_or(Ok(None), Err)
}

/// Requests and 5xx answers over the last `minutes` of a series (for error-rate alerts).
pub fn recent_errors(series: &StatsSeries, now_ms: f64, minutes: u32) -> (u64, u64) {
    let from = now_ms - f64::from(minutes) * 60_000.0;
    series
        .at
        .iter()
        .enumerate()
        // A bucket counts when it ended inside the window (partly-inside buckets count
        // whole: short windows over coarse buckets would otherwise see nothing).
        .filter(|(_, at)| **at > from)
        .fold((0, 0), |(requests, errors), (i, _)| {
            (
                requests + u64::from(series.requests.get(i).copied().unwrap_or(0)),
                errors + u64::from(series.server_errors.get(i).copied().unwrap_or(0)),
            )
        })
}

fn clamp(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Milliseconds since the epoch.
pub(crate) fn now_ms() -> f64 {
    crate::traffic::now_ms()
}

/// Top `n` of a breakdown for one hostname.
fn top(rows: Option<&Vec<cf_api::BreakdownRow>>, host: &str, n: usize) -> Vec<Ranked> {
    rows.into_iter()
        .flatten()
        .filter(|r| r.host.eq_ignore_ascii_case(host))
        .take(n)
        .map(|r| Ranked {
            key: r.key.clone(),
            requests: r.requests,
        })
        .collect()
}

/// Sums per-class counts from per-status rows.
fn classes_of(statuses: &[Ranked]) -> StatusClasses {
    let mut classes = StatusClasses::default();
    for row in statuses {
        match row.key.parse::<u16>().unwrap_or(0) {
            300..=399 => classes.redirects += row.requests,
            400..=499 => classes.client_errors += row.requests,
            500..=599 => classes.server_errors += row.requests,
            _ => classes.ok += row.requests,
        }
    }
    classes
}

/// Rows of a series for one hostname, as columns with a bucket for every step of the
/// range (a quiet minute is a zero, not a gap).
fn columns(
    rows: &[cf_api::SeriesRow],
    host: &str,
    start: u64,
    end: u64,
    bucket: cf_api::Bucket,
) -> StatsSeries {
    let size = bucket.seconds();
    let mut by_bucket: BTreeMap<u64, (u64, u64, u64, u64)> = BTreeMap::new();
    for row in rows.iter().filter(|r| r.host.eq_ignore_ascii_case(host)) {
        let slot = by_bucket.entry(row.at / size * size).or_default();
        slot.0 += row.requests;
        slot.1 += row.client_errors;
        slot.2 += row.server_errors;
        slot.3 += row.bytes;
    }
    let mut series = StatsSeries::default();
    let mut at = start / size * size;
    #[allow(clippy::cast_precision_loss)]
    while at < end {
        let (requests, client, server, bytes) = by_bucket.get(&at).copied().unwrap_or_default();
        series.push(
            ((at + size) * 1000) as f64,
            size as f64,
            requests,
            client,
            server,
            bytes,
        );
        at += size;
    }
    series
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_literal_prefixes_of_path_rules() {
        assert_eq!(path_prefix("^/api/.*").as_deref(), Some("/api/"));
        assert_eq!(path_prefix("^/api").as_deref(), Some("/api"));
        assert_eq!(path_prefix("^/").as_deref(), Some("/"));
        assert_eq!(path_prefix("\\.(png|jpg)$"), None);
        assert_eq!(path_prefix("^(a|b)"), None);
    }

    #[test]
    fn ranges_parse_and_pick_buckets() {
        assert_eq!(AnalyticsRange::parse("Week"), Some(AnalyticsRange::Week));
        assert_eq!(AnalyticsRange::parse("30d"), Some(AnalyticsRange::Month));
        assert_eq!(AnalyticsRange::parse("year"), None);
        assert_eq!(AnalyticsRange::Day.bucket(), cf_api::Bucket::FifteenMinutes);
        assert!(AnalyticsRange::Hour.ttl() < AnalyticsRange::Month.ttl());
    }

    #[test]
    fn fills_every_bucket_and_counts_classes() {
        let row = |at, requests, server| cf_api::SeriesRow {
            host: "a.x.com".into(),
            at,
            requests,
            bytes: 10,
            client_errors: 0,
            server_errors: server,
        };
        let series = columns(
            &[row(60, 5, 1), row(180, 2, 0), row(120, 1, 0)],
            "A.x.com",
            30,
            240,
            cf_api::Bucket::Minute,
        );
        assert_eq!(series.at, [60_000.0, 120_000.0, 180_000.0, 240_000.0]);
        assert_eq!(series.requests, [0, 5, 1, 2]);
        assert_eq!(series.server_errors, [0, 1, 0, 0]);
        assert_eq!(recent_errors(&series, 240_000.0, 2), (3, 0));
        assert_eq!(recent_errors(&series, 240_000.0, 3), (8, 1));

        let classes = classes_of(&[
            Ranked {
                key: "200".into(),
                requests: 7,
            },
            Ranked {
                key: "301".into(),
                requests: 1,
            },
            Ranked {
                key: "404".into(),
                requests: 2,
            },
            Ranked {
                key: "502".into(),
                requests: 3,
            },
        ]);
        assert_eq!(
            classes,
            StatusClasses {
                ok: 7,
                redirects: 1,
                client_errors: 2,
                server_errors: 3
            }
        );
    }

    struct Fixed(Option<SourceKind>, bool);
    impl AnalyticsSource for Fixed {
        fn kind(&self) -> SourceKind {
            self.0.unwrap_or(SourceKind::Proxy)
        }
        fn route_stats<'a>(
            &'a self,
            route: &'a RouteRef,
            range: AnalyticsRange,
        ) -> BoxFuture<'a, Result<Option<RouteStats>, AnalyticsError>> {
            Box::pin(async move {
                if self.1 {
                    return Err(AnalyticsError::RateLimited);
                }
                Ok(self.0.map(|source| RouteStats {
                    source,
                    route: route.clone(),
                    range,
                    series: StatsSeries::default(),
                    requests: 0,
                    bytes: 0,
                    classes: StatusClasses::default(),
                    statuses: Vec::new(),
                    paths: Vec::new(),
                    countries: Vec::new(),
                    browsers: Vec::new(),
                    bots: Vec::new(),
                    cache: Vec::new(),
                    origin_ms: None,
                    ttfb_ms: None,
                    available_from: None,
                    unavailable: Vec::new(),
                    fetched_at: 0.0,
                }))
            })
        }
    }

    #[tokio::test]
    async fn the_first_source_that_sees_a_route_answers() {
        let route = RouteRef {
            hostname: "a.x.com".into(),
            path: None,
        };
        let sources: Vec<Arc<dyn AnalyticsSource>> = vec![
            Arc::new(Fixed(None, false)),
            Arc::new(Fixed(None, true)),
            Arc::new(Fixed(Some(SourceKind::Connector), false)),
        ];
        let stats = first_answer(&sources, &route, AnalyticsRange::Hour)
            .await
            .unwrap();
        assert_eq!(stats.map(|s| s.source), Some(SourceKind::Connector));
        let failing: Vec<Arc<dyn AnalyticsSource>> = vec![Arc::new(Fixed(None, true))];
        assert!(matches!(
            first_answer(&failing, &route, AnalyticsRange::Hour).await,
            Err(AnalyticsError::RateLimited)
        ));
        assert_eq!(route.key(), "a.x.com");
    }
}
