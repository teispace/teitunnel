//! Uptime: every route this machine serves is checked through Cloudflare's edge once a
//! minute while the app, `teitunnel up` or `teitunnel serve` runs (one of them at a
//! time, by a lease in the database).
//!
//! Checks are stored raw for two days and summed per hour for 30 days, so uptime over a
//! day, a week and a month, a response-time chart and a status-page strip come from the
//! database. Two failed checks in a row open an incident; the next good one closes it.
//! Nothing is recorded while this computer is offline (a baseline connection to the edge
//! fails) and failures right after a wake from sleep are given one more minute, so a
//! closed laptop never shows up as an outage.

pub mod monitor;
mod probe;
pub mod store;

use std::{collections::HashMap, time::Duration};

use serde::{Deserialize, Serialize};

use crate::{
    accounts::Accounts,
    analytics::{RouteRef, path_prefix},
    engine::Local,
    text::{Text, msg},
};

pub use monitor::{Monitor, TickReport};
pub use store::UptimeStore;

/// How often routes are checked.
pub const INTERVAL: Duration = Duration::from_secs(60);
/// Failed checks in a row that open an incident.
pub const CONFIRM: u32 = 2;
/// How long raw checks are kept.
pub const RAW_RETENTION: Duration = Duration::from_secs(2 * 86_400);
/// How long hourly sums and closed incidents are kept.
pub const RETENTION: Duration = Duration::from_secs(30 * 86_400);

/// Why a check failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Cause {
    /// No connector is connected to the tunnel (Cloudflare error 1033).
    NoConnector,
    /// The hostname points at a tunnel that doesn't serve it (1016/530).
    TunnelMismatch,
    /// Cloudflare doesn't know the hostname (1001).
    NotOnCloudflare,
    /// The connector couldn't reach the origin (502).
    OriginUnreachable,
    /// The origin didn't answer in time (504).
    OriginTimeout,
    /// The origin answered with a server error (500, 503…).
    ServerError,
    /// The edge couldn't be reached for this hostname.
    EdgeUnreachable,
    /// The certificate doesn't cover the hostname.
    Certificate,
    /// The check took too long.
    Timeout,
    /// The DNS record is missing or points elsewhere.
    NoRecord,
}

impl Cause {
    /// The stored name (camelCase, as serialized).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoConnector => "noConnector",
            Self::TunnelMismatch => "tunnelMismatch",
            Self::NotOnCloudflare => "notOnCloudflare",
            Self::OriginUnreachable => "originUnreachable",
            Self::OriginTimeout => "originTimeout",
            Self::ServerError => "serverError",
            Self::EdgeUnreachable => "edgeUnreachable",
            Self::Certificate => "certificate",
            Self::Timeout => "timeout",
            Self::NoRecord => "noRecord",
        }
    }

    /// Reads a stored name.
    pub fn parse(value: &str) -> Option<Self> {
        [
            Self::NoConnector,
            Self::TunnelMismatch,
            Self::NotOnCloudflare,
            Self::OriginUnreachable,
            Self::OriginTimeout,
            Self::ServerError,
            Self::EdgeUnreachable,
            Self::Certificate,
            Self::Timeout,
            Self::NoRecord,
        ]
        .into_iter()
        .find(|c| c.as_str() == value)
    }

    /// One sentence for the UI and notifications.
    pub fn message(self) -> Text {
        use msg::uptime::cause as m;
        match self {
            Self::NoConnector => m::no_connector(),
            Self::TunnelMismatch => m::tunnel_mismatch(),
            Self::NotOnCloudflare => m::not_on_cloudflare(),
            Self::OriginUnreachable => m::origin_unreachable(),
            Self::OriginTimeout => m::origin_timeout(),
            Self::ServerError => m::server_error(),
            Self::EdgeUnreachable => m::edge_unreachable(),
            Self::Certificate => m::certificate(),
            Self::Timeout => m::timeout(),
            Self::NoRecord => m::no_record(),
        }
    }
}

/// What one check found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckOutcome {
    /// Whether the route works.
    pub ok: bool,
    /// The HTTP status, when there was an answer.
    pub status: Option<u16>,
    /// How long the answer took, in milliseconds.
    pub latency_ms: Option<u32>,
    /// Why it failed.
    pub cause: Option<Cause>,
}

/// A route to check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// Account the route is in.
    pub account_id: String,
    /// The tunnel that carries it (on this machine).
    pub tunnel_id: String,
    /// Hostname and path.
    pub route: RouteRef,
}

impl Target {
    /// The key checks, incidents and alerts use.
    pub fn key(&self) -> String {
        self.route.key()
    }

    /// The path the check requests.
    pub fn probe_path(&self) -> &str {
        self.route.path.as_deref().unwrap_or("/")
    }
}

/// Whether a rule's service is HTTP (others, like SSH, can't be checked with a GET).
fn is_http(service: &str) -> bool {
    ["http://", "https://", "unix:", "unix+tls:", "hello_world"]
        .iter()
        .any(|prefix| service.starts_with(prefix))
}

/// The routes this machine's tunnels serve, from the configuration Teitunnel last wrote
/// (no Cloudflare call). Skipped: wildcards, non-HTTP services, and path rules without a
/// literal prefix (a check couldn't be sure to reach them).
pub async fn targets(accounts: &Accounts, local: &Local) -> Vec<Target> {
    let mut out: Vec<Target> = Vec::new();
    for account in accounts.list().await.unwrap_or_default() {
        for tunnel in local.tunnels(&account.id).await.unwrap_or_default() {
            let ingress = local
                .applied_ingress(&tunnel.tunnel_id)
                .await
                .ok()
                .flatten()
                .unwrap_or_default();
            for rule in ingress {
                let Some(hostname) = rule.hostname.as_deref() else {
                    continue;
                };
                if hostname.starts_with('*') || !is_http(&rule.service) {
                    continue;
                }
                let path = match rule.path.as_deref() {
                    None => None,
                    Some(rule) => match path_prefix(rule) {
                        Some(prefix) if prefix == "/" => None,
                        Some(prefix) => Some(prefix),
                        None => continue,
                    },
                };
                let target = Target {
                    account_id: account.id.clone(),
                    tunnel_id: tunnel.tunnel_id.clone(),
                    route: RouteRef {
                        hostname: hostname.to_ascii_lowercase(),
                        path,
                    },
                };
                if !out.iter().any(|t| t.key() == target.key()) {
                    out.push(target);
                }
            }
        }
    }
    out
}

/// An outage of one route.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Incident {
    /// Row id.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub id: i64,
    /// Account.
    pub account_id: String,
    /// The route.
    pub route: RouteRef,
    /// First failed check, milliseconds since the epoch.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub started_at: i64,
    /// First good check after it, if it's over.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub ended_at: Option<i64>,
    /// Why the checks failed (at the start).
    pub cause: Cause,
}

/// A slice of time in the status strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UptimeBar {
    /// Start, milliseconds since the epoch.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub start: i64,
    /// End.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub end: i64,
    /// Checks made (0: no data, the app wasn't running or the computer was offline).
    pub checks: u32,
    /// Checks that passed.
    pub up: u32,
}

/// One route's uptime at a glance.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UptimeSummary {
    /// Account.
    pub account_id: String,
    /// The route.
    pub route: RouteRef,
    /// The last check passed (None: never checked).
    pub up: Option<bool>,
    /// When it was last checked.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub last_checked: Option<i64>,
    /// How long the last answer took.
    pub last_latency_ms: Option<u32>,
    /// Why the last check failed.
    pub last_cause: Option<Cause>,
    /// Share of passing checks over 24 hours (0–1).
    pub uptime_day: Option<f64>,
    /// Over 7 days.
    pub uptime_week: Option<f64>,
    /// Over 30 days.
    pub uptime_month: Option<f64>,
    /// Response time P95 over 24 hours, milliseconds.
    pub p95_ms: Option<f64>,
    /// The outage going on, if any.
    pub open_incident: Option<Incident>,
}

/// Response time over time (a check's time through the edge).
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LatencySeries {
    /// Time of each point, milliseconds since the epoch.
    #[cfg_attr(feature = "specta", specta(type = Vec<u32>))]
    pub at: Vec<i64>,
    /// Response time (the mean for hourly points), milliseconds; None: the check failed.
    pub ms: Vec<Option<f64>>,
}

/// One route's uptime in detail.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UptimeDetail {
    /// The summary.
    pub summary: UptimeSummary,
    /// 90 slices of the range, oldest first.
    pub bars: Vec<UptimeBar>,
    /// Response times over the range.
    pub latency: LatencySeries,
    /// Incidents in the range, newest first.
    pub incidents: Vec<Incident>,
}

/// Where a route's checks stand, for opening and closing incidents.
#[derive(Debug, Clone, Copy, Default)]
struct Track {
    failures: u32,
    first_failure: i64,
    cause: Option<Cause>,
    incident: bool,
}

/// What a check changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transition {
    /// Enough failures in a row: an outage began at `started_at`.
    Opened {
        /// The first failed check.
        started_at: i64,
        /// Its cause.
        cause: Cause,
    },
    /// A check passed during an outage.
    Closed,
}

/// Consecutive failures per route. Pure: fed check results, returns transitions.
#[derive(Debug, Default)]
pub struct Tracker {
    routes: HashMap<String, Track>,
}

impl Tracker {
    /// A tracker that knows about incidents still open in the database.
    pub fn resume(open: &[Incident]) -> Self {
        Self {
            routes: open
                .iter()
                .map(|i| {
                    (
                        i.route.key(),
                        Track {
                            failures: CONFIRM,
                            first_failure: i.started_at,
                            cause: Some(i.cause),
                            incident: true,
                        },
                    )
                })
                .collect(),
        }
    }

    /// Records a check of `key` at `at`.
    pub fn record(&mut self, key: &str, outcome: &CheckOutcome, at: i64) -> Option<Transition> {
        let track = self.routes.entry(key.to_owned()).or_default();
        if outcome.ok {
            let was_open = track.incident;
            *track = Track::default();
            return was_open.then_some(Transition::Closed);
        }
        if track.failures == 0 {
            track.first_failure = at;
            track.cause = outcome.cause;
        }
        track.failures += 1;
        if track.failures >= CONFIRM && !track.incident {
            track.incident = true;
            return Some(Transition::Opened {
                started_at: track.first_failure,
                cause: track.cause.unwrap_or(Cause::EdgeUnreachable),
            });
        }
        None
    }

    /// Failed checks in a row for `key`.
    pub fn failures(&self, key: &str) -> u32 {
        self.routes.get(key).map_or(0, |t| t.failures)
    }

    /// Forgets routes that are gone; returns those that had an open incident.
    pub fn retain(&mut self, keys: &[String]) -> Vec<String> {
        let gone: Vec<String> = self
            .routes
            .keys()
            .filter(|k| !keys.contains(k))
            .cloned()
            .collect();
        gone.into_iter()
            .filter(|k| self.routes.remove(k).is_some_and(|t| t.incident))
            .collect()
    }
}

/// Whether a tick came much later than scheduled: the computer slept (or the process
/// was suspended), so its network may still be coming up.
pub fn resumed(last_tick: Option<i64>, now: i64) -> bool {
    let interval = i64::try_from(INTERVAL.as_millis()).unwrap_or(60_000);
    last_tick.is_some_and(|last| now - last > 3 * interval)
}

/// Passing share, if anything was checked.
pub fn share(checks: u64, up: u64) -> Option<f64> {
    #[allow(clippy::cast_precision_loss)]
    (checks > 0).then(|| up as f64 / checks as f64)
}

/// The `q` quantile (0–1) of `values` (nearest rank), if any.
pub fn quantile(values: &mut [f64], q: f64) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let rank = ((q * values.len() as f64).ceil() as usize).clamp(1, values.len());
    values.get(rank - 1).copied()
}

/// Splits `[from, to)` into `count` bars and counts checks into them. `checks` are raw
/// `(at, ok)`; `hourly` are `(hour start ms, checks, up)` for times before `raw_from`.
pub fn bars(
    checks: &[(i64, bool)],
    hourly: &[(i64, u32, u32)],
    raw_from: i64,
    from: i64,
    to: i64,
    count: usize,
) -> Vec<UptimeBar> {
    let count = count.max(1);
    let width = ((to - from) / i64::try_from(count).unwrap_or(1)).max(1);
    let mut out: Vec<UptimeBar> = (0..count)
        .map(|i| {
            let start = from + width * i64::try_from(i).unwrap_or(0);
            UptimeBar {
                start,
                end: start + width,
                checks: 0,
                up: 0,
            }
        })
        .collect();
    let slot = |at: i64| -> Option<usize> {
        (at >= from && at < to).then(|| {
            usize::try_from((at - from) / width)
                .unwrap_or(0)
                .min(count - 1)
        })
    };
    for (at, ok) in checks {
        if let Some(bar) = slot(*at).and_then(|i| out.get_mut(i)) {
            bar.checks += 1;
            bar.up += u32::from(*ok);
        }
    }
    for (hour, n, up) in hourly.iter().filter(|(hour, _, _)| *hour < raw_from) {
        if let Some(bar) = slot(*hour).and_then(|i| out.get_mut(i)) {
            bar.checks += n;
            bar.up += up;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(ok: bool) -> CheckOutcome {
        CheckOutcome {
            ok,
            status: Some(if ok { 200 } else { 502 }),
            latency_ms: Some(40),
            cause: (!ok).then_some(Cause::OriginUnreachable),
        }
    }

    #[test]
    fn two_failures_open_an_incident_and_one_success_closes_it() {
        let mut tracker = Tracker::default();
        assert_eq!(tracker.record("a", &outcome(true), 0), None);
        assert_eq!(tracker.record("a", &outcome(false), 60), None);
        assert_eq!(
            tracker.record("a", &outcome(false), 120),
            Some(Transition::Opened {
                started_at: 60,
                cause: Cause::OriginUnreachable
            })
        );
        assert_eq!(tracker.failures("a"), 2);
        assert_eq!(tracker.record("a", &outcome(false), 180), None);
        assert_eq!(
            tracker.record("a", &outcome(true), 240),
            Some(Transition::Closed)
        );
        assert_eq!(tracker.failures("a"), 0);
        // A single blip never opens one.
        assert_eq!(tracker.record("a", &outcome(false), 300), None);
        assert_eq!(tracker.record("a", &outcome(true), 360), None);
    }

    #[test]
    fn resumes_open_incidents_and_forgets_removed_routes() {
        let open = Incident {
            id: 1,
            account_id: "acc".into(),
            route: RouteRef {
                hostname: "a.x.com".into(),
                path: None,
            },
            started_at: 5,
            ended_at: None,
            cause: Cause::NoConnector,
        };
        let mut tracker = Tracker::resume(&[open]);
        assert_eq!(tracker.failures("a.x.com"), CONFIRM);
        assert_eq!(tracker.retain(&[]), ["a.x.com"]);
        let mut tracker = Tracker::resume(&[]);
        assert_eq!(tracker.record("b", &outcome(true), 0), None);
        assert!(tracker.retain(&[]).is_empty());
    }

    #[test]
    fn notices_sleep() {
        assert!(!resumed(None, 1_000_000));
        assert!(!resumed(Some(0), 61_000));
        assert!(!resumed(Some(0), 150_000));
        assert!(resumed(Some(0), 30 * 60_000));
    }

    #[test]
    fn computes_shares_quantiles_and_bars() {
        assert_eq!(share(0, 0), None);
        assert_eq!(share(4, 3), Some(0.75));
        let mut values = vec![5.0, 1.0, 3.0, 2.0, 4.0];
        assert_eq!(quantile(&mut values, 0.5), Some(3.0));
        assert_eq!(quantile(&mut values, 0.95), Some(5.0));
        assert_eq!(quantile(&mut [], 0.95), None);

        let bars = bars(
            &[(100, true), (150, false), (250, true)],
            &[(0, 60, 59), (200, 60, 60)],
            100,
            0,
            300,
            3,
        );
        assert_eq!(
            bars.iter().map(|b| (b.checks, b.up)).collect::<Vec<_>>(),
            // The hourly row at 200 is covered by raw checks and not counted twice.
            [(60, 59), (2, 1), (1, 1)]
        );
    }

    #[test]
    fn causes_round_trip() {
        for cause in [Cause::NoConnector, Cause::ServerError, Cause::NoRecord] {
            assert_eq!(Cause::parse(cause.as_str()), Some(cause));
            let json = serde_json::to_string(&cause).unwrap();
            assert_eq!(json, format!("\"{}\"", cause.as_str()));
        }
        assert_eq!(Cause::parse("nope"), None);
    }
}
