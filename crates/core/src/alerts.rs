//! Alerts: when a route goes down or comes back, answers with too many server errors,
//! gets slow, or a connector drops. Pure and time-driven like `health`: the uptime
//! monitor feeds it check results and windowed numbers and gets back [`Alert`]s to record
//! in Activity and, unless it's quiet hours or the user turned alerts off, to notify.
//!
//! Never spam: an alert fires once when its condition starts and once when it ends;
//! error-rate and latency alerts end only well below their threshold (hysteresis); and
//! the same subject notifies at most once per [`COOLDOWN_MS`] (flapping routes still show
//! every change in Activity).

use std::collections::HashMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use crate::{
    engine::{ActivityKind, ActivityRecord, Local},
    store::{Store, StoreError},
    text::{Text, msg},
    uptime::Cause,
};

/// The least time between two notifications about the same subject.
pub const COOLDOWN_MS: i64 = 15 * 60_000;
const RULES_KEY: &str = "alertRules";

/// What to alert about. Stored in settings (`alertRules`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AlertRules {
    /// A route stops answering.
    pub route_down: bool,
    /// Failed checks in a row before "down" (a check runs every minute).
    pub down_after: u32,
    /// A route that was down answers again.
    pub recovered: bool,
    /// Too many 5xx answers.
    pub error_rate: bool,
    /// The share of 5xx answers that alerts, in percent.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub error_rate_percent: f64,
    /// Over this many minutes.
    pub error_rate_minutes: u32,
    /// With at least this many requests in the window (a few requests prove nothing).
    pub min_requests: u32,
    /// Slow answers.
    pub latency: bool,
    /// The response time (P95 of the checks through the edge) that alerts, in ms.
    pub latency_ms: u32,
    /// Over this many minutes.
    pub latency_minutes: u32,
    /// A connector on this machine loses Cloudflare.
    pub connector_down: bool,
    /// Routes (`hostname` or `hostname/path`) that never alert.
    pub muted: Vec<String>,
}

impl Default for AlertRules {
    fn default() -> Self {
        Self {
            route_down: true,
            down_after: 3,
            recovered: true,
            error_rate: true,
            error_rate_percent: 5.0,
            error_rate_minutes: 10,
            min_requests: 20,
            latency: false,
            latency_ms: 3_000,
            latency_minutes: 10,
            connector_down: true,
            muted: Vec::new(),
        }
    }
}

impl AlertRules {
    /// Keeps values in sensible ranges (a hand-edited or old value can't break alerting).
    #[must_use]
    pub fn sanitized(mut self) -> Self {
        self.down_after = self.down_after.clamp(1, 60);
        self.error_rate_percent = if self.error_rate_percent.is_finite() {
            self.error_rate_percent.clamp(0.1, 100.0)
        } else {
            5.0
        };
        self.error_rate_minutes = self.error_rate_minutes.clamp(1, 60);
        self.min_requests = self.min_requests.clamp(1, 1_000_000);
        self.latency_ms = self.latency_ms.clamp(50, 120_000);
        self.latency_minutes = self.latency_minutes.clamp(1, 60);
        self.muted.sort();
        self.muted.dedup();
        self
    }

    fn muted(&self, subject: &str) -> bool {
        self.muted.iter().any(|m| m.eq_ignore_ascii_case(subject))
    }
}

/// Loads the rules (defaults when unset or unreadable).
///
/// # Errors
/// Database errors.
pub async fn load_rules(store: &Store) -> Result<AlertRules, StoreError> {
    let raw: Option<String> = store
        .call(|conn| {
            use rusqlite::OptionalExtension;
            Ok(conn
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![RULES_KEY],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .await?;
    // Stored rules are laid over the defaults, so a rule added later starts at its
    // default instead of discarding what the user chose.
    let mut merged = serde_json::to_value(AlertRules::default())?;
    if let (Some(serde_json::Value::Object(stored)), serde_json::Value::Object(base)) = (
        raw.and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).ok()),
        &mut merged,
    ) {
        base.extend(stored);
    }
    Ok(serde_json::from_value::<AlertRules>(merged)
        .unwrap_or_default()
        .sanitized())
}

/// Saves the rules and returns them as stored.
///
/// # Errors
/// Database errors.
pub async fn save_rules(store: &Store, rules: AlertRules) -> Result<AlertRules, StoreError> {
    let rules = rules.sanitized();
    let json = serde_json::to_string(&rules)?;
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![RULES_KEY, json],
            )?;
            Ok(())
        })
        .await?;
    Ok(rules)
}

/// Minutes since local midnight (the database's `localtime`, i.e. the system's zone), for
/// quiet hours.
///
/// # Errors
/// Database errors.
pub async fn local_minute(store: &Store) -> Result<u16, StoreError> {
    store
        .call(|conn| {
            Ok(conn.query_row(
                "SELECT CAST(strftime('%H', 'now', 'localtime') AS INTEGER) * 60
                      + CAST(strftime('%M', 'now', 'localtime') AS INTEGER)",
                [],
                |row| row.get(0),
            )?)
        })
        .await
}

/// Whether `minute` (since local midnight) is inside quiet hours `from..to`, which may
/// span midnight (22:00–07:00).
pub fn is_quiet(enabled: bool, from: u16, to: u16, minute: u16) -> bool {
    if !enabled || from == to {
        return false;
    }
    if from < to {
        (from..to).contains(&minute)
    } else {
        minute >= from || minute < to
    }
}

/// What kind of alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum AlertKind {
    /// A route stopped answering.
    RouteDown,
    /// It answers again.
    RouteRecovered,
    /// Too many server errors.
    ErrorRate,
    /// Back to normal.
    ErrorRateResolved,
    /// Slow answers.
    Latency,
    /// Fast again.
    LatencyResolved,
    /// A connector lost Cloudflare.
    ConnectorDown,
    /// It's connected again.
    ConnectorBack,
}

impl AlertKind {
    /// Whether this starts a problem (the others end one).
    pub fn is_problem(self) -> bool {
        matches!(
            self,
            Self::RouteDown | Self::ErrorRate | Self::Latency | Self::ConnectorDown
        )
    }
}

/// Something worth telling the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Alert {
    /// What happened.
    pub kind: AlertKind,
    /// The account it's in.
    pub account_id: String,
    /// The route (`hostname` or `hostname/path`) or tunnel it's about.
    pub subject: String,
    /// Hostnames involved.
    pub hostnames: Vec<String>,
    /// Title.
    pub title: Text,
    /// Body.
    pub body: Text,
    /// When, milliseconds since the epoch.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub at: i64,
    /// Whether to post a notification (false within the cooldown of the last one about
    /// the same subject; Activity still records it).
    pub notify: bool,
}

impl Alert {
    /// The Activity record of the alert.
    pub fn record(&self) -> ActivityRecord {
        ActivityRecord {
            kind: ActivityKind::Alert,
            hostnames: self.hostnames.clone(),
            tunnel: String::new(),
            steps: Vec::new(),
            changes: Vec::new(),
            summary: Some(self.title.clone()),
            error: self.kind.is_problem().then(|| self.body.clone()),
            leftovers: Vec::new(),
            connector_error: None,
            // Teitunnel's own finding, not something a person or an agent did.
            actor: None,
        }
    }
}

/// Records alerts in the account's Activity (`outcome`: `alert` or `resolved`).
pub async fn log(local: &Local, alerts: &[Alert]) {
    for alert in alerts {
        let outcome = if alert.kind.is_problem() {
            "alert"
        } else {
            "resolved"
        };
        let detail = [alert.body.english()];
        if let Err(err) = local
            .log(
                &alert.account_id,
                &alert.title.english(),
                outcome,
                &detail,
                Some(&alert.record()),
            )
            .await
        {
            tracing::warn!(%err, "couldn't record an alert in Activity");
        }
    }
}

/// One notification for several alerts (a batch of routes going down together is one
/// event for the user).
pub fn notice(alerts: &[&Alert]) -> Option<(Text, Text)> {
    match alerts {
        [] => None,
        [one] => Some((one.title.clone(), one.body.clone())),
        [first, rest @ ..] => Some((
            msg::alerts::many(alerts.len() as u64),
            msg::alerts::many_body(rest.len() as u64, &first.title),
        )),
    }
}

#[derive(Debug, Clone, Copy)]
struct Active {
    since: i64,
}

/// Which alerts are active, and when each subject last notified.
#[derive(Debug, Default)]
pub struct AlertEngine {
    active: HashMap<String, Active>,
    notified: HashMap<String, i64>,
}

fn route_host(subject: &str) -> String {
    subject.split('/').next().unwrap_or(subject).to_owned()
}

impl AlertEngine {
    fn fire(
        &mut self,
        kind: AlertKind,
        account: &str,
        subject: &str,
        hostnames: Vec<String>,
        (title, body): (Text, Text),
        now: i64,
    ) -> Alert {
        let notify = self
            .notified
            .get(subject)
            .is_none_or(|last| now - last >= COOLDOWN_MS);
        if notify {
            self.notified.insert(subject.to_owned(), now);
        }
        Alert {
            kind,
            account_id: account.to_owned(),
            subject: subject.to_owned(),
            hostnames,
            title,
            body,
            at: now,
            notify,
        }
    }

    /// A route's check: `failures` in a row so far (0 after a passing check).
    #[allow(clippy::too_many_arguments)]
    pub fn route_check(
        &mut self,
        rules: &AlertRules,
        account: &str,
        subject: &str,
        failures: u32,
        cause: Option<Cause>,
        now: i64,
    ) -> Option<Alert> {
        let key = format!("down:{subject}");
        let hostnames = vec![route_host(subject)];
        if failures == 0 {
            let active = self.active.remove(&key)?;
            if !rules.recovered || rules.muted(subject) {
                return None;
            }
            let minutes = u64::try_from((now - active.since).max(0) / 60_000).unwrap_or(0);
            let texts = (
                msg::alerts::recovered(subject),
                msg::alerts::recovered_body(minutes.max(1)),
            );
            return Some(self.fire(
                AlertKind::RouteRecovered,
                account,
                subject,
                hostnames,
                texts,
                now,
            ));
        }
        if !rules.route_down
            || rules.muted(subject)
            || failures < rules.down_after
            || self.active.contains_key(&key)
        {
            return None;
        }
        // Down since the first failed check.
        let since = now - i64::from(failures.saturating_sub(1)) * 60_000;
        self.active.insert(key, Active { since });
        let body = cause.map_or_else(msg::alerts::down_body, Cause::message);
        let texts = (msg::alerts::down(subject), body);
        Some(self.fire(
            AlertKind::RouteDown,
            account,
            subject,
            hostnames,
            texts,
            now,
        ))
    }

    /// A route's (or a tunnel's) requests and 5xx answers over the rule's window.
    #[allow(clippy::too_many_arguments)]
    pub fn error_rate(
        &mut self,
        rules: &AlertRules,
        account: &str,
        subject: &str,
        tunnel: bool,
        hostnames: Vec<String>,
        requests: u64,
        errors: u64,
        now: i64,
    ) -> Option<Alert> {
        let key = format!("errors:{subject}");
        #[allow(clippy::cast_precision_loss)]
        let percent = if requests == 0 {
            0.0
        } else {
            errors as f64 * 100.0 / requests as f64
        };
        let enough = requests >= u64::from(rules.min_requests);
        if self.active.contains_key(&key) {
            // Resolved only well below the threshold, so a rate hovering around it
            // doesn't alert over and over.
            if enough && percent >= rules.error_rate_percent / 2.0 {
                return None;
            }
            self.active.remove(&key);
            if !rules.error_rate || rules.muted(subject) {
                return None;
            }
            let texts = (
                msg::alerts::errors_resolved(subject),
                msg::alerts::errors_resolved_body(format_percent(rules.error_rate_percent)),
            );
            return Some(self.fire(
                AlertKind::ErrorRateResolved,
                account,
                subject,
                hostnames,
                texts,
                now,
            ));
        }
        if !rules.error_rate
            || rules.muted(subject)
            || !enough
            || percent < rules.error_rate_percent
        {
            return None;
        }
        self.active.insert(key, Active { since: now });
        let title = if tunnel {
            msg::alerts::errors_tunnel(subject)
        } else {
            msg::alerts::errors(subject)
        };
        let body =
            msg::alerts::errors_body(format_percent(percent), u64::from(rules.error_rate_minutes));
        Some(self.fire(
            AlertKind::ErrorRate,
            account,
            subject,
            hostnames,
            (title, body),
            now,
        ))
    }

    /// A route's P95 response time over the rule's window (None: too few checks).
    pub fn latency(
        &mut self,
        rules: &AlertRules,
        account: &str,
        subject: &str,
        p95_ms: Option<f64>,
        now: i64,
    ) -> Option<Alert> {
        let key = format!("latency:{subject}");
        let threshold = f64::from(rules.latency_ms);
        let p95 = p95_ms?;
        let hostnames = vec![route_host(subject)];
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let shown = p95.round().max(0.0) as u64;
        if self.active.contains_key(&key) {
            if p95 >= threshold * 0.8 {
                return None;
            }
            self.active.remove(&key);
            if !rules.latency || rules.muted(subject) {
                return None;
            }
            let texts = (msg::alerts::fast(subject), msg::alerts::fast_body(shown));
            return Some(self.fire(
                AlertKind::LatencyResolved,
                account,
                subject,
                hostnames,
                texts,
                now,
            ));
        }
        if !rules.latency || rules.muted(subject) || p95 < threshold {
            return None;
        }
        self.active.insert(key, Active { since: now });
        let texts = (
            msg::alerts::slow(subject),
            msg::alerts::slow_body(shown, u64::from(rules.latency_minutes)),
        );
        Some(self.fire(AlertKind::Latency, account, subject, hostnames, texts, now))
    }

    /// A connector on this machine went down or came back (the `health` policy already
    /// ignored blips).
    pub fn connector(
        &mut self,
        rules: &AlertRules,
        account: &str,
        tunnel: &str,
        down: bool,
        now: i64,
    ) -> Option<Alert> {
        if !rules.connector_down {
            return None;
        }
        let (kind, texts) = if down {
            (
                AlertKind::ConnectorDown,
                (
                    msg::alerts::connector_down(tunnel),
                    msg::alerts::connector_down_body(),
                ),
            )
        } else {
            (
                AlertKind::ConnectorBack,
                (
                    msg::alerts::connector_back(tunnel),
                    msg::alerts::connector_back_body(),
                ),
            )
        };
        Some(self.fire(
            kind,
            account,
            &format!("tunnel:{tunnel}"),
            Vec::new(),
            texts,
            now,
        ))
    }
}

/// `5`, `12.5`: a percentage without trailing zeros.
fn format_percent(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded:.1}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MIN: i64 = 60_000;

    #[test]
    fn down_after_n_failures_once_then_recovered() {
        let rules = AlertRules::default();
        let mut engine = AlertEngine::default();
        let check = |e: &mut AlertEngine, failures, now| {
            e.route_check(
                &rules,
                "acc",
                "a.x.com",
                failures,
                Some(Cause::NoConnector),
                now,
            )
        };
        assert!(check(&mut engine, 1, 0).is_none());
        assert!(check(&mut engine, 2, MIN).is_none());
        let down = check(&mut engine, 3, 2 * MIN).unwrap();
        assert_eq!(down.kind, AlertKind::RouteDown);
        assert_eq!(down.title.english(), "a.x.com is down");
        assert!(down.notify);
        assert!(check(&mut engine, 4, 3 * MIN).is_none(), "once per outage");
        let back = check(&mut engine, 0, 10 * MIN).unwrap();
        assert_eq!(back.kind, AlertKind::RouteRecovered);
        assert_eq!(back.body.english(), "It was down for 10 minutes.");
        assert!(!back.notify, "within the cooldown of the down notice");
        assert!(check(&mut engine, 0, 11 * MIN).is_none());
        // Down again later: notifies again.
        assert!(check(&mut engine, 3, 40 * MIN).unwrap().notify);
    }

    #[test]
    fn muted_routes_and_disabled_rules_stay_quiet() {
        let rules = AlertRules {
            muted: vec!["a.x.com".into()],
            ..AlertRules::default()
        };
        let mut engine = AlertEngine::default();
        assert!(
            engine
                .route_check(&rules, "acc", "a.x.com", 5, None, 0)
                .is_none()
        );
        let rules = AlertRules {
            route_down: false,
            ..AlertRules::default()
        };
        assert!(
            engine
                .route_check(&rules, "acc", "b.x.com", 5, None, 0)
                .is_none()
        );
        assert!(
            engine
                .connector(
                    &AlertRules {
                        connector_down: false,
                        ..AlertRules::default()
                    },
                    "acc",
                    "Mac",
                    true,
                    0
                )
                .is_none()
        );
        let down = engine
            .connector(&AlertRules::default(), "acc", "Mac", true, 0)
            .unwrap();
        assert_eq!(down.kind, AlertKind::ConnectorDown);
    }

    #[test]
    fn error_rate_has_a_floor_and_hysteresis() {
        let rules = AlertRules::default();
        let mut engine = AlertEngine::default();
        let hosts = || vec!["a.x.com".to_owned()];
        // 50% of 4 requests proves nothing.
        assert!(
            engine
                .error_rate(&rules, "acc", "a.x.com", false, hosts(), 4, 2, 0)
                .is_none()
        );
        let alert = engine
            .error_rate(&rules, "acc", "a.x.com", false, hosts(), 100, 12, MIN)
            .unwrap();
        assert_eq!(alert.kind, AlertKind::ErrorRate);
        assert_eq!(
            alert.body.english(),
            "12% of requests failed with a server error in the last 10 minutes."
        );
        // 3% is below 5% but above half of it: still active, no new alert.
        assert!(
            engine
                .error_rate(&rules, "acc", "a.x.com", false, hosts(), 100, 3, 2 * MIN)
                .is_none()
        );
        let resolved = engine
            .error_rate(&rules, "acc", "a.x.com", false, hosts(), 100, 1, 3 * MIN)
            .unwrap();
        assert_eq!(resolved.kind, AlertKind::ErrorRateResolved);
        let tunnel = engine
            .error_rate(&rules, "acc", "Mac", true, hosts(), 100, 50, 4 * MIN)
            .unwrap();
        assert_eq!(
            tunnel.title.english(),
            "Routes through Mac answer with errors"
        );
    }

    #[test]
    fn latency_alerts_when_enabled() {
        let mut rules = AlertRules::default();
        let mut engine = AlertEngine::default();
        assert!(
            engine
                .latency(&rules, "acc", "a.x.com", Some(9_000.0), 0)
                .is_none(),
            "off by default"
        );
        rules.latency = true;
        assert!(engine.latency(&rules, "acc", "a.x.com", None, 0).is_none());
        let slow = engine
            .latency(&rules, "acc", "a.x.com", Some(4_200.4), 0)
            .unwrap();
        assert_eq!(
            slow.body.english(),
            "Responses took 4200 ms (95th percentile) over the last 10 minutes."
        );
        assert!(
            engine
                .latency(&rules, "acc", "a.x.com", Some(2_900.0), MIN)
                .is_none()
        );
        let fast = engine
            .latency(&rules, "acc", "a.x.com", Some(800.0), 2 * MIN)
            .unwrap();
        assert_eq!(fast.kind, AlertKind::LatencyResolved);
    }

    #[test]
    fn coalesces_notices_and_knows_quiet_hours() {
        let rules = AlertRules::default();
        let mut engine = AlertEngine::default();
        let a = engine
            .route_check(&rules, "acc", "a.x.com", 3, None, 0)
            .unwrap();
        let b = engine
            .route_check(&rules, "acc", "b.x.com", 3, None, 0)
            .unwrap();
        let (title, body) = notice(&[&a, &b]).unwrap();
        assert_eq!(title.english(), "2 alerts");
        assert_eq!(body.english(), "a.x.com is down, and 1 more.");
        assert_eq!(notice(&[&a]).unwrap().0.english(), "a.x.com is down");
        assert!(notice(&[]).is_none());

        assert!(is_quiet(true, 22 * 60, 7 * 60, 23 * 60));
        assert!(is_quiet(true, 22 * 60, 7 * 60, 60));
        assert!(!is_quiet(true, 22 * 60, 7 * 60, 12 * 60));
        assert!(is_quiet(true, 60, 120, 90));
        assert!(!is_quiet(false, 60, 120, 90));
        assert!(!is_quiet(true, 60, 60, 60));
    }

    #[tokio::test]
    async fn rules_persist_sanitized() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load_rules(&store).await.unwrap(), AlertRules::default());
        let saved = save_rules(
            &store,
            AlertRules {
                down_after: 0,
                error_rate_percent: f64::NAN,
                muted: vec!["b".into(), "a".into(), "b".into()],
                ..AlertRules::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(saved.down_after, 1);
        assert!((saved.error_rate_percent - 5.0).abs() < f64::EPSILON);
        assert_eq!(saved.muted, ["a", "b"]);
        assert_eq!(load_rules(&store).await.unwrap(), saved);
        // Rules stored by an older version (fewer fields) keep what was chosen.
        store
            .call(|conn| {
                conn.execute(
                    "UPDATE settings SET value = '{\"routeDown\":false}' WHERE key = 'alertRules'",
                    [],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let old = load_rules(&store).await.unwrap();
        assert!(!old.route_down);
        assert_eq!(old.down_after, AlertRules::default().down_after);
        let minute = local_minute(&store).await.unwrap();
        assert!(minute < 24 * 60);
    }

    #[test]
    fn alerts_record_in_activity() {
        let rules = AlertRules::default();
        let mut engine = AlertEngine::default();
        let down = engine
            .route_check(&rules, "acc", "a.x.com/api", 3, Some(Cause::ServerError), 0)
            .unwrap();
        let record = down.record();
        assert_eq!(record.kind, ActivityKind::Alert);
        assert_eq!(record.hostnames, ["a.x.com"]);
        assert!(record.error.is_some());
    }
}
