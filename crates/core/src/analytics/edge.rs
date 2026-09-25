//! Edge analytics: Cloudflare's GraphQL Analytics per hostname, cached.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use cf_api::{Client, DatasetLimits, Part, Traffic, TrafficQuery};
use futures_util::future::BoxFuture;

use super::{
    AnalyticsError, AnalyticsRange, AnalyticsSource, AnalyticsSummary, HostSummary, Percentiles,
    RouteRef, RouteStats, SourceKind, StatsPart, classes_of, columns, now_ms, top,
};
use crate::accounts::Accounts;

/// How long a zone's plan limits are reused.
const LIMITS_TTL: Duration = Duration::from_secs(6 * 3_600);
/// How long an account's domain list is reused.
const ZONES_TTL: Duration = Duration::from_secs(10 * 60);
/// Queries per account per [`BUDGET_WINDOW`]: Cloudflare allows 300 per user; the
/// dashboard and other tools share them.
const BUDGET: usize = 200;
const BUDGET_WINDOW: Duration = Duration::from_secs(300);
/// Rows per breakdown in a route's detail.
const TOP: u32 = 10;
/// Keep this far from the plan's history limit, so a query sent a little later still
/// fits.
const HISTORY_MARGIN: u64 = 300;

/// A zone: id and domain name.
pub type ZoneName = (String, String);

#[derive(Debug, Clone)]
struct Fetched {
    traffic: Traffic,
    start: u64,
    end: u64,
    available_from: Option<u64>,
    fetched_at: f64,
}

#[derive(Debug, Default)]
struct Cache {
    zones: HashMap<String, (Instant, Vec<ZoneName>)>,
    limits: HashMap<String, (Instant, DatasetLimits)>,
    results: HashMap<String, (Instant, Arc<Fetched>)>,
    sent: HashMap<String, VecDeque<Instant>>,
}

/// Edge analytics with caching and a query budget per account. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct Analytics {
    cache: Arc<Mutex<Cache>>,
}

fn seconds_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The zone holding `host` (the longest matching domain).
fn zone_for<'a>(zones: &'a [ZoneName], host: &str) -> Option<&'a ZoneName> {
    let host = host.to_ascii_lowercase();
    zones
        .iter()
        .filter(|(_, name)| {
            let name = name.to_ascii_lowercase();
            host == name || host.ends_with(&format!(".{name}"))
        })
        .max_by_key(|(_, name)| name.len())
}

/// Whether the plan lists a field a part needs (an empty list means "all").
fn offered(limits: &DatasetLimits, part: Part) -> bool {
    let field = match part {
        Part::Errors | Part::Statuses => "edgeResponseStatus",
        Part::Paths => "clientRequestPath",
        Part::Countries => "clientCountryName",
        Part::Browsers => "userAgentBrowser",
        Part::Bots => "verifiedBotCategory",
        Part::Cache => "cacheStatus",
        Part::Latency => "originResponseDurationMs",
    };
    limits.available_fields.is_empty() || limits.available_fields.iter().any(|f| f.contains(field))
}

impl Analytics {
    fn lock(&self) -> std::sync::MutexGuard<'_, Cache> {
        self.cache.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes one query from `account`'s budget, or reports it's used up.
    fn spend(&self, account: &str, now: Instant) -> bool {
        let mut cache = self.lock();
        let sent = cache.sent.entry(account.to_owned()).or_default();
        while sent
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= BUDGET_WINDOW)
        {
            sent.pop_front();
        }
        if sent.len() >= BUDGET {
            return false;
        }
        sent.push_back(now);
        true
    }

    /// The account's domains (cached).
    async fn zones(
        &self,
        accounts: &Accounts,
        account: &str,
    ) -> Result<Vec<ZoneName>, AnalyticsError> {
        if let Some((at, zones)) = self.lock().zones.get(account)
            && at.elapsed() < ZONES_TTL
        {
            return Ok(zones.clone());
        }
        let zones: Vec<ZoneName> = accounts
            .domains(account)
            .await?
            .into_iter()
            .map(|d| (d.id, d.name))
            .collect();
        self.lock()
            .zones
            .insert(account.to_owned(), (Instant::now(), zones.clone()));
        Ok(zones)
    }

    /// Plan limits of `zones` (cached), fetched in one query for those not known.
    async fn limits(
        &self,
        client: &Client,
        account: &str,
        zones: &[String],
    ) -> Result<Vec<DatasetLimits>, AnalyticsError> {
        let missing: Vec<String> = {
            let cache = self.lock();
            zones
                .iter()
                .filter(|z| {
                    cache
                        .limits
                        .get(*z)
                        .is_none_or(|(at, _)| at.elapsed() >= LIMITS_TTL)
                })
                .cloned()
                .collect()
        };
        if !missing.is_empty() {
            if !self.spend(account, Instant::now()) {
                return Err(AnalyticsError::RateLimited);
            }
            let fetched = client.analytics_limits(&missing).await?;
            let mut cache = self.lock();
            for (zone, limits) in fetched {
                cache.limits.insert(zone, (Instant::now(), limits));
            }
        }
        let cache = self.lock();
        Ok(zones
            .iter()
            .filter_map(|z| cache.limits.get(z).map(|(_, l)| l.clone()))
            .collect())
    }

    /// Fetches (or reuses) the traffic of `hosts` over `range`.
    #[allow(clippy::too_many_arguments)]
    async fn fetch(
        &self,
        client: &Client,
        account: &str,
        zones: &[ZoneName],
        hosts: &[String],
        path: Option<&str>,
        range: AnalyticsRange,
        parts: &[Part],
        now: u64,
    ) -> Result<Arc<Fetched>, AnalyticsError> {
        let mut sorted: Vec<String> = hosts.iter().map(|h| h.to_ascii_lowercase()).collect();
        sorted.sort();
        sorted.dedup();
        let key = format!(
            "{account}|{range:?}|{}|{}|{parts:?}",
            path.unwrap_or(""),
            sorted.join(",")
        );
        let stale = self.lock().results.get(&key).cloned();
        if let Some((at, fetched)) = &stale
            && at.elapsed() < range.ttl()
        {
            return Ok(Arc::clone(fetched));
        }
        let result = self
            .query(client, account, zones, &sorted, path, range, parts, now)
            .await;
        match result {
            Ok(fetched) => {
                let fetched = Arc::new(fetched);
                self.lock()
                    .results
                    .insert(key, (Instant::now(), Arc::clone(&fetched)));
                Ok(fetched)
            }
            // Out of budget or offline: the last answer is better than none.
            Err(AnalyticsError::RateLimited | AnalyticsError::Api(cf_api::Error::Network(_)))
                if stale.is_some() =>
            {
                Ok(stale.map(|(_, f)| f).unwrap_or_default())
            }
            Err(err) => Err(err),
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn query(
        &self,
        client: &Client,
        account: &str,
        zones: &[ZoneName],
        hosts: &[String],
        path: Option<&str>,
        range: AnalyticsRange,
        parts: &[Part],
        now: u64,
    ) -> Result<Fetched, AnalyticsError> {
        let mut by_zone: Vec<(String, Vec<String>)> = Vec::new();
        for host in hosts {
            let Some((zone, _)) = zone_for(zones, host) else {
                continue;
            };
            match by_zone.iter_mut().find(|(z, _)| z == zone) {
                Some((_, list)) => list.push(host.clone()),
                None => by_zone.push((zone.clone(), vec![host.clone()])),
            }
        }
        let bucket = range.bucket().seconds();
        let end = now.div_ceil(60) * 60;
        let mut merged = Fetched {
            traffic: Traffic::default(),
            start: end.saturating_sub(range.seconds()),
            end,
            available_from: None,
            fetched_at: now_ms(),
        };
        for group in by_zone.chunks(cf_api::MAX_ZONES_PER_QUERY) {
            let zone_ids: Vec<String> = group.iter().map(|(z, _)| z.clone()).collect();
            let limits = self.limits(client, account, &zone_ids).await?;
            let enabled: Vec<&DatasetLimits> = limits.iter().filter(|l| l.enabled).collect();
            if enabled.is_empty() {
                return Err(AnalyticsError::NotOnPlan);
            }
            let history = enabled.iter().map(|l| l.not_older_than).min().unwrap_or(0);
            let max_duration = enabled.iter().map(|l| l.max_duration).min();
            let page_size = enabled
                .iter()
                .map(|l| l.max_page_size)
                .min()
                .unwrap_or(1_000);
            let mut start = end.saturating_sub(range.seconds());
            let oldest = end.saturating_sub(history.saturating_sub(HISTORY_MARGIN));
            if history > 0 && start < oldest {
                start = oldest.div_ceil(bucket) * bucket;
                merged.available_from = Some(start);
            }
            let (offered_parts, refused): (Vec<Part>, Vec<Part>) = parts
                .iter()
                .partition(|p| enabled.iter().all(|l| offered(l, **p)));
            let query = TrafficQuery {
                zones: zone_ids,
                hosts: group.iter().flat_map(|(_, h)| h.clone()).collect(),
                path_prefix: path.map(str::to_owned),
                start,
                end,
                bucket: range.bucket(),
                parts: offered_parts,
                top: TOP,
                page_size,
                max_duration,
            };
            if !self.spend(account, Instant::now()) {
                return Err(AnalyticsError::RateLimited);
            }
            let traffic = client.http_traffic(&query).await?;
            merged.traffic.series.extend(traffic.series);
            for (part, rows) in traffic.breakdowns {
                merged
                    .traffic
                    .breakdowns
                    .entry(part)
                    .or_default()
                    .extend(rows);
            }
            merged.traffic.latency.extend(traffic.latency);
            merged.traffic.unavailable.extend(traffic.unavailable);
            merged.traffic.unavailable.extend(refused);
        }
        merged.traffic.unavailable.sort();
        merged.traffic.unavailable.dedup();
        Ok(merged)
    }

    /// Every hostname's traffic side by side, from an account's zones.
    ///
    /// # Errors
    /// See [`AnalyticsError`].
    pub async fn summary(
        &self,
        accounts: &Accounts,
        account: &str,
        hosts: &[String],
        range: AnalyticsRange,
    ) -> Result<AnalyticsSummary, AnalyticsError> {
        let zones = self.zones(accounts, account).await?;
        let client = accounts.client(account).await?;
        self.summary_with(&client, account, &zones, hosts, range, seconds_now())
            .await
    }

    /// [`Analytics::summary`] with the client, zones and time given.
    ///
    /// # Errors
    /// See [`AnalyticsError`].
    pub async fn summary_with(
        &self,
        client: &Client,
        account: &str,
        zones: &[ZoneName],
        hosts: &[String],
        range: AnalyticsRange,
        now: u64,
    ) -> Result<AnalyticsSummary, AnalyticsError> {
        let parts = [Part::Errors, Part::Latency];
        let fetched = self
            .fetch(client, account, zones, hosts, None, range, &parts, now)
            .await?;
        let data_start = fetched.available_from.unwrap_or(fetched.start);
        let hosts = hosts
            .iter()
            .map(|host| {
                let series = columns(
                    &fetched.traffic.series,
                    host,
                    data_start,
                    fetched.end,
                    range.bucket(),
                );
                let requests: u64 = series.requests.iter().map(|n| u64::from(*n)).sum();
                let errors: u64 = series.server_errors.iter().map(|n| u64::from(*n)).sum();
                #[allow(clippy::cast_precision_loss)]
                let error_rate = (requests > 0
                    && !fetched.traffic.unavailable.contains(&Part::Errors))
                .then(|| errors as f64 / requests as f64);
                HostSummary {
                    hostname: host.clone(),
                    requests,
                    bytes: series.bytes.iter().sum::<f64>().max(0.0).round() as u64,
                    error_rate,
                    p95_ms: fetched
                        .traffic
                        .latency
                        .iter()
                        .find(|l| l.host.eq_ignore_ascii_case(host))
                        .and_then(|l| l.origin_ms[1]),
                    spark_errors: series.server_errors,
                    spark: series.requests,
                }
            })
            .collect();
        let bucket = range.bucket().seconds();
        Ok(AnalyticsSummary {
            range,
            hosts,
            available_from: fetched.available_from.map(|s| s as f64 * 1000.0),
            unavailable: fetched
                .traffic
                .unavailable
                .iter()
                .copied()
                .map(StatsPart::from)
                .collect(),
            ends_at: (fetched.end.div_ceil(bucket) * bucket) as f64 * 1000.0,
            bucket_seconds: u32::try_from(bucket).unwrap_or(u32::MAX),
            fetched_at: fetched.fetched_at,
        })
    }

    /// One route's traffic in detail.
    ///
    /// # Errors
    /// See [`AnalyticsError`].
    pub async fn route(
        &self,
        accounts: &Accounts,
        account: &str,
        route: &RouteRef,
        range: AnalyticsRange,
    ) -> Result<RouteStats, AnalyticsError> {
        let zones = self.zones(accounts, account).await?;
        let client = accounts.client(account).await?;
        self.route_with(&client, account, &zones, route, range, seconds_now())
            .await
    }

    /// [`Analytics::route`] with the client, zones and time given.
    ///
    /// # Errors
    /// See [`AnalyticsError`].
    pub async fn route_with(
        &self,
        client: &Client,
        account: &str,
        zones: &[ZoneName],
        route: &RouteRef,
        range: AnalyticsRange,
        now: u64,
    ) -> Result<RouteStats, AnalyticsError> {
        if zone_for(zones, &route.hostname).is_none() {
            return Err(AnalyticsError::NoZone(route.hostname.clone()));
        }
        let fetched = self
            .fetch(
                client,
                account,
                zones,
                std::slice::from_ref(&route.hostname),
                route.path.as_deref(),
                range,
                &Part::ALL,
                now,
            )
            .await?;
        let host = route.hostname.as_str();
        let breakdown = |part: Part, n: usize| top(fetched.traffic.breakdowns.get(&part), host, n);
        let data_start = fetched.available_from.unwrap_or(fetched.start);
        let series = columns(
            &fetched.traffic.series,
            host,
            data_start,
            fetched.end,
            range.bucket(),
        );
        let statuses = breakdown(Part::Statuses, 20);
        let mut classes = classes_of(&statuses);
        if statuses.is_empty() {
            classes.client_errors = series.client_errors.iter().map(|n| u64::from(*n)).sum();
            classes.server_errors = series.server_errors.iter().map(|n| u64::from(*n)).sum();
        }
        let latency = fetched
            .traffic
            .latency
            .iter()
            .find(|l| l.host.eq_ignore_ascii_case(host));
        Ok(RouteStats {
            source: SourceKind::Edge,
            route: route.clone(),
            range,
            requests: series.requests.iter().map(|n| u64::from(*n)).sum(),
            bytes: series.bytes.iter().sum::<f64>().max(0.0).round() as u64,
            series,
            classes,
            statuses,
            paths: breakdown(Part::Paths, 10),
            countries: breakdown(Part::Countries, 10),
            browsers: breakdown(Part::Browsers, 10),
            bots: breakdown(Part::Bots, 10),
            cache: breakdown(Part::Cache, 10),
            origin_ms: latency.and_then(|l| Percentiles::from_array(l.origin_ms)),
            ttfb_ms: latency.and_then(|l| Percentiles::from_array(l.ttfb_ms)),
            available_from: fetched.available_from.map(|s| s as f64 * 1000.0),
            unavailable: fetched
                .traffic
                .unavailable
                .iter()
                .copied()
                .map(StatsPart::from)
                .collect(),
            fetched_at: fetched.fetched_at,
        })
    }
}

impl Default for Fetched {
    fn default() -> Self {
        Self {
            traffic: Traffic::default(),
            start: 0,
            end: 0,
            available_from: None,
            fetched_at: 0.0,
        }
    }
}

/// [`Analytics`] for one account, as an [`AnalyticsSource`].
#[derive(Debug, Clone)]
pub struct EdgeSource {
    analytics: Analytics,
    accounts: Accounts,
    account: String,
}

impl EdgeSource {
    /// The edge's view of `account`'s routes.
    pub fn new(analytics: Analytics, accounts: Accounts, account: &str) -> Self {
        Self {
            analytics,
            accounts,
            account: account.to_owned(),
        }
    }
}

impl AnalyticsSource for EdgeSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Edge
    }

    fn route_stats<'a>(
        &'a self,
        route: &'a RouteRef,
        range: AnalyticsRange,
    ) -> BoxFuture<'a, Result<Option<RouteStats>, AnalyticsError>> {
        Box::pin(async move {
            match self
                .analytics
                .route(&self.accounts, &self.account, route, range)
                .await
            {
                Ok(stats) => Ok(Some(stats)),
                Err(AnalyticsError::NoZone(_)) => Ok(None),
                Err(err) => Err(err),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{Mock, MockServer, Request, ResponseTemplate, matchers::path};

    use super::*;

    const NOW: u64 = 1_790_000_000;

    fn zones() -> Vec<ZoneName> {
        vec![
            ("z1".into(), "xyz.com".into()),
            ("z2".into(), "dev.xyz.com".into()),
            ("z3".into(), "yx.com".into()),
        ]
    }

    fn limits(zone: &str, not_older_than: u64) -> serde_json::Value {
        serde_json::json!({"zoneTag": zone, "settings": {"httpRequestsAdaptiveGroups": {
            "enabled": true, "maxDuration": 86_400, "notOlderThan": not_older_than, "maxPageSize": 10_000,
            "availableFields": []
        }}})
    }

    async fn server(history: u64) -> (MockServer, Client) {
        let server = MockServer::start().await;
        Mock::given(path("/graphql"))
            .respond_with(move |request: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                let query = body["query"].as_str().unwrap();
                if query.contains("settings") {
                    return ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "data": {"viewer": {"zones": [limits("z1", history), limits("z3", history)]}}
                    }));
                }
                ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "data": {"viewer": {"zones": [{
                        "zoneTag": "z1",
                        "c0_series": [
                            {"count": 30, "sum": {"edgeResponseBytes": 1000}, "dimensions": {"datetimeMinute": cf_api::rfc3339(NOW - 120), "datetimeHour": cf_api::rfc3339(NOW - 120), "clientRequestHTTPHost": "app.xyz.com"}},
                            {"count": 10, "sum": {"edgeResponseBytes": 500}, "dimensions": {"datetimeMinute": cf_api::rfc3339(NOW - 60), "datetimeHour": cf_api::rfc3339(NOW - 60), "clientRequestHTTPHost": "app.xyz.com"}}
                        ],
                        "c0_e5": [{"count": 4, "dimensions": {"datetimeMinute": cf_api::rfc3339(NOW - 60), "datetimeHour": cf_api::rfc3339(NOW - 60), "clientRequestHTTPHost": "app.xyz.com"}}],
                        "c0_statuses": [
                            {"count": 36, "dimensions": {"clientRequestHTTPHost": "app.xyz.com", "edgeResponseStatus": 200}},
                            {"count": 4, "dimensions": {"clientRequestHTTPHost": "app.xyz.com", "edgeResponseStatus": 502}}
                        ],
                        "c0_countries": [{"count": 40, "dimensions": {"clientRequestHTTPHost": "app.xyz.com", "clientCountryName": "DE"}}]
                    }]}}
                }))
            })
            .mount(&server)
            .await;
        let client = Client::with_base(&server.uri(), cf_api::ApiToken::new("t")).unwrap();
        (server, client)
    }

    #[test]
    fn picks_the_most_specific_zone() {
        let zones = zones();
        assert_eq!(
            zone_for(&zones, "a.dev.xyz.com").map(|z| z.0.as_str()),
            Some("z2")
        );
        assert_eq!(
            zone_for(&zones, "XYZ.com").map(|z| z.0.as_str()),
            Some("z1")
        );
        assert_eq!(zone_for(&zones, "other.org"), None);
    }

    #[tokio::test]
    async fn summarises_every_host_and_caches_the_answer() {
        let (server, client) = server(30 * 86_400).await;
        let analytics = Analytics::default();
        let hosts = vec!["app.xyz.com".to_owned(), "api.yx.com".to_owned()];
        let summary = analytics
            .summary_with(&client, "a", &zones(), &hosts, AnalyticsRange::Hour, NOW)
            .await
            .unwrap();
        assert_eq!(summary.hosts.len(), 2);
        let app = &summary.hosts[0];
        assert_eq!(app.requests, 40);
        assert_eq!(app.error_rate, Some(0.1));
        assert_eq!(app.spark.len(), 60);
        assert_eq!(summary.hosts[1].requests, 0);
        assert_eq!(summary.hosts[1].error_rate, None);
        assert_eq!(summary.available_from, None);
        // Limits + traffic.
        assert_eq!(server.received_requests().await.unwrap().len(), 2);
        analytics
            .summary_with(&client, "a", &zones(), &hosts, AnalyticsRange::Hour, NOW)
            .await
            .unwrap();
        assert_eq!(
            server.received_requests().await.unwrap().len(),
            2,
            "the second read comes from the cache"
        );
    }

    #[tokio::test]
    async fn details_one_route_and_marks_short_history() {
        // The plan keeps two days: a week shows what there is.
        let (_server, client) = server(2 * 86_400).await;
        let analytics = Analytics::default();
        let route = RouteRef {
            hostname: "app.xyz.com".into(),
            path: None,
        };
        let stats = analytics
            .route_with(&client, "a", &zones(), &route, AnalyticsRange::Week, NOW)
            .await
            .unwrap();
        assert_eq!(stats.requests, 40);
        assert_eq!(stats.classes.server_errors, 4);
        assert_eq!(stats.classes.ok, 36);
        assert_eq!(stats.countries[0].key, "DE");
        let from = stats.available_from.unwrap();
        assert!(from > (NOW - 2 * 86_400) as f64 * 1000.0 && from < NOW as f64 * 1000.0);
        assert!(stats.series.at.len() < 7 * 24);

        let elsewhere = RouteRef {
            hostname: "a.other.org".into(),
            path: None,
        };
        assert!(matches!(
            analytics
                .route_with(
                    &client,
                    "a",
                    &zones(),
                    &elsewhere,
                    AnalyticsRange::Hour,
                    NOW
                )
                .await,
            Err(AnalyticsError::NoZone(_))
        ));
    }

    #[tokio::test]
    async fn a_missing_permission_is_reported_as_such() {
        let server = MockServer::start().await;
        Mock::given(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": null, "errors": [{"message": "zones ['z1'] are not authorized"}]
            })))
            .mount(&server)
            .await;
        let client = Client::with_base(&server.uri(), cf_api::ApiToken::new("t")).unwrap();
        let err = Analytics::default()
            .summary_with(
                &client,
                "a",
                &zones(),
                &["app.xyz.com".into()],
                AnalyticsRange::Day,
                NOW,
            )
            .await
            .unwrap_err();
        assert!(matches!(err, AnalyticsError::Permission), "{err:?}");
    }

    #[test]
    fn the_budget_runs_out_and_refills() {
        let analytics = Analytics::default();
        let start = Instant::now();
        for _ in 0..BUDGET {
            assert!(analytics.spend("a", start));
        }
        assert!(!analytics.spend("a", start));
        assert!(analytics.spend("b", start), "budgets are per account");
        assert!(analytics.spend("a", start + BUDGET_WINDOW));
    }
}
