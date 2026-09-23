//! Traffic history for this Mac's tunnel connectors, from cloudflared's `/metrics`.
//!
//! Connectors are sampled every [`IDLE_INTERVAL`], or every [`LIVE_INTERVAL`] while
//! someone is looking ([`TrafficLog::watch`], a short lease renewed by each read). The
//! newest [`CAPACITY`] samples stay in memory per tunnel. Counters become per-interval
//! deltas, so a connector restart (counters back to zero) never shows as negative
//! traffic. Each finished minute is summed into a [`MinuteRollup`] for the persistent
//! 7-day history (`engine::local`).
//!
//! Series cross IPC column by column ([`TrafficSeries`]), the shape the charts draw.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use cloudflared::MetricsSnapshot;
use serde::{Deserialize, Serialize};

/// How often connectors are sampled while nobody watches.
pub const IDLE_INTERVAL: Duration = Duration::from_secs(10);
/// How often a watched connector is sampled.
pub const LIVE_INTERVAL: Duration = Duration::from_secs(1);
/// How long one read keeps a connector on [`LIVE_INTERVAL`].
const WATCH_LEASE: Duration = Duration::from_secs(5);
/// Samples kept in memory per tunnel (an hour at the live rate).
pub const CAPACITY: usize = 3_600;
/// How long minute rollups are kept.
pub const RETENTION: Duration = Duration::from_secs(7 * 24 * 3_600);

/// One sample interval.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    at: f64,
    span: f64,
    requests: u32,
    errors: u32,
    classes: [u32; 4],
    concurrent: u32,
    connections: u32,
    rtt_ms: Option<f64>,
}

/// Samples as columns: entry `i` of every column belongs to the same interval.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TrafficSeries {
    /// End of each interval, in milliseconds since the epoch; ascending.
    // Always finite: typed as plain numbers, not `number | null`.
    #[cfg_attr(feature = "specta", specta(type = Vec<u32>))]
    pub at: Vec<f64>,
    /// Seconds each interval covers (rates are counts divided by this).
    // Always finite.
    #[cfg_attr(feature = "specta", specta(type = Vec<u32>))]
    pub span: Vec<f64>,
    /// Requests during the interval.
    pub requests: Vec<u32>,
    /// Failed requests (the origin couldn't be reached) during the interval.
    pub errors: Vec<u32>,
    /// 1xx and 2xx responses.
    pub ok: Vec<u32>,
    /// 3xx responses.
    pub redirects: Vec<u32>,
    /// 4xx responses.
    pub client_errors: Vec<u32>,
    /// 5xx responses.
    pub server_errors: Vec<u32>,
    /// Most requests in flight at once (at the sample; the peak for rollups).
    pub concurrent: Vec<u32>,
    /// Edge connections (at the sample; the lowest for rollups).
    pub connections: Vec<u32>,
    /// Smoothed round trip to the edge, in milliseconds (the mean for rollups).
    pub rtt_ms: Vec<Option<f64>>,
}

impl TrafficSeries {
    fn push(&mut self, p: &Point) {
        self.at.push(p.at);
        self.span.push(p.span);
        self.requests.push(p.requests);
        self.errors.push(p.errors);
        let [ok, redirects, client, server] = p.classes;
        self.ok.push(ok);
        self.redirects.push(redirects);
        self.client_errors.push(client);
        self.server_errors.push(server);
        self.concurrent.push(p.concurrent);
        self.connections.push(p.connections);
        self.rtt_ms.push(p.rtt_ms);
    }

    /// Number of intervals.
    pub fn len(&self) -> usize {
        self.at.len()
    }

    /// Whether there are no intervals.
    pub fn is_empty(&self) -> bool {
        self.at.is_empty()
    }
}

/// Live traffic of one tunnel's connector on this Mac.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Traffic {
    /// Samples newer than the read's `since` (all of them without it), oldest first.
    pub series: TrafficSeries,
    /// Requests since the connector started.
    pub total_requests: u32,
    /// Failed requests since the connector started.
    pub total_errors: u32,
    /// Edge connections now.
    pub connections: u32,
    /// Round-trip time to the edge now, in milliseconds.
    pub rtt_ms: Option<f64>,
    /// Edge locations, e.g. `AMS`.
    pub locations: Vec<String>,
}

/// A span of persisted history.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum HistoryRange {
    /// The last 24 hours, in 5-minute buckets.
    Day,
    /// The last 7 days, in 30-minute buckets.
    Week,
}

impl HistoryRange {
    /// Minutes per bucket.
    pub const fn bucket_minutes(self) -> i64 {
        match self {
            Self::Day => 5,
            Self::Week => 30,
        }
    }

    /// Minutes covered.
    pub const fn minutes(self) -> i64 {
        match self {
            Self::Day => 24 * 60,
            Self::Week => 7 * 24 * 60,
        }
    }
}

/// One minute of a tunnel's traffic, as persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct MinuteRollup {
    /// Tunnel id.
    pub tunnel: String,
    /// Minutes since the epoch.
    pub minute: i64,
    /// Requests.
    pub requests: u64,
    /// Failed requests.
    pub errors: u64,
    /// Responses by class: 2xx, 3xx, 4xx, 5xx.
    pub classes: [u64; 4],
    /// Peak requests in flight.
    pub concurrent_max: u32,
    /// Fewest edge connections seen.
    pub connections_min: u32,
    /// Sum of the RTT samples, for the mean.
    pub rtt_sum_ms: f64,
    /// Number of RTT samples.
    pub rtt_samples: u32,
}

impl MinuteRollup {
    fn new(tunnel: &str, minute: i64) -> Self {
        Self {
            tunnel: tunnel.to_owned(),
            minute,
            requests: 0,
            errors: 0,
            classes: [0; 4],
            concurrent_max: 0,
            connections_min: u32::MAX,
            rtt_sum_ms: 0.0,
            rtt_samples: 0,
        }
    }

    fn add(&mut self, p: &Point) {
        self.requests += u64::from(p.requests);
        self.errors += u64::from(p.errors);
        for (sum, n) in self.classes.iter_mut().zip(p.classes) {
            *sum += u64::from(n);
        }
        self.concurrent_max = self.concurrent_max.max(p.concurrent);
        self.connections_min = self.connections_min.min(p.connections);
        if let Some(rtt) = p.rtt_ms {
            self.rtt_sum_ms += rtt;
            self.rtt_samples += 1;
        }
    }
}

/// Counter values of the previous scrape.
#[derive(Debug, Clone, Copy)]
struct Counters {
    requests: u64,
    errors: u64,
    classes: [u64; 4],
}

impl Counters {
    fn of(metrics: &MetricsSnapshot) -> Self {
        Self {
            requests: metrics.total_requests(),
            errors: metrics.request_errors(),
            classes: metrics.responses_by_class(),
        }
    }

    /// `self - before`, or `self` when any counter went down (the connector restarted).
    fn since(self, before: Self) -> Self {
        let restarted = self.requests < before.requests
            || self.errors < before.errors
            || self.classes.iter().zip(before.classes).any(|(n, b)| *n < b);
        if restarted {
            return self;
        }
        Self {
            requests: self.requests - before.requests,
            errors: self.errors - before.errors,
            classes: std::array::from_fn(|i| self.classes[i] - before.classes[i]),
        }
    }
}

#[derive(Debug, Default)]
struct Series {
    points: VecDeque<Point>,
    last: Option<(f64, Counters)>,
    latest: Option<MetricsSnapshot>,
    minute: Option<MinuteRollup>,
    last_sampled: Option<Instant>,
    watched_until: Option<Instant>,
}

/// Traffic histories by tunnel id. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct TrafficLog {
    series: Arc<Mutex<HashMap<String, Series>>>,
}

fn clamp(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Milliseconds since the epoch.
pub fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

#[allow(clippy::cast_possible_truncation)]
fn minute_of(at_ms: f64) -> i64 {
    (at_ms / 60_000.0).floor() as i64
}

impl TrafficLog {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Series>> {
        self.series.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a metrics scrape for `tunnel` taken at `at` (ms). Returns the previous
    /// minute's rollup once a scrape lands in a new minute.
    pub fn record(&self, tunnel: &str, at: f64, metrics: MetricsSnapshot) -> Option<MinuteRollup> {
        let counters = Counters::of(&metrics);
        let mut all = self.lock();
        let series = all.entry(tunnel.to_owned()).or_default();
        let previous = series.last.replace((at, counters));
        let point = previous.map(|(before_at, before)| {
            let delta = counters.since(before);
            Point {
                at,
                span: ((at - before_at) / 1000.0).max(0.0),
                requests: clamp(delta.requests),
                errors: clamp(delta.errors),
                classes: delta.classes.map(clamp),
                concurrent: clamp(metrics.concurrent_requests()),
                connections: clamp(metrics.ha_connections()),
                rtt_ms: metrics
                    .smoothed_rtt_ms()
                    .map(|ms| (ms * 10.0).round() / 10.0),
            }
        });
        series.latest = Some(metrics);
        let point = point?;

        let minute = minute_of(at);
        let finished = series.minute.take_if(|rollup| rollup.minute != minute);
        series
            .minute
            .get_or_insert_with(|| MinuteRollup::new(tunnel, minute))
            .add(&point);
        if series.points.len() == CAPACITY {
            series.points.pop_front();
        }
        series.points.push_back(point);
        finished
    }

    /// Records a scrape taken now.
    pub fn record_now(&self, tunnel: &str, metrics: MetricsSnapshot) -> Option<MinuteRollup> {
        self.record(tunnel, now_ms(), metrics)
    }

    /// Keeps `tunnel` on the live sampling rate for a few seconds.
    pub fn watch(&self, tunnel: &str) {
        self.lock()
            .entry(tunnel.to_owned())
            .or_default()
            .watched_until = Some(Instant::now() + WATCH_LEASE);
    }

    /// Whether `tunnel` should be sampled at `now`: it's watched, or the idle interval
    /// has (nearly) passed. Marks it sampled when it is.
    pub fn take_due(&self, tunnel: &str, now: Instant) -> bool {
        let mut all = self.lock();
        let series = all.entry(tunnel.to_owned()).or_default();
        let watched = series.watched_until.is_some_and(|until| until > now);
        // A little slack so a 1 s tick never stretches the idle interval to 11 s.
        let idle_due = series.last_sampled.is_none_or(|last| {
            now.saturating_duration_since(last) + LIVE_INTERVAL / 2 >= IDLE_INTERVAL
        });
        let due = watched || idle_due;
        if due {
            series.last_sampled = Some(now);
        }
        due
    }

    /// Forgets a tunnel (its connector stopped), returning its unfinished minute.
    pub fn forget(&self, tunnel: &str) -> Option<MinuteRollup> {
        self.lock().remove(tunnel).and_then(|series| series.minute)
    }

    /// The samples after `since` (all without it) and the latest numbers for `tunnel`.
    pub fn get(&self, tunnel: &str, since: Option<f64>) -> Option<Traffic> {
        let all = self.lock();
        let series = all.get(tunnel)?;
        let latest = series.latest.as_ref()?;
        let mut columns = TrafficSeries::default();
        let start = since.map_or(0, |since| series.points.partition_point(|p| p.at <= since));
        for point in series.points.range(start..) {
            columns.push(point);
        }
        Some(Traffic {
            series: columns,
            total_requests: clamp(latest.total_requests()),
            total_errors: clamp(latest.request_errors()),
            connections: clamp(latest.ha_connections()),
            rtt_ms: latest.smoothed_rtt_ms(),
            locations: latest.edge_locations(),
        })
    }
}

/// Buckets persisted rollups (ascending by minute) into a series for `range`.
pub fn bucket(rollups: &[MinuteRollup], range: HistoryRange) -> TrafficSeries {
    let size = range.bucket_minutes();
    let mut series = TrafficSeries::default();
    let mut rest = rollups;
    while let Some(first) = rest.first() {
        let start = first.minute.div_euclid(size) * size;
        let end = rest
            .iter()
            .position(|r| r.minute >= start + size)
            .unwrap_or(rest.len());
        let (group, tail) = rest.split_at(end);
        rest = tail;
        let mut sum = MinuteRollup::new(&first.tunnel, start);
        for r in group {
            sum.requests += r.requests;
            sum.errors += r.errors;
            for (total, n) in sum.classes.iter_mut().zip(r.classes) {
                *total += n;
            }
            sum.concurrent_max = sum.concurrent_max.max(r.concurrent_max);
            sum.connections_min = sum.connections_min.min(r.connections_min);
            sum.rtt_sum_ms += r.rtt_sum_ms;
            sum.rtt_samples += r.rtt_samples;
        }
        #[allow(clippy::cast_precision_loss)]
        series.push(&Point {
            at: ((start + size) * 60_000) as f64,
            // Rates are over the minutes the connector ran, not the whole bucket.
            span: group.len() as f64 * 60.0,
            requests: clamp(sum.requests),
            errors: clamp(sum.errors),
            classes: sum.classes.map(clamp),
            concurrent: sum.concurrent_max,
            connections: if sum.connections_min == u32::MAX {
                0
            } else {
                sum.connections_min
            },
            rtt_ms: (sum.rtt_samples > 0)
                .then(|| (sum.rtt_sum_ms / f64::from(sum.rtt_samples) * 10.0).round() / 10.0),
        });
    }
    series
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(requests: u64, errors: u64) -> MetricsSnapshot {
        MetricsSnapshot::parse(&format!(
            "cloudflared_tunnel_total_requests {requests}\ncloudflared_tunnel_request_errors {errors}\ncloudflared_tunnel_ha_connections 4\ncloudflared_tunnel_response_by_code{{status_code=\"200\"}} {requests}\nquic_client_smoothed_rtt 18.44\n"
        ))
    }

    #[test]
    fn keeps_deltas_through_restarts_and_caps_history() {
        let log = TrafficLog::default();
        assert!(log.record("t", 0.0, metrics(10, 1)).is_none());
        log.record("t", 10_000.0, metrics(25, 1));
        log.record("t", 20_000.0, metrics(3, 0)); // connector restarted
        let traffic = log.get("t", None).unwrap();
        // The first scrape is the baseline, not a point.
        assert_eq!(traffic.series.requests, [15, 3]);
        assert_eq!(traffic.series.ok, [15, 3]);
        assert_eq!(traffic.series.span, [10.0, 10.0]);
        assert_eq!(traffic.series.rtt_ms, [Some(18.4), Some(18.4)]);
        assert_eq!(traffic.total_requests, 3);
        assert_eq!(traffic.connections, 4);

        for i in 0..4_000u32 {
            log.record("t", f64::from(i) * 1000.0, metrics(u64::from(i) * 2, 0));
        }
        assert_eq!(log.get("t", None).unwrap().series.len(), CAPACITY);
        log.forget("t");
        assert!(log.get("t", None).is_none());
    }

    #[test]
    fn reads_only_newer_samples() {
        let log = TrafficLog::default();
        for i in 0..5u32 {
            log.record("t", f64::from(i) * 1000.0, metrics(u64::from(i), 0));
        }
        let newer = log.get("t", Some(2_000.0)).unwrap().series;
        assert_eq!(newer.at, [3_000.0, 4_000.0]);
        assert!(log.get("t", Some(4_000.0)).unwrap().series.is_empty());
    }

    #[test]
    fn rolls_up_finished_minutes() {
        let log = TrafficLog::default();
        log.record("t", 50_000.0, metrics(0, 0));
        assert!(log.record("t", 55_000.0, metrics(4, 0)).is_none());
        assert!(log.record("t", 59_000.0, metrics(10, 1)).is_none());
        let rollup = log.record("t", 61_000.0, metrics(11, 1)).unwrap();
        assert_eq!(rollup.minute, 0);
        assert_eq!(rollup.requests, 10);
        assert_eq!(rollup.errors, 1);
        assert_eq!(rollup.connections_min, 4);
        assert_eq!(rollup.rtt_samples, 2);
        // The unfinished minute comes back when the connector stops.
        let partial = log.forget("t").unwrap();
        assert_eq!((partial.minute, partial.requests), (1, 1));
    }

    #[test]
    fn samples_watched_connectors_live_and_others_idle() {
        let log = TrafficLog::default();
        let start = Instant::now();
        assert!(log.take_due("t", start));
        assert!(!log.take_due("t", start + Duration::from_secs(1)));
        assert!(log.take_due("t", start + Duration::from_millis(9_600)));
        log.watch("t");
        let now = Instant::now();
        assert!(log.take_due("t", now + Duration::from_secs(1)));
        assert!(log.take_due("t", now + Duration::from_secs(2)));
    }

    #[test]
    fn buckets_rollups_by_range() {
        let rollup = |minute, requests, rtt: f64| MinuteRollup {
            requests,
            rtt_sum_ms: rtt,
            rtt_samples: 1,
            connections_min: 4,
            ..MinuteRollup::new("t", minute)
        };
        let series = bucket(
            &[
                rollup(1, 10, 10.0),
                rollup(4, 20, 20.0),
                rollup(5, 5, 30.0),
                rollup(12, 1, 40.0),
            ],
            HistoryRange::Day,
        );
        assert_eq!(series.at, [300_000.0, 600_000.0, 900_000.0]);
        assert_eq!(series.requests, [30, 5, 1]);
        assert_eq!(series.span, [120.0, 60.0, 60.0]);
        assert_eq!(series.rtt_ms, [Some(15.0), Some(30.0), Some(40.0)]);
        assert_eq!(series.connections, [4, 4, 4]);
        assert!(bucket(&[], HistoryRange::Week).is_empty());
    }
}
