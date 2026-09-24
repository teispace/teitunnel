//! The uptime and alert loop, shared by the app, `teitunnel up` and `teitunnel serve`:
//! each calls [`Monitor::tick`] every [`INTERVAL`](super::INTERVAL) and delivers the
//! alerts it returns (a notification, or a line on the terminal).

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use futures_util::{StreamExt, stream};
use tokio::sync::Mutex;

use super::{
    INTERVAL, Target, Tracker, Transition, UptimeStore, probe, quantile, resumed, targets,
};
use crate::{
    accounts::Accounts,
    alerts::{self, Alert, AlertEngine, AlertRules},
    analytics::{
        Analytics, AnalyticsError, AnalyticsRange, AnalyticsSource, RouteRef,
        connector::ConnectorSource, recent_errors,
    },
    engine::{Edge, Local},
    store::Store,
};

/// Checks at once.
const PARALLEL: usize = 8;
/// How often error rates and latency are evaluated.
const RATES_EVERY_MS: i64 = 5 * 60_000;
/// How long an account without the analytics permission isn't asked again.
const NO_ANALYTICS_MS: i64 = 60 * 60_000;
const PRUNE_EVERY_MS: i64 = 60 * 60_000;

/// What a tick did.
#[derive(Debug, Default)]
pub struct TickReport {
    /// Another process runs the checks.
    pub skipped: bool,
    /// This computer couldn't reach Cloudflare: nothing was recorded.
    pub offline: bool,
    /// The tick came long after the previous one (sleep): failures weren't counted.
    pub resumed: bool,
    /// Routes checked.
    pub checked: usize,
    /// Alerts raised (already recorded in Activity).
    pub alerts: Vec<Alert>,
}

#[derive(Debug, Default)]
struct State {
    tracker: Option<Tracker>,
    alerts: AlertEngine,
    last_tick: Option<i64>,
    last_rates: i64,
    last_prune: i64,
    no_analytics: HashMap<String, i64>,
}

/// Runs uptime checks and alert evaluation. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Monitor {
    uptime: UptimeStore,
    store: Store,
    local: Local,
    accounts: Accounts,
    analytics: Analytics,
    edge: Edge,
    owner: String,
    state: Arc<Mutex<State>>,
}

fn interval_ms() -> i64 {
    i64::try_from(INTERVAL.as_millis()).unwrap_or(60_000)
}

impl Monitor {
    /// A monitor; `owner` names this process for the lease (`app`, `cli-1234`).
    pub fn new(
        store: Store,
        accounts: Accounts,
        analytics: Analytics,
        edge: Edge,
        owner: &str,
    ) -> Self {
        Self {
            uptime: UptimeStore::new(store.clone()),
            local: Local::new(store.clone()),
            store,
            accounts,
            analytics,
            edge,
            owner: owner.to_owned(),
            state: Arc::default(),
        }
    }

    /// The uptime data (for views).
    pub fn uptime(&self) -> &UptimeStore {
        &self.uptime
    }

    /// Every route this machine serves, with its uptime at `now`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn summaries(
        &self,
        now: i64,
    ) -> Result<Vec<super::UptimeSummary>, crate::store::StoreError> {
        let targets = targets(&self.accounts, &self.local).await;
        self.uptime.summaries(&targets, now).await
    }

    /// One route's uptime over `range`, or `None` when this machine doesn't serve it.
    ///
    /// # Errors
    /// Database errors.
    pub async fn detail(
        &self,
        route: &RouteRef,
        range: AnalyticsRange,
        now: i64,
    ) -> Result<Option<super::UptimeDetail>, crate::store::StoreError> {
        let targets = targets(&self.accounts, &self.local).await;
        let Some(target) = targets.iter().find(|t| t.route.key() == route.key()) else {
            return Ok(None);
        };
        self.uptime.detail(target, range, now).await.map(Some)
    }

    /// Gives up the lease (call when the process stops).
    pub async fn release(&self) {
        let _ = self.uptime.release(&self.owner).await;
    }

    /// Checks every route once and evaluates the alert rules. `paused` tunnels (stopped
    /// on purpose) aren't checked.
    pub async fn tick(&self, now: i64, paused: &HashSet<String>) -> TickReport {
        let mut report = TickReport::default();
        match self.uptime.claim(&self.owner, now, 3 * interval_ms()).await {
            Ok(true) => {}
            Ok(false) => {
                report.skipped = true;
                return report;
            }
            Err(err) => {
                tracing::warn!(%err, "uptime: couldn't take the lease");
                report.skipped = true;
                return report;
            }
        }
        let mut state = self.state.lock().await;
        if state.tracker.is_none() {
            let open = self.uptime.open_incidents().await.unwrap_or_default();
            state.tracker = Some(Tracker::resume(&open));
        }
        report.resumed = resumed(state.last_tick, now);
        state.last_tick = Some(now);
        if !probe::baseline(self.edge).await {
            report.offline = true;
            return report;
        }
        let rules = alerts::load_rules(&self.store).await.unwrap_or_default();
        let targets: Vec<Target> = targets(&self.accounts, &self.local)
            .await
            .into_iter()
            .filter(|t| !paused.contains(&t.tunnel_id))
            .collect();
        let addr = probe::edge_address(self.edge).await;
        let edge = self.edge;
        let results: Vec<(Target, super::CheckOutcome)> = stream::iter(targets.clone())
            .map(|target| async move {
                let outcome =
                    probe::check(edge, addr, &target.route.hostname, target.probe_path()).await;
                (target, outcome)
            })
            .buffer_unordered(PARALLEL)
            .collect()
            .await;
        report.checked = results.len();

        let State {
            tracker, alerts, ..
        } = &mut *state;
        let tracker = tracker.get_or_insert_with(Tracker::default);
        for (target, outcome) in &results {
            // Just after a wake the network may still be joining: give failures a minute.
            if report.resumed && !outcome.ok {
                continue;
            }
            let key = target.key();
            if let Err(err) = self.uptime.record(&key, now, *outcome).await {
                tracing::warn!(%err, "uptime: couldn't store a check");
            }
            match tracker.record(&key, outcome, now) {
                Some(Transition::Opened { started_at, cause }) => {
                    let _ = self.uptime.open_incident(target, started_at, cause).await;
                }
                Some(Transition::Closed) => {
                    let _ = self.uptime.close_incident(&key, now).await;
                }
                None => {}
            }
            if let Some(alert) = alerts.route_check(
                &rules,
                &target.account_id,
                &key,
                tracker.failures(&key),
                outcome.cause,
                now,
            ) {
                report.alerts.push(alert);
            }
        }
        // Routes that are gone (removed, or their tunnel paused) end their outages.
        let keys: Vec<String> = targets.iter().map(Target::key).collect();
        for gone in tracker.retain(&keys) {
            let _ = self.uptime.close_incident(&gone, now).await;
        }

        if now - state.last_rates >= RATES_EVERY_MS {
            state.last_rates = now;
            let rated = self.rates(&rules, &targets, &mut state, now).await;
            report.alerts.extend(rated);
        }
        if now - state.last_prune >= PRUNE_EVERY_MS {
            state.last_prune = now;
            if let Err(err) = self.uptime.prune(now).await {
                tracing::warn!(%err, "uptime: couldn't prune old checks");
            }
        }
        drop(state);
        alerts::log(&self.local, &report.alerts).await;
        report
    }

    /// Error-rate and latency alerts. Error rates come from the edge per hostname when
    /// the account allows analytics, otherwise from this machine's connectors per tunnel.
    async fn rates(
        &self,
        rules: &AlertRules,
        targets: &[Target],
        state: &mut State,
        now: i64,
    ) -> Vec<Alert> {
        let mut out = Vec::new();
        if rules.latency {
            let since = now - i64::from(rules.latency_minutes) * 60_000;
            for target in targets {
                let key = target.key();
                let mut values = self.uptime.latencies(&key, since).await.unwrap_or_default();
                let p95 = if values.len() >= 3 {
                    quantile(&mut values, 0.95)
                } else {
                    None
                };
                out.extend(
                    state
                        .alerts
                        .latency(rules, &target.account_id, &key, p95, now),
                );
            }
        }
        if !rules.error_rate {
            return out;
        }
        let mut by_account: HashMap<&str, Vec<&Target>> = HashMap::new();
        for target in targets {
            by_account
                .entry(&target.account_id)
                .or_default()
                .push(target);
        }
        #[allow(clippy::cast_precision_loss)]
        let now_f = now as f64;
        for (account, list) in by_account {
            let allowed = state
                .no_analytics
                .get(account)
                .is_none_or(|until| *until <= now);
            if allowed {
                let mut hosts: Vec<String> =
                    list.iter().map(|t| t.route.hostname.clone()).collect();
                hosts.sort();
                hosts.dedup();
                match self
                    .analytics
                    .summary(&self.accounts, account, &hosts, AnalyticsRange::Hour)
                    .await
                {
                    Ok(summary) => {
                        for host in &summary.hosts {
                            let (requests, errors) = recent_errors(
                                &summary.series(host),
                                now_f,
                                rules.error_rate_minutes,
                            );
                            out.extend(state.alerts.error_rate(
                                rules,
                                account,
                                &host.hostname,
                                false,
                                vec![host.hostname.clone()],
                                requests,
                                errors,
                                now,
                            ));
                        }
                        continue;
                    }
                    Err(AnalyticsError::Permission | AnalyticsError::NotOnPlan) => {
                        state
                            .no_analytics
                            .insert(account.to_owned(), now + NO_ANALYTICS_MS);
                    }
                    Err(err) => {
                        tracing::debug!(%err, "alerts: edge analytics unavailable");
                    }
                }
            }
            // Per tunnel, from this machine's connectors.
            let mut tunnels: HashMap<&str, Vec<String>> = HashMap::new();
            for target in &list {
                tunnels
                    .entry(target.tunnel_id.as_str())
                    .or_default()
                    .push(target.route.hostname.clone());
            }
            let names: HashMap<String, String> = self
                .local
                .tunnels(account)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|t| (t.tunnel_id, t.name))
                .collect();
            for (tunnel, hostnames) in tunnels {
                let Some(first) = hostnames.first() else {
                    continue;
                };
                let source = ConnectorSource::new(self.local.clone(), tunnel, &hostnames);
                let route = RouteRef {
                    hostname: first.clone(),
                    path: None,
                };
                let Ok(Some(stats)) = source.route_stats(&route, AnalyticsRange::Hour).await else {
                    continue;
                };
                let (requests, errors) =
                    recent_errors(&stats.series, now_f, rules.error_rate_minutes);
                let name = names
                    .get(tunnel)
                    .cloned()
                    .unwrap_or_else(|| tunnel.to_owned());
                out.extend(state.alerts.error_rate(
                    rules, account, &name, true, hostnames, requests, errors, now,
                ));
            }
        }
        out
    }

    /// Records a connector going down or coming back (from the `health` policy) as an
    /// alert, when the rules ask for it.
    pub async fn connector_changed(
        &self,
        account: &str,
        tunnel_name: &str,
        down: bool,
        now: i64,
    ) -> Option<Alert> {
        let rules = alerts::load_rules(&self.store).await.unwrap_or_default();
        let alert =
            self.state
                .lock()
                .await
                .alerts
                .connector(&rules, account, tunnel_name, down, now)?;
        alerts::log(&self.local, std::slice::from_ref(&alert)).await;
        Some(alert)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

    use super::*;
    use crate::{
        accounts::Accounts,
        secrets::{MemoryStore, Secrets},
    };

    async fn monitor_with(
        server: &MockServer,
        routes: &[(&str, Option<&str>)],
    ) -> (Monitor, Store) {
        monitor_at(*server.address(), routes).await
    }

    async fn monitor_at(
        edge: std::net::SocketAddr,
        routes: &[(&str, Option<&str>)],
    ) -> (Monitor, Store) {
        let store = Store::open_in_memory().unwrap();
        store
            .call(|conn| {
                conn.execute(
                    "INSERT INTO accounts (id, name, credential, added_at) VALUES ('acc', 'Acc', 'apiToken', 0)",
                    [],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        let local = Local::new(store.clone());
        local.set_machine_tunnel("acc", "t1", "Mac").await.unwrap();
        let ingress: Vec<cf_api::IngressRule> = routes
            .iter()
            .map(|(host, path)| {
                serde_json::from_value(serde_json::json!({
                    "hostname": host, "path": path, "service": "http://localhost:3000"
                }))
                .unwrap()
            })
            .collect();
        local.set_applied("t1", 1, &ingress).await.unwrap();
        let secrets: Secrets = Arc::new(MemoryStore::default());
        let accounts = Accounts::with_api_base(store.clone(), secrets, "http://127.0.0.1:9", None);
        let monitor = Monitor::new(
            store.clone(),
            accounts,
            Analytics::default(),
            Edge::Test(edge),
            "test",
        );
        (monitor, store)
    }

    #[tokio::test]
    async fn checks_routes_opens_incidents_and_alerts_once() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(530).set_body_string("error code: 1033"))
            .mount(&server)
            .await;
        let (monitor, store) = monitor_with(
            &server,
            &[
                ("app.teitunnel-test.invalid", None),
                ("ssh.teitunnel-test.invalid", Some("\\.png$")),
            ],
        )
        .await;
        let none = HashSet::new();
        let mut alerts = Vec::new();
        for minute in 0..4 {
            let report = monitor.tick(minute * 60_000, &none).await;
            assert!(!report.offline && !report.skipped && !report.resumed);
            assert_eq!(report.checked, 1, "the unprobeable path rule is skipped");
            alerts.extend(report.alerts);
        }
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(alerts[0].kind, alerts::AlertKind::RouteDown);
        let open = monitor.uptime().open_incidents().await.unwrap();
        assert_eq!(open.len(), 1);
        assert_eq!(open[0].started_at, 0);
        let activity = Local::new(store).activity("acc", 10).await.unwrap();
        assert_eq!(activity[0].outcome, "alert");

        // The route recovers.
        server.reset().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let report = monitor.tick(4 * 60_000, &none).await;
        assert_eq!(report.alerts.len(), 1);
        assert_eq!(report.alerts[0].kind, alerts::AlertKind::RouteRecovered);
        assert!(monitor.uptime().open_incidents().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn records_nothing_offline_or_just_after_sleep() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(502))
            .mount(&server)
            .await;
        let (monitor, _store) =
            monitor_with(&server, &[("app.teitunnel-test.invalid", None)]).await;
        let none = HashSet::new();
        monitor.tick(0, &none).await;
        // Two hours later (the lid was closed): the failure isn't counted.
        let report = monitor.tick(2 * 3_600_000, &none).await;
        assert!(report.resumed);
        let target = targets(&monitor.accounts, &monitor.local).await;
        let summary = &monitor
            .uptime()
            .summaries(&target, 2 * 3_600_000)
            .await
            .unwrap()[0];
        assert_eq!(summary.last_checked, Some(0));

        // No network at all: nothing recorded.
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            listener.local_addr().unwrap()
        };
        let (offline, _store) = monitor_at(closed, &[("app.teitunnel-test.invalid", None)]).await;
        let report = offline.tick(0, &none).await;
        assert!(report.offline);
        assert_eq!(report.checked, 0);
    }

    #[tokio::test]
    async fn skips_paused_tunnels_and_other_runners() {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        let (monitor, store) = monitor_with(&server, &[("app.teitunnel-test.invalid", None)]).await;
        let paused = HashSet::from(["t1".to_owned()]);
        assert_eq!(monitor.tick(0, &paused).await.checked, 0);
        UptimeStore::new(store)
            .claim("someone-else", 0, 10 * 60_000)
            .await
            .ok();
        // Our own lease from the first tick is still valid, so the other can't take it…
        let other = Monitor {
            owner: "other".into(),
            ..monitor.clone()
        };
        assert!(other.tick(60_000, &HashSet::new()).await.skipped);
        monitor.release().await;
        assert!(!other.tick(120_000, &HashSet::new()).await.skipped);
    }
}
