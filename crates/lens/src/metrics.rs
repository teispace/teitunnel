//! Per-tap counters and a lock-free latency histogram.
//!
//! Everything is an atomic updated on the proxy's hot path; [`TapMetrics::snapshot`]
//! reads a consistent-enough copy for analytics (counters may be a request apart).

use std::{
    sync::atomic::{AtomicU64, Ordering::Relaxed},
    time::Duration,
};

use serde::Serialize;

/// Sub-buckets per power of two: the histogram's relative error is at most 1/16.
const SUB_BITS: u32 = 4;
const SUB: usize = 1 << SUB_BITS;
/// Powers of two covered above the linear range (microseconds up to ~2^44 µs ≈ 200 days).
const EXPONENTS: usize = 40;
const BUCKETS: usize = SUB + EXPONENTS * SUB;

/// A log-linear histogram of microsecond values.
pub(crate) struct Histogram {
    buckets: Box<[AtomicU64]>,
    count: AtomicU64,
    sum: AtomicU64,
    max: AtomicU64,
}

impl std::fmt::Debug for Histogram {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Histogram")
            .field("count", &self.count.load(Relaxed))
            .finish_non_exhaustive()
    }
}

impl Histogram {
    pub(crate) fn new() -> Self {
        Self {
            buckets: (0..BUCKETS).map(|_| AtomicU64::new(0)).collect(),
            count: AtomicU64::new(0),
            sum: AtomicU64::new(0),
            max: AtomicU64::new(0),
        }
    }

    fn index(value: u64) -> usize {
        if value < SUB as u64 {
            return usize::try_from(value).unwrap_or(0);
        }
        let exp = 63 - value.leading_zeros(); // >= SUB_BITS
        let sub = usize::try_from((value >> (exp - SUB_BITS)) & (SUB as u64 - 1)).unwrap_or(0);
        let group = usize::try_from(exp - SUB_BITS).unwrap_or(0);
        (SUB + group * SUB + sub).min(BUCKETS - 1)
    }

    /// Midpoint of a bucket's range.
    fn value_of(index: usize) -> u64 {
        if index < SUB {
            return index as u64;
        }
        let group = (index - SUB) / SUB;
        let sub = ((index - SUB) % SUB) as u64;
        let exp = u32::try_from(group).unwrap_or(0) + SUB_BITS;
        let width = 1u64 << (exp - SUB_BITS);
        let low = (1u64 << exp) + sub * width;
        low + width / 2
    }

    pub(crate) fn record(&self, micros: u64) {
        if let Some(bucket) = self.buckets.get(Self::index(micros)) {
            bucket.fetch_add(1, Relaxed);
        }
        self.count.fetch_add(1, Relaxed);
        self.sum.fetch_add(micros, Relaxed);
        self.max.fetch_max(micros, Relaxed);
    }

    /// The value at quantile `q` (0–1), in microseconds.
    pub(crate) fn quantile(&self, q: f64) -> Option<u64> {
        let count = self.count.load(Relaxed);
        if count == 0 {
            return None;
        }
        if q >= 1.0 {
            return Some(self.max.load(Relaxed));
        }
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            clippy::cast_precision_loss
        )]
        let rank = ((q.clamp(0.0, 1.0) * count as f64).ceil() as u64).max(1);
        let mut seen = 0u64;
        for (index, bucket) in self.buckets.iter().enumerate() {
            seen += bucket.load(Relaxed);
            if seen >= rank {
                return Some(Self::value_of(index).min(self.max.load(Relaxed)));
            }
        }
        Some(self.max.load(Relaxed))
    }

    fn summary(&self) -> LatencySummary {
        let count = self.count.load(Relaxed);
        #[allow(clippy::cast_precision_loss)]
        let ms = |micros: Option<u64>| micros.map(|us| us as f64 / 1_000.0);
        LatencySummary {
            count,
            p50_ms: ms(self.quantile(0.50)),
            p95_ms: ms(self.quantile(0.95)),
            p99_ms: ms(self.quantile(0.99)),
            max_ms: ms((count > 0).then(|| self.max.load(Relaxed))),
            #[allow(clippy::cast_precision_loss)]
            mean_ms: (count > 0).then(|| self.sum.load(Relaxed) as f64 / count as f64 / 1_000.0),
        }
    }
}

/// Latency percentiles (time to the response head), in milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LatencySummary {
    /// Samples.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub count: u64,
    /// Median.
    pub p50_ms: Option<f64>,
    /// 95th percentile.
    pub p95_ms: Option<f64>,
    /// 99th percentile.
    pub p99_ms: Option<f64>,
    /// Slowest.
    pub max_ms: Option<f64>,
    /// Mean.
    pub mean_ms: Option<f64>,
}

/// Responses by status class.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct StatusCounts {
    /// 1xx (mostly `101 Switching Protocols`).
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub informational: u64,
    /// 2xx.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub success: u64,
    /// 3xx.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub redirect: u64,
    /// 4xx.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub client_error: u64,
    /// 5xx.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub server_error: u64,
}

/// A point-in-time copy of a tap's metrics.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MetricsSnapshot {
    /// Requests received.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub requests: u64,
    /// Responses by status class.
    pub status: StatusCounts,
    /// Exchanges that failed (upstream unreachable, reset, client aborted…).
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub errors: u64,
    /// Requests stopped by a gate.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub blocked: u64,
    /// Requests answered by a stub.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub stubbed: u64,
    /// Request body bytes received from clients.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub bytes_in: u64,
    /// Response body bytes sent to clients.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub bytes_out: u64,
    /// Open client connections on listeners routing to this tap.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub active_connections: u64,
    /// Requests in flight.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub active_requests: u64,
    /// Open WebSocket/upgraded streams.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub active_streams: u64,
    /// Time to the response head.
    pub latency: LatencySummary,
}

/// Live counters of one tap.
#[derive(Debug)]
pub(crate) struct TapMetrics {
    pub(crate) requests: AtomicU64,
    status: [AtomicU64; 5],
    pub(crate) errors: AtomicU64,
    pub(crate) blocked: AtomicU64,
    pub(crate) stubbed: AtomicU64,
    pub(crate) bytes_in: AtomicU64,
    pub(crate) bytes_out: AtomicU64,
    pub(crate) active_connections: AtomicU64,
    pub(crate) active_requests: AtomicU64,
    pub(crate) active_streams: AtomicU64,
    latency: Histogram,
}

impl TapMetrics {
    pub(crate) fn new() -> Self {
        Self {
            requests: AtomicU64::new(0),
            status: Default::default(),
            errors: AtomicU64::new(0),
            blocked: AtomicU64::new(0),
            stubbed: AtomicU64::new(0),
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            active_requests: AtomicU64::new(0),
            active_streams: AtomicU64::new(0),
            latency: Histogram::new(),
        }
    }

    pub(crate) fn record_status(&self, status: u16) {
        if let Some(counter) = usize::from(status / 100)
            .checked_sub(1)
            .and_then(|class| self.status.get(class))
        {
            counter.fetch_add(1, Relaxed);
        }
    }

    pub(crate) fn record_latency(&self, latency: Duration) {
        self.latency
            .record(u64::try_from(latency.as_micros()).unwrap_or(u64::MAX));
    }

    /// Decrements a gauge without wrapping below zero.
    pub(crate) fn dec(gauge: &AtomicU64) {
        let _ = gauge.fetch_update(Relaxed, Relaxed, |v| Some(v.saturating_sub(1)));
    }

    pub(crate) fn snapshot(&self) -> MetricsSnapshot {
        let status = |i: usize| self.status[i].load(Relaxed);
        MetricsSnapshot {
            requests: self.requests.load(Relaxed),
            status: StatusCounts {
                informational: status(0),
                success: status(1),
                redirect: status(2),
                client_error: status(3),
                server_error: status(4),
            },
            errors: self.errors.load(Relaxed),
            blocked: self.blocked.load(Relaxed),
            stubbed: self.stubbed.load(Relaxed),
            bytes_in: self.bytes_in.load(Relaxed),
            bytes_out: self.bytes_out.load(Relaxed),
            active_connections: self.active_connections.load(Relaxed),
            active_requests: self.active_requests.load(Relaxed),
            active_streams: self.active_streams.load(Relaxed),
            latency: self.latency.summary(),
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn percentiles_are_close() {
        let h = Histogram::new();
        for us in 1..=10_000u64 {
            h.record(us);
        }
        let p50 = h.quantile(0.5).unwrap() as f64;
        let p99 = h.quantile(0.99).unwrap() as f64;
        assert!((p50 - 5_000.0).abs() / 5_000.0 < 0.07, "p50 {p50}");
        assert!((p99 - 9_900.0).abs() / 9_900.0 < 0.07, "p99 {p99}");
        assert_eq!(h.quantile(1.0), Some(10_000));
        let summary = h.summary();
        assert_eq!(summary.count, 10_000);
        assert!((summary.mean_ms.unwrap() - 5.0005).abs() < 0.001);
    }

    #[test]
    fn empty_histogram_has_no_percentiles() {
        let h = Histogram::new();
        assert_eq!(h.quantile(0.5), None);
        assert_eq!(h.summary(), LatencySummary::default());
    }

    #[test]
    fn status_classes_and_gauges() {
        let m = TapMetrics::new();
        for status in [101, 200, 204, 301, 404, 503, 700, 0] {
            m.record_status(status);
        }
        m.active_requests.fetch_add(1, Relaxed);
        TapMetrics::dec(&m.active_requests);
        TapMetrics::dec(&m.active_requests);
        let snap = m.snapshot();
        assert_eq!(snap.status.success, 2);
        assert_eq!(snap.status.server_error, 1);
        assert_eq!(snap.status.informational, 1);
        assert_eq!(snap.active_requests, 0);
    }

    proptest! {
        #[test]
        fn bucket_midpoint_is_within_error(value in 0u64..(1 << 44)) {
            let mid = Histogram::value_of(Histogram::index(value));
            let err = (mid as f64 - value as f64).abs();
            prop_assert!(err <= (value as f64 / 16.0).max(1.0), "value {value} mid {mid}");
        }
    }
}
