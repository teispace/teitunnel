//! Cloudflare's GraphQL Analytics API (`POST /graphql`): the edge's view of a hostname's
//! traffic, from the zone-scoped `httpRequestsAdaptiveGroups` dataset.
//!
//! Facts this relies on are in `docs/research/cloudflare-analytics.md`: the dataset and
//! field names, the per-plan limits exposed by the `settings` node (`maxDuration`,
//! `notOlderThan`, `maxPageSize`), the 300 queries per 5 minutes budget, up to 10 zones
//! per query, and that errors arrive as `{"data": …, "errors": [{"message", "path"}]}`,
//! usually with HTTP 200.
//!
//! One request covers every hostname in up to 10 zones: each part of the answer is an
//! aliased selection of the same dataset, grouped by hostname. A range wider than the
//! plan's `maxDuration` is split into chunks inside the same request. Parts the plan
//! doesn't offer (latency quantiles on Free, for example) are dropped and reported in
//! [`Traffic::unavailable`] instead of failing the whole query.

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::{ApiMessage, Client, Error, Result};

/// The dataset every query reads.
pub const HTTP_DATASET: &str = "httpRequestsAdaptiveGroups";
/// Zones one query may cover (Cloudflare's limit).
pub const MAX_ZONES_PER_QUERY: usize = 10;
/// Chunks one request may hold when a range is wider than the plan allows in one go.
const MAX_CHUNKS: usize = 8;
/// Retries after dropping parts the plan refused.
const MAX_DROPS: usize = 2;

/// The size of one time bucket of a series.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bucket {
    /// One minute (`datetimeMinute`).
    Minute,
    /// Five minutes (`datetimeFiveMinutes`).
    FiveMinutes,
    /// Fifteen minutes (`datetimeFifteenMinutes`).
    FifteenMinutes,
    /// One hour (`datetimeHour`).
    Hour,
    /// One day, UTC (`date`).
    Day,
}

impl Bucket {
    /// Seconds per bucket.
    pub const fn seconds(self) -> u64 {
        match self {
            Self::Minute => 60,
            Self::FiveMinutes => 300,
            Self::FifteenMinutes => 900,
            Self::Hour => 3_600,
            Self::Day => 86_400,
        }
    }

    const fn dimension(self) -> &'static str {
        match self {
            Self::Minute => "datetimeMinute",
            Self::FiveMinutes => "datetimeFiveMinutes",
            Self::FifteenMinutes => "datetimeFifteenMinutes",
            Self::Hour => "datetimeHour",
            Self::Day => "date",
        }
    }
}

/// An optional part of a traffic query. Plans differ in what they offer; a part the
/// plan refuses is left out of the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Part {
    /// 4xx and 5xx counts per bucket.
    Errors,
    /// Requests per response status.
    Statuses,
    /// Requests per path.
    Paths,
    /// Requests per visitor country.
    Countries,
    /// Requests per browser (from the user agent).
    Browsers,
    /// Requests per verified bot category (empty: not a verified bot).
    Bots,
    /// Requests per cache status.
    Cache,
    /// Origin response time and edge time to first byte, P50/P95/P99 (Pro and up).
    Latency,
}

impl Part {
    /// Every part.
    pub const ALL: [Self; 8] = [
        Self::Errors,
        Self::Statuses,
        Self::Paths,
        Self::Countries,
        Self::Browsers,
        Self::Bots,
        Self::Cache,
        Self::Latency,
    ];

    const fn alias(self) -> &'static str {
        match self {
            Self::Errors => "errors",
            Self::Statuses => "statuses",
            Self::Paths => "paths",
            Self::Countries => "countries",
            Self::Browsers => "browsers",
            Self::Bots => "bots",
            Self::Cache => "cache",
            Self::Latency => "latency",
        }
    }

    /// The dimension a breakdown groups by.
    const fn dimension(self) -> Option<&'static str> {
        match self {
            Self::Statuses => Some("edgeResponseStatus"),
            Self::Paths => Some("clientRequestPath"),
            Self::Countries => Some("clientCountryName"),
            Self::Browsers => Some("userAgentBrowser"),
            Self::Bots => Some("verifiedBotCategory"),
            Self::Cache => Some("cacheStatus"),
            Self::Errors | Self::Latency => None,
        }
    }

    fn from_alias(alias: &str) -> Option<Self> {
        match alias {
            "e4" | "e5" => Some(Self::Errors),
            other => Self::ALL.into_iter().find(|p| p.alias() == other),
        }
    }
}

/// What to ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrafficQuery {
    /// Zone ids (at most [`MAX_ZONES_PER_QUERY`]).
    pub zones: Vec<String>,
    /// Hostnames to count.
    pub hosts: Vec<String>,
    /// Only requests whose path starts with this.
    pub path_prefix: Option<String>,
    /// Start, in seconds since the epoch (inclusive).
    pub start: u64,
    /// End, in seconds since the epoch (exclusive).
    pub end: u64,
    /// Series bucket size.
    pub bucket: Bucket,
    /// Optional parts.
    pub parts: Vec<Part>,
    /// Rows per breakdown per hostname.
    pub top: u32,
    /// The plan's largest page (`maxPageSize`).
    pub page_size: u32,
    /// The plan's widest range in one selection (`maxDuration`, seconds).
    pub max_duration: Option<u64>,
}

/// One bucket of one hostname.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeriesRow {
    /// Hostname.
    pub host: String,
    /// Bucket start, seconds since the epoch.
    pub at: u64,
    /// Requests.
    pub requests: u64,
    /// Bytes sent to visitors.
    pub bytes: u64,
    /// 4xx responses (0 when [`Part::Errors`] is unavailable).
    pub client_errors: u64,
    /// 5xx responses.
    pub server_errors: u64,
}

/// Requests of one hostname with one value of a breakdown's dimension.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BreakdownRow {
    /// Hostname.
    pub host: String,
    /// The dimension's value, e.g. `/api`, `DE`, `200`, `hit`.
    pub key: String,
    /// Requests.
    pub requests: u64,
}

/// Latency percentiles of one hostname, in milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct LatencyRow {
    /// Hostname.
    pub host: String,
    /// Requests the percentiles cover.
    pub requests: u64,
    /// Origin response time P50, P95, P99.
    pub origin_ms: [Option<f64>; 3],
    /// Edge time to first byte P50, P95, P99.
    pub ttfb_ms: [Option<f64>; 3],
}

/// The answer to a [`TrafficQuery`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Traffic {
    /// Buckets per hostname, oldest first.
    pub series: Vec<SeriesRow>,
    /// Breakdowns by part, largest first.
    pub breakdowns: BTreeMap<Part, Vec<BreakdownRow>>,
    /// Percentiles per hostname.
    pub latency: Vec<LatencyRow>,
    /// Parts the plan (or the token) didn't allow.
    pub unavailable: Vec<Part>,
}

/// What a plan allows for a dataset (the `settings` node).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DatasetLimits {
    /// Whether the dataset is available at all.
    pub enabled: bool,
    /// Widest range in one selection, seconds.
    pub max_duration: u64,
    /// How far back data goes, seconds.
    pub not_older_than: u64,
    /// Largest `limit`.
    pub max_page_size: u32,
    /// Fields the requester may use (may be empty: then assume all).
    #[serde(default)]
    pub available_fields: Vec<String>,
}

/// One GraphQL error.
#[derive(Debug, Clone, Deserialize)]
struct GraphQlError {
    message: String,
    #[serde(default)]
    path: Option<Vec<Value>>,
}

#[derive(Debug, Deserialize)]
struct GraphQlResponse {
    data: Option<Value>,
    #[serde(default)]
    errors: Option<Vec<GraphQlError>>,
}

/// Seconds since the epoch as `2026-09-24T10:00:00Z`.
pub fn rfc3339(secs: u64) -> String {
    let days = i64::try_from(secs / 86_400).unwrap_or(i64::MAX);
    let rest = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rest / 3_600,
        rest % 3_600 / 60,
        rest % 60
    )
}

/// Parses `2026-09-24T10:00:00Z` or `2026-09-24` into seconds since the epoch.
pub fn parse_time(value: &str) -> Option<u64> {
    let date = value.get(..10)?;
    let mut parts = date.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: u32 = parts.next()?.parse().ok()?;
    let d: u32 = parts.next()?.parse().ok()?;
    let days = u64::try_from(days_from_civil(y, m, d)).ok()?;
    let seconds = match value.get(10..) {
        None | Some("") => 0,
        Some(time) => {
            let time = time.trim_start_matches('T').trim_end_matches('Z');
            let mut hms = time.split(':');
            let h: u64 = hms.next()?.parse().ok()?;
            let min: u64 = hms.next().unwrap_or("0").parse().ok()?;
            let s: u64 = hms.next().unwrap_or("0").split('.').next()?.parse().ok()?;
            h * 3_600 + min * 60 + s
        }
    };
    Some(days * 86_400 + seconds)
}

// Howard Hinnant's civil calendar algorithms (public domain).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(m);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// A GraphQL string literal (JSON's escaping is valid GraphQL).
fn literal(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}

fn list(values: &[String]) -> String {
    format!(
        "[{}]",
        values
            .iter()
            .map(|v| literal(v))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// Maps unhandled GraphQL errors onto the REST error shape, with a status that says what
/// kind of problem it was: 429 for the budget, 403 for permissions, 503 for "try again",
/// 400 otherwise (a range the plan doesn't allow, a field it doesn't offer).
fn to_error(errors: &[GraphQlError]) -> Error {
    let lower: Vec<String> = errors.iter().map(|e| e.message.to_lowercase()).collect();
    let any = |needles: &[&str]| {
        lower
            .iter()
            .any(|m| needles.iter().any(|needle| m.contains(needle)))
    };
    let status = if any(&["rate limiter", "too many nodes", "excessive resources"]) {
        429
    } else if any(&[
        "unauthorized",
        "not authorized",
        "does not have access",
        "authentication",
    ]) {
        403
    } else if any(&["try again later", "internal server error"]) {
        503
    } else {
        400
    };
    Error::Api {
        status,
        errors: errors
            .iter()
            .map(|e| ApiMessage {
                code: 0,
                message: e.message.clone(),
            })
            .collect(),
    }
}

impl Error {
    /// Whether Cloudflare's query budget is used up (retry in a few minutes).
    pub fn is_rate_limited(&self) -> bool {
        self.status() == Some(429)
    }
}

/// The part an error is about, from its path (`["viewer","zones",0,"c0_paths"]`) or
/// message (`… path "viewer.zones.0.c0_latency.quantiles"`).
fn part_of(error: &GraphQlError) -> Option<Part> {
    let from_name = |name: &str| {
        let alias = name.split_once('_').map(|(chunk, alias)| {
            (
                chunk.starts_with('c') && chunk[1..].chars().all(|c| c.is_ascii_digit()),
                alias,
            )
        })?;
        if !alias.0 {
            return None;
        }
        Part::from_alias(alias.1)
    };
    let from_path = error
        .path
        .iter()
        .flatten()
        .filter_map(Value::as_str)
        .find_map(from_name);
    from_path.or_else(|| {
        error
            .message
            .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .find_map(from_name)
    })
}

impl Client {
    /// Sends a GraphQL query and returns its `data`.
    ///
    /// # Errors
    /// GraphQL errors (mapped onto [`Error::Api`], see [`to_error`]), HTTP errors,
    /// network failures, or a body that isn't a GraphQL response.
    async fn graphql(&self, query: &str) -> Result<Value> {
        let body = serde_json::json!({ "query": query });
        let (status, bytes) = self.post_graphql(&body).await?;
        let (data, errors) = self.decode_graphql(status, &bytes)?;
        if errors.is_empty() {
            return data.ok_or(Error::Api {
                status,
                errors: Vec::new(),
            });
        }
        Err(to_error(&errors))
    }

    #[allow(clippy::unused_self)]
    fn decode_graphql(
        &self,
        status: u16,
        bytes: &[u8],
    ) -> Result<(Option<Value>, Vec<GraphQlError>)> {
        if status == 429 {
            return Err(Error::Api {
                status,
                errors: Vec::new(),
            });
        }
        match serde_json::from_slice::<GraphQlResponse>(bytes) {
            Ok(response) => {
                let errors = response.errors.unwrap_or_default();
                if !(200..300).contains(&status) && errors.is_empty() {
                    return Err(Error::Api {
                        status,
                        errors: Vec::new(),
                    });
                }
                Ok((response.data, errors))
            }
            Err(err) if (200..300).contains(&status) => Err(Error::Decode(err)),
            // Not GraphQL: a REST envelope (auth failures) or a proxy's error page.
            Err(_) => Err(crate::Envelope::<Value>::check(status, bytes)
                .err()
                .unwrap_or(Error::Api {
                    status,
                    errors: Vec::new(),
                })),
        }
    }

    /// What each zone's plan allows for [`HTTP_DATASET`], by zone id.
    ///
    /// # Errors
    /// See [`Client::graphql`]; a zone the token can't read analytics for fails the query.
    pub async fn analytics_limits(&self, zones: &[String]) -> Result<Vec<(String, DatasetLimits)>> {
        let query = format!(
            "{{viewer{{zones(filter:{{zoneTag_in:{}}}){{zoneTag settings{{{HTTP_DATASET}{{enabled maxDuration notOlderThan maxPageSize availableFields}}}}}}}}}}",
            list(zones)
        );
        let data = self.graphql(&query).await?;
        let mut out = Vec::new();
        for zone in zone_nodes(&data) {
            let Some(tag) = zone.get("zoneTag").and_then(Value::as_str) else {
                continue;
            };
            let Some(settings) = zone.pointer(&format!("/settings/{HTTP_DATASET}")) else {
                continue;
            };
            out.push((tag.to_owned(), serde_json::from_value(settings.clone())?));
        }
        Ok(out)
    }

    /// Whether the credential may read `zone_id`'s analytics (Zone ▸ Analytics ▸ Read).
    /// Reads only the zone's limits, so it's cheap and changes nothing.
    pub async fn probe_analytics(&self, zone_id: &str) -> crate::Access {
        match self.analytics_limits(&[zone_id.to_owned()]).await {
            Ok(limits) if limits.is_empty() => crate::Access::Denied,
            Ok(_) => crate::Access::Allowed,
            Err(err) if err.is_auth() => crate::Access::Denied,
            Err(_) => crate::Access::Unknown,
        }
    }

    /// The edge's view of the query's hostnames. Parts the plan refuses are dropped (up
    /// to twice) and listed in [`Traffic::unavailable`].
    ///
    /// # Errors
    /// See [`Client::graphql`]; the series itself failing fails the query.
    pub async fn http_traffic(&self, query: &TrafficQuery) -> Result<Traffic> {
        let mut parts = query.parts.clone();
        let mut unavailable = Vec::new();
        let chunks = chunks(query);
        for _ in 0..=MAX_DROPS {
            let text = build(query, &parts, &chunks);
            let body = serde_json::json!({ "query": text });
            let (status, bytes) = self.post_graphql(&body).await?;
            let (data, errors) = self.decode_graphql(status, &bytes)?;
            if errors.is_empty() {
                let data = data.ok_or(Error::Api {
                    status,
                    errors: Vec::new(),
                })?;
                let mut traffic = parse(&data, &parts, chunks.len(), query.bucket);
                unavailable.sort();
                unavailable.dedup();
                traffic.unavailable = unavailable;
                return Ok(traffic);
            }
            let refused: Option<Vec<Part>> = errors.iter().map(part_of).collect();
            match refused {
                Some(refused) if refused.iter().all(|p| parts.contains(p)) => {
                    tracing::debug!(
                        ?refused,
                        "analytics parts not available; asking without them"
                    );
                    parts.retain(|p| !refused.contains(p));
                    unavailable.extend(refused);
                }
                _ => return Err(to_error(&errors)),
            }
        }
        Err(Error::Api {
            status: 400,
            errors: Vec::new(),
        })
    }
}

fn zone_nodes(data: &Value) -> impl Iterator<Item = &Value> {
    data.pointer("/viewer/zones")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// Splits the range into at most [`MAX_CHUNKS`] pieces no wider than the plan allows,
/// newest first; older pieces that don't fit are left out.
fn chunks(query: &TrafficQuery) -> Vec<(u64, u64)> {
    let width = query
        .max_duration
        .filter(|w| *w > 0)
        .unwrap_or(u64::MAX)
        .max(query.bucket.seconds());
    let mut out = Vec::new();
    let mut end = query.end;
    while end > query.start && out.len() < MAX_CHUNKS {
        let start = end.saturating_sub(width).max(query.start);
        out.push((start, end));
        end = start;
    }
    out.reverse();
    out
}

fn build(query: &TrafficQuery, parts: &[Part], chunks: &[(u64, u64)]) -> String {
    let hosts = u32::try_from(query.hosts.len().max(1)).unwrap_or(u32::MAX);
    let page = query.page_size.max(1);
    let buckets = chunks
        .iter()
        .map(|(s, e)| (e - s).div_ceil(query.bucket.seconds()))
        .max()
        .unwrap_or(1);
    let series_limit = u32::try_from(buckets)
        .unwrap_or(u32::MAX)
        .saturating_mul(hosts)
        .min(page);
    let top_limit = query.top.saturating_mul(hosts).min(page);
    let dim = query.bucket.dimension();
    let host = "clientRequestHTTPHost";
    let mut selections = Vec::new();
    for (i, (start, end)) in chunks.iter().enumerate() {
        let mut filter = format!(
            "datetime_geq:{},datetime_lt:{},{host}_in:{}",
            literal(&rfc3339(*start)),
            literal(&rfc3339(*end)),
            list(&query.hosts)
        );
        if let Some(prefix) = &query.path_prefix {
            let escaped = prefix.replace('%', "\\%").replace('_', "\\_");
            filter.push_str(&format!(
                ",clientRequestPath_like:{}",
                literal(&format!("{escaped}%"))
            ));
        }
        selections.push(format!(
            "c{i}_series:{HTTP_DATASET}(filter:{{{filter}}},limit:{series_limit},orderBy:[{dim}_ASC]){{count sum{{edgeResponseBytes}} dimensions{{{dim} {host}}}}}"
        ));
        for part in parts {
            match part {
                Part::Errors => {
                    for (alias, range) in [
                        ("e4", "edgeResponseStatus_geq:400,edgeResponseStatus_lt:500"),
                        ("e5", "edgeResponseStatus_geq:500"),
                    ] {
                        selections.push(format!(
                            "c{i}_{alias}:{HTTP_DATASET}(filter:{{{filter},{range}}},limit:{series_limit},orderBy:[{dim}_ASC]){{count dimensions{{{dim} {host}}}}}"
                        ));
                    }
                }
                Part::Latency => selections.push(format!(
                    "c{i}_latency:{HTTP_DATASET}(filter:{{{filter}}},limit:{hosts}){{count quantiles{{originResponseDurationMsP50 originResponseDurationMsP95 originResponseDurationMsP99 edgeTimeToFirstByteMsP50 edgeTimeToFirstByteMsP95 edgeTimeToFirstByteMsP99}} dimensions{{{host}}}}}"
                )),
                breakdown => {
                    if let Some(key) = breakdown.dimension() {
                        selections.push(format!(
                            "c{i}_{}:{HTTP_DATASET}(filter:{{{filter}}},limit:{top_limit},orderBy:[count_DESC]){{count dimensions{{{host} {key}}}}}",
                            breakdown.alias()
                        ));
                    }
                }
            }
        }
    }
    format!(
        "{{viewer{{zones(filter:{{zoneTag_in:{}}}){{zoneTag {}}}}}}}",
        list(&query.zones),
        selections.join(" ")
    )
}

fn count(row: &Value) -> u64 {
    row.get("count").and_then(Value::as_u64).unwrap_or(0)
}

fn dimension<'a>(row: &'a Value, name: &str) -> Option<&'a Value> {
    row.get("dimensions").and_then(|d| d.get(name))
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn rows<'a>(zone: &'a Value, alias: &str) -> impl Iterator<Item = &'a Value> {
    zone.get(alias)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// Requests, weighted sums of origin and TTFB percentiles, and the requests each covers.
type LatencySums = (u64, [f64; 3], [f64; 3], [u64; 2]);

fn parse(data: &Value, parts: &[Part], chunks: usize, bucket: Bucket) -> Traffic {
    let dim = bucket.dimension();
    let host = "clientRequestHTTPHost";
    let mut series: BTreeMap<(String, u64), SeriesRow> = BTreeMap::new();
    let mut breakdowns: BTreeMap<Part, BTreeMap<(String, String), u64>> = BTreeMap::new();
    let mut latency: BTreeMap<String, LatencySums> = BTreeMap::new();
    let key_of = |row: &Value| -> Option<(String, u64)> {
        let name = dimension(row, host).map(text)?;
        let at = dimension(row, dim)
            .and_then(Value::as_str)
            .and_then(parse_time)?;
        Some((name, at))
    };
    for zone in zone_nodes(data) {
        for i in 0..chunks {
            for row in rows(zone, &format!("c{i}_series")) {
                let Some((name, at)) = key_of(row) else {
                    continue;
                };
                let entry = series.entry((name.clone(), at)).or_insert(SeriesRow {
                    host: name,
                    at,
                    requests: 0,
                    bytes: 0,
                    client_errors: 0,
                    server_errors: 0,
                });
                entry.requests += count(row);
                entry.bytes += row
                    .pointer("/sum/edgeResponseBytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(0);
            }
            for (alias, server) in [("e4", false), ("e5", true)] {
                for row in rows(zone, &format!("c{i}_{alias}")) {
                    let Some(key) = key_of(row) else { continue };
                    if let Some(entry) = series.get_mut(&key) {
                        if server {
                            entry.server_errors += count(row);
                        } else {
                            entry.client_errors += count(row);
                        }
                    }
                }
            }
            for part in parts {
                if let Some(key) = part.dimension() {
                    let sums = breakdowns.entry(*part).or_default();
                    for row in rows(zone, &format!("c{i}_{}", part.alias())) {
                        let Some(name) = dimension(row, host).map(text) else {
                            continue;
                        };
                        let value = dimension(row, key).map(text).unwrap_or_default();
                        *sums.entry((name, value)).or_default() += count(row);
                    }
                }
            }
            for row in rows(zone, &format!("c{i}_latency")) {
                let Some(name) = dimension(row, host).map(text) else {
                    continue;
                };
                let n = count(row);
                let get = |field: &str| {
                    row.pointer(&format!("/quantiles/{field}"))
                        .and_then(Value::as_f64)
                };
                let entry = latency
                    .entry(name)
                    .or_insert((0, [0.0; 3], [0.0; 3], [0; 2]));
                entry.0 += n;
                #[allow(clippy::cast_precision_loss)]
                let weight = n as f64;
                for (j, p) in ["P50", "P95", "P99"].iter().enumerate() {
                    if let Some(v) = get(&format!("originResponseDurationMs{p}")) {
                        entry.1[j] += v * weight;
                        if j == 0 {
                            entry.3[0] += n;
                        }
                    }
                    if let Some(v) = get(&format!("edgeTimeToFirstByteMs{p}")) {
                        entry.2[j] += v * weight;
                        if j == 0 {
                            entry.3[1] += n;
                        }
                    }
                }
            }
        }
    }
    // Percentiles of several chunks are combined weighted by requests: an estimate, but
    // the only one possible without the raw distribution.
    #[allow(clippy::cast_precision_loss)]
    let average = |sums: [f64; 3], n: u64| -> [Option<f64>; 3] {
        sums.map(|s| (n > 0).then(|| (s / n as f64 * 10.0).round() / 10.0))
    };
    Traffic {
        series: series.into_values().collect(),
        breakdowns: breakdowns
            .into_iter()
            .map(|(part, sums)| {
                let mut list: Vec<BreakdownRow> = sums
                    .into_iter()
                    .map(|((host, key), requests)| BreakdownRow {
                        host,
                        key,
                        requests,
                    })
                    .collect();
                list.sort_by(|a, b| b.requests.cmp(&a.requests).then_with(|| a.key.cmp(&b.key)));
                (part, list)
            })
            .collect(),
        latency: latency
            .into_iter()
            .map(|(host, (requests, origin, ttfb, weights))| LatencyRow {
                host,
                requests,
                origin_ms: average(origin, weights[0]),
                ttfb_ms: average(ttfb, weights[1]),
            })
            .collect(),
        unavailable: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use wiremock::{
        Mock, MockServer, Request, ResponseTemplate,
        matchers::{header, method, path},
    };

    use super::*;
    use crate::ApiToken;

    async fn client() -> (MockServer, Client) {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("t"))
            .unwrap()
            .with_backoff(Duration::from_millis(5));
        (server, client)
    }

    fn query() -> TrafficQuery {
        TrafficQuery {
            zones: vec!["z1".into()],
            hosts: vec!["app.xyz.com".into(), "api.xyz.com".into()],
            path_prefix: None,
            start: 1_790_000_000 - 3_600,
            end: 1_790_000_000,
            bucket: Bucket::Minute,
            parts: vec![Part::Errors, Part::Paths, Part::Latency],
            top: 5,
            page_size: 10_000,
            max_duration: None,
        }
    }

    fn body(request: &Request) -> String {
        let value: Value = serde_json::from_slice(&request.body).unwrap();
        value["query"].as_str().unwrap().to_owned()
    }

    #[test]
    fn formats_and_parses_times() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(1_790_000_000), "2026-09-21T14:13:20Z");
        assert_eq!(parse_time("2026-09-21T14:13:20Z"), Some(1_790_000_000));
        assert_eq!(
            parse_time("2026-09-21"),
            Some(1_790_000_000 - 14 * 3_600 - 13 * 60 - 20)
        );
        assert_eq!(parse_time("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_time("nope"), None);
        for secs in [0, 951_782_400, 1_709_164_800, 4_102_444_800] {
            assert_eq!(parse_time(&rfc3339(secs)), Some(secs));
        }
    }

    #[test]
    fn splits_wide_ranges_newest_first_within_the_cap() {
        let mut q = query();
        q.start = 0;
        q.end = 10 * 86_400;
        q.max_duration = Some(86_400);
        let c = chunks(&q);
        assert_eq!(c.len(), MAX_CHUNKS);
        assert_eq!(c.last(), Some(&(9 * 86_400, 10 * 86_400)));
        assert_eq!(c.first(), Some(&(2 * 86_400, 3 * 86_400)));
        q.max_duration = None;
        assert_eq!(chunks(&q), [(0, 10 * 86_400)]);
    }

    #[test]
    fn builds_one_query_for_every_host_and_part() {
        let q = query();
        let text = build(&q, &q.parts, &chunks(&q));
        assert!(text.starts_with("{viewer{zones(filter:{zoneTag_in:[\"z1\"]}){zoneTag "));
        assert!(text.contains("clientRequestHTTPHost_in:[\"app.xyz.com\",\"api.xyz.com\"]"));
        assert!(text.contains("c0_series:httpRequestsAdaptiveGroups("));
        assert!(text.contains("limit:120,orderBy:[datetimeMinute_ASC]"));
        assert!(text.contains("c0_e5:httpRequestsAdaptiveGroups(filter:{"));
        assert!(text.contains("edgeResponseStatus_geq:500"));
        assert!(text.contains("c0_paths:httpRequestsAdaptiveGroups("));
        assert!(text.contains("limit:10,orderBy:[count_DESC]"));
        assert!(text.contains("originResponseDurationMsP95"));
        assert!(text.contains("datetime_geq:\"2026-09-21T13:13:20Z\""));
        let mut q = q;
        q.path_prefix = Some("/api_v1".into());
        let text = build(&q, &[], &chunks(&q));
        assert!(
            text.contains(r#"clientRequestPath_like:"/api\\_v1%""#),
            "{text}"
        );
    }

    #[test]
    fn finds_the_part_an_error_is_about() {
        let err = |message: &str, path: Option<Value>| GraphQlError {
            message: message.into(),
            path: path.map(|p| p.as_array().unwrap().clone()),
        };
        assert_eq!(
            part_of(&err(
                "x",
                Some(serde_json::json!(["viewer", "zones", 0, "c1_latency"]))
            )),
            Some(Part::Latency)
        );
        assert_eq!(
            part_of(&err(
                "zone 'z1' does not have access to the path viewer.zones.0.c0_e4.dimensions",
                None
            )),
            Some(Part::Errors)
        );
        assert_eq!(part_of(&err("rate limiter budget depleted", None)), None);
        assert_eq!(
            part_of(&err(
                "x",
                Some(serde_json::json!(["viewer", "zones", 0, "c0_series"]))
            )),
            None
        );
    }

    #[tokio::test]
    async fn reads_series_breakdowns_and_latency() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/graphql"))
            .and(header("authorization", "Bearer t"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"viewer": {"zones": [{
                    "zoneTag": "z1",
                    "c0_series": [
                        {"count": 10, "sum": {"edgeResponseBytes": 2048}, "dimensions": {"datetimeMinute": "2026-09-21T14:00:00Z", "clientRequestHTTPHost": "app.xyz.com"}},
                        {"count": 4, "sum": {"edgeResponseBytes": 100}, "dimensions": {"datetimeMinute": "2026-09-21T14:01:00Z", "clientRequestHTTPHost": "app.xyz.com"}}
                    ],
                    "c0_e4": [{"count": 2, "dimensions": {"datetimeMinute": "2026-09-21T14:00:00Z", "clientRequestHTTPHost": "app.xyz.com"}}],
                    "c0_e5": [{"count": 1, "dimensions": {"datetimeMinute": "2026-09-21T14:01:00Z", "clientRequestHTTPHost": "app.xyz.com"}}],
                    "c0_paths": [
                        {"count": 9, "dimensions": {"clientRequestHTTPHost": "app.xyz.com", "clientRequestPath": "/"}},
                        {"count": 5, "dimensions": {"clientRequestHTTPHost": "app.xyz.com", "clientRequestPath": "/api"}}
                    ],
                    "c0_latency": [{"count": 14, "quantiles": {"originResponseDurationMsP50": 12.0, "originResponseDurationMsP95": 80.5, "originResponseDurationMsP99": 120.0, "edgeTimeToFirstByteMsP50": 20.0, "edgeTimeToFirstByteMsP95": 90.0, "edgeTimeToFirstByteMsP99": 130.0}, "dimensions": {"clientRequestHTTPHost": "app.xyz.com"}}]
                }]}},
                "errors": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        let traffic = client.http_traffic(&query()).await.unwrap();
        assert_eq!(traffic.series.len(), 2);
        assert_eq!(
            traffic.series[0],
            SeriesRow {
                host: "app.xyz.com".into(),
                at: 1_789_999_200,
                requests: 10,
                bytes: 2048,
                client_errors: 2,
                server_errors: 0
            }
        );
        assert_eq!(traffic.series[1].server_errors, 1);
        let paths = &traffic.breakdowns[&Part::Paths];
        assert_eq!((paths[0].key.as_str(), paths[0].requests), ("/", 9));
        assert_eq!(
            traffic.latency[0].origin_ms,
            [Some(12.0), Some(80.5), Some(120.0)]
        );
        assert!(traffic.unavailable.is_empty());
    }

    #[tokio::test]
    async fn drops_parts_the_plan_refuses_and_says_so() {
        let (server, client) = client().await;
        // First answer: latency isn't on this plan. Second: fine without it.
        Mock::given(path("/graphql"))
            .respond_with(|request: &Request| {
                if body(request).contains("c0_latency") {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "data": null,
                        "errors": [{"message": "zone 'z1' does not have access to the path", "path": ["viewer", "zones", 0, "c0_latency"], "extensions": {"timestamp": "2026-09-21T14:00:00Z"}}]
                    }))
                } else {
                    ResponseTemplate::new(200).set_body_json(serde_json::json!({
                        "data": {"viewer": {"zones": [{"zoneTag": "z1", "c0_series": []}]}},
                        "errors": null
                    }))
                }
            })
            .expect(2)
            .mount(&server)
            .await;
        let traffic = client.http_traffic(&query()).await.unwrap();
        assert_eq!(traffic.unavailable, [Part::Latency]);
        assert!(traffic.series.is_empty());
    }

    #[tokio::test]
    async fn classifies_errors_it_cannot_work_around() {
        let (server, client) = client().await;
        let error = |message: &str| {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": null, "errors": [{"message": message, "path": null}]
            }))
        };
        Mock::given(path("/graphql"))
            .respond_with(error("zones ['z1'] are not authorized"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let err = client.http_traffic(&query()).await.unwrap_err();
        assert!(err.is_auth(), "{err:?}");
        assert!(err.detail().contains("not authorized"));

        Mock::given(path("/graphql"))
            .respond_with(error(
                "rate limiter budget depleted, try again after 5 minutes",
            ))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        assert!(
            client
                .http_traffic(&query())
                .await
                .unwrap_err()
                .is_rate_limited()
        );

        Mock::given(path("/graphql"))
            .respond_with(error("query time range is too large"))
            .mount(&server)
            .await;
        let err = client.http_traffic(&query()).await.unwrap_err();
        assert_eq!(err.status(), Some(400));
        assert!(!err.is_auth());
    }

    #[tokio::test]
    async fn rest_style_auth_failures_are_auth_errors() {
        let (server, client) = client().await;
        Mock::given(path("/graphql"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "success": false, "errors": [{"code": 10000, "message": "Authentication error"}], "messages": [], "result": null
            })))
            .mount(&server)
            .await;
        let err = client.analytics_limits(&["z1".into()]).await.unwrap_err();
        assert!(err.is_auth());
        assert_eq!(client.probe_analytics("z1").await, crate::Access::Denied);
    }

    #[tokio::test]
    async fn reads_plan_limits_and_probes() {
        let (server, client) = client().await;
        Mock::given(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"viewer": {"zones": [{"zoneTag": "z1", "settings": {"httpRequestsAdaptiveGroups": {
                    "enabled": true, "maxDuration": 86400, "notOlderThan": 691200, "maxPageSize": 10000,
                    "availableFields": ["count", "dimensions.clientRequestPath"]
                }}}]}}
            })))
            .mount(&server)
            .await;
        let limits = client.analytics_limits(&["z1".into()]).await.unwrap();
        assert_eq!(limits[0].0, "z1");
        assert_eq!(limits[0].1.max_duration, 86_400);
        assert_eq!(limits[0].1.not_older_than, 691_200);
        assert_eq!(client.probe_analytics("z1").await, crate::Access::Allowed);
    }

    #[tokio::test]
    async fn never_exceeds_the_query_budget() {
        let (server, client) = client().await;
        let client = client.with_graphql_rate(1, Duration::from_secs(60));
        Mock::given(path("/graphql"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {"viewer": {"zones": []}}
            })))
            .expect(1)
            .mount(&server)
            .await;
        client.analytics_limits(&["z1".into()]).await.unwrap();
        let err = client.analytics_limits(&["z1".into()]).await.unwrap_err();
        assert!(err.is_rate_limited());
    }
}
