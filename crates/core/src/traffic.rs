//! Traffic history for this Mac's tunnel connectors: cloudflared's metrics, sampled every
//! 10 s into a one-hour ring buffer per tunnel. Counters become per-interval deltas, so a
//! connector restart (counters back to zero) never shows as negative traffic.

use std::{
    collections::{HashMap, VecDeque},
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cloudflared::MetricsSnapshot;
use serde::Serialize;

/// How often connectors are sampled.
pub const INTERVAL: Duration = Duration::from_secs(10);
/// One hour of samples.
const CAPACITY: usize = 360;

/// One sample interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TrafficPoint {
    /// Milliseconds since the epoch.
    pub at: f64,
    /// Requests during the interval.
    pub requests: u32,
    /// Failed requests during the interval.
    pub errors: u32,
}

/// Traffic of one tunnel's connector on this Mac.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Traffic {
    /// The last hour, oldest first.
    pub points: Vec<TrafficPoint>,
    /// Requests since the connector started.
    pub total_requests: u32,
    /// Failed requests since the connector started.
    pub total_errors: u32,
    /// Edge connections now.
    pub connections: u32,
    /// Round-trip time to the edge, in milliseconds.
    pub rtt_ms: Option<f64>,
    /// Edge locations, e.g. `AMS`.
    pub locations: Vec<String>,
}

#[derive(Debug, Default)]
struct Series {
    points: VecDeque<TrafficPoint>,
    last: Option<(u64, u64)>,
    latest: Option<MetricsSnapshot>,
}

/// Traffic histories by tunnel id. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct TrafficLog {
    series: Arc<Mutex<HashMap<String, Series>>>,
}

fn clamp(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn now_ms() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0)
}

impl TrafficLog {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Series>> {
        self.series.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Records a metrics scrape for `tunnel` taken at `at` (ms).
    pub fn record(&self, tunnel: &str, at: f64, metrics: MetricsSnapshot) {
        let (requests, errors) = (metrics.total_requests(), metrics.request_errors());
        let mut all = self.lock();
        let series = all.entry(tunnel.to_owned()).or_default();
        let point = match series.last {
            // Counters only go down when the connector restarted: count from zero.
            Some((r, e)) if requests >= r && errors >= e => TrafficPoint {
                at,
                requests: clamp(requests - r),
                errors: clamp(errors - e),
            },
            Some(_) => TrafficPoint {
                at,
                requests: clamp(requests),
                errors: clamp(errors),
            },
            None => TrafficPoint {
                at,
                requests: 0,
                errors: 0,
            },
        };
        series.last = Some((requests, errors));
        series.latest = Some(metrics);
        if series.points.len() == CAPACITY {
            series.points.pop_front();
        }
        series.points.push_back(point);
    }

    /// Records a scrape taken now.
    pub fn record_now(&self, tunnel: &str, metrics: MetricsSnapshot) {
        self.record(tunnel, now_ms(), metrics);
    }

    /// Forgets a tunnel (its connector stopped).
    pub fn forget(&self, tunnel: &str) {
        self.lock().remove(tunnel);
    }

    /// The history and latest numbers for `tunnel`.
    pub fn get(&self, tunnel: &str) -> Option<Traffic> {
        let all = self.lock();
        let series = all.get(tunnel)?;
        let latest = series.latest.as_ref()?;
        Some(Traffic {
            points: series.points.iter().copied().collect(),
            total_requests: clamp(latest.total_requests()),
            total_errors: clamp(latest.request_errors()),
            connections: clamp(latest.ha_connections()),
            rtt_ms: latest.rtt_ms(),
            locations: latest.edge_locations(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics(requests: u64, errors: u64) -> MetricsSnapshot {
        MetricsSnapshot::parse(&format!(
            "cloudflared_tunnel_total_requests {requests}\ncloudflared_tunnel_request_errors {errors}\ncloudflared_tunnel_ha_connections 4\n"
        ))
    }

    #[test]
    fn keeps_deltas_through_restarts_and_caps_history() {
        let log = TrafficLog::default();
        log.record("t", 0.0, metrics(10, 1));
        log.record("t", 10.0, metrics(25, 1));
        log.record("t", 20.0, metrics(3, 0)); // connector restarted
        let traffic = log.get("t").unwrap();
        let requests: Vec<u32> = traffic.points.iter().map(|p| p.requests).collect();
        assert_eq!(requests, [0, 15, 3]);
        assert_eq!(traffic.total_requests, 3);
        assert_eq!(traffic.connections, 4);

        for i in 0..400u32 {
            log.record("t", f64::from(i), metrics(u64::from(i) * 2, 0));
        }
        assert_eq!(log.get("t").unwrap().points.len(), CAPACITY);
        log.forget("t");
        assert!(log.get("t").is_none());
    }
}
