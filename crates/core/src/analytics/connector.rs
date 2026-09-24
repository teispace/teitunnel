//! This machine's cloudflared metrics as an analytics source: exact counts for everything
//! a tunnel's connector here served, but per tunnel (cloudflared doesn't split its
//! counters by hostname) and only for the minutes it ran (7 days of history).

use futures_util::future::BoxFuture;

use super::{
    AnalyticsError, AnalyticsRange, AnalyticsSource, RouteRef, RouteStats, SourceKind, StatsPart,
    StatsSeries, StatusClasses, now_ms,
};
use crate::{engine::Local, traffic::MinuteRollup};

/// A tunnel's connector on this machine, answering for the routes it carries.
#[derive(Debug, Clone)]
pub struct ConnectorSource {
    local: Local,
    tunnel_id: String,
    /// Hostnames the tunnel serves (lower case).
    hostnames: Vec<String>,
}

impl ConnectorSource {
    /// The connector of `tunnel_id`, serving `hostnames`.
    pub fn new(local: Local, tunnel_id: &str, hostnames: &[String]) -> Self {
        Self {
            local,
            tunnel_id: tunnel_id.to_owned(),
            hostnames: hostnames.iter().map(|h| h.to_ascii_lowercase()).collect(),
        }
    }
}

/// Minute rollups summed into the range's buckets, every bucket present.
pub(crate) fn stats_from_rollups(
    rollups: &[MinuteRollup],
    route: &RouteRef,
    range: AnalyticsRange,
    now_ms: f64,
) -> RouteStats {
    let size = i64::try_from(range.bucket().seconds() / 60)
        .unwrap_or(1)
        .max(1);
    #[allow(clippy::cast_possible_truncation)]
    let now_minute = (now_ms / 60_000.0).floor() as i64;
    let range_minutes = i64::try_from(range.seconds() / 60).unwrap_or(i64::MAX);
    let kept = i64::try_from(crate::traffic::RETENTION.as_secs() / 60).unwrap_or(i64::MAX);
    let first = now_minute - range_minutes.min(kept);
    let mut series = StatsSeries::default();
    let mut classes = StatusClasses::default();
    let mut bucket = first.div_euclid(size) * size;
    while bucket <= now_minute {
        let (mut requests, mut client, mut server) = (0u64, 0u64, 0u64);
        for r in rollups
            .iter()
            .filter(|r| r.minute >= bucket && r.minute < bucket + size)
        {
            requests += r.requests;
            client += r.classes[2];
            server += r.classes[3];
            classes.ok += r.classes[0];
            classes.redirects += r.classes[1];
        }
        classes.client_errors += client;
        classes.server_errors += server;
        #[allow(clippy::cast_precision_loss)]
        series.push(
            ((bucket + size) * 60_000) as f64,
            (size * 60) as f64,
            requests,
            client,
            server,
            0,
        );
        bucket += size;
    }
    let requests = series.requests.iter().map(|n| u64::from(*n)).sum();
    #[allow(clippy::cast_precision_loss)]
    RouteStats {
        source: SourceKind::Connector,
        route: route.clone(),
        range,
        series,
        requests,
        bytes: 0,
        classes,
        statuses: Vec::new(),
        paths: Vec::new(),
        countries: Vec::new(),
        browsers: Vec::new(),
        bots: Vec::new(),
        cache: Vec::new(),
        origin_ms: None,
        ttfb_ms: None,
        available_from: (range_minutes > kept).then(|| (first * 60_000) as f64),
        unavailable: vec![
            StatsPart::Statuses,
            StatsPart::Paths,
            StatsPart::Countries,
            StatsPart::Browsers,
            StatsPart::Bots,
            StatsPart::Cache,
            StatsPart::Latency,
        ],
        fetched_at: now_ms,
    }
}

impl AnalyticsSource for ConnectorSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Connector
    }

    fn route_stats<'a>(
        &'a self,
        route: &'a RouteRef,
        range: AnalyticsRange,
    ) -> BoxFuture<'a, Result<Option<RouteStats>, AnalyticsError>> {
        Box::pin(async move {
            if !self
                .hostnames
                .contains(&route.hostname.to_ascii_lowercase())
            {
                return Ok(None);
            }
            let now = now_ms();
            #[allow(clippy::cast_possible_truncation)]
            let since =
                (now / 60_000.0).floor() as i64 - i64::try_from(range.seconds() / 60).unwrap_or(0);
            let rollups = self.local.rollups(&self.tunnel_id, since).await?;
            Ok(Some(stats_from_rollups(&rollups, route, range, now)))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rollup(minute: i64, requests: u64, server: u64) -> MinuteRollup {
        MinuteRollup {
            tunnel: "t".into(),
            minute,
            requests,
            errors: 0,
            classes: [requests - server, 0, 0, server],
            concurrent_max: 1,
            connections_min: 4,
            rtt_sum_ms: 0.0,
            rtt_samples: 0,
        }
    }

    #[test]
    fn sums_minutes_into_the_ranges_buckets() {
        let route = RouteRef {
            hostname: "a.x.com".into(),
            path: None,
        };
        let now = 1_000.0 * 60_000.0;
        let stats = stats_from_rollups(
            &[rollup(990, 10, 2), rollup(999, 5, 0), rollup(1_000, 1, 1)],
            &route,
            AnalyticsRange::Hour,
            now,
        );
        assert_eq!(stats.source, SourceKind::Connector);
        assert_eq!(stats.requests, 16);
        assert_eq!(stats.classes.server_errors, 3);
        assert_eq!(stats.series.at.len(), 61);
        assert_eq!(stats.series.at.last(), Some(&(1_001.0 * 60_000.0)));
        assert!(stats.available_from.is_none());
        let month = stats_from_rollups(&[], &route, AnalyticsRange::Month, now);
        assert!(month.available_from.is_some(), "only a week is kept");
    }

    #[tokio::test]
    async fn answers_only_for_its_hostnames() {
        let store = crate::store::Store::open_in_memory().unwrap();
        let source = ConnectorSource::new(Local::new(store), "t", &["A.x.com".into()]);
        let mine = RouteRef {
            hostname: "a.x.com".into(),
            path: None,
        };
        let other = RouteRef {
            hostname: "b.x.com".into(),
            path: None,
        };
        assert!(
            source
                .route_stats(&mine, AnalyticsRange::Day)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            source
                .route_stats(&other, AnalyticsRange::Day)
                .await
                .unwrap()
                .is_none()
        );
    }
}
