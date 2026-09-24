//! Uptime checks, hourly sums and incidents in SQLite (migration 12).

use rusqlite::{OptionalExtension, params};

use super::{
    Cause, CheckOutcome, Incident, LatencySeries, RAW_RETENTION, RETENTION, Target, UptimeBar,
    UptimeDetail, UptimeSummary, bars, quantile, share,
};
use crate::{
    analytics::{AnalyticsRange, RouteRef},
    store::{Store, StoreError},
};

const HOUR_MS: i64 = 3_600_000;
const DAY_MS: i64 = 24 * HOUR_MS;
/// Bars in a status strip.
pub const BARS: usize = 90;
/// The settings key of the runner lease.
const LEASE_KEY: &str = "uptimeRunner";

fn ms(duration: std::time::Duration) -> i64 {
    i64::try_from(duration.as_millis()).unwrap_or(i64::MAX)
}

fn incident_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Incident> {
    let cause: String = row.get(6)?;
    Ok(Incident {
        id: row.get(0)?,
        account_id: row.get(1)?,
        route: RouteRef {
            hostname: row.get(2)?,
            path: row.get(3)?,
        },
        started_at: row.get(4)?,
        ended_at: row.get(5)?,
        cause: Cause::parse(&cause).unwrap_or(Cause::EdgeUnreachable),
    })
}

const INCIDENT_COLUMNS: &str = "id, account_id, hostname, path, started_at, ended_at, cause";

/// Uptime data. Cheap to clone.
#[derive(Debug, Clone)]
pub struct UptimeStore {
    store: Store,
}

impl UptimeStore {
    /// Uptime data in `store`.
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    /// Takes (or renews) the right to run checks for `ttl_ms`, unless another process
    /// holds it. The app and `teitunnel up`/`serve` on one machine would otherwise check
    /// every route twice.
    ///
    /// # Errors
    /// Database errors.
    pub async fn claim(&self, owner: &str, now: i64, ttl_ms: i64) -> Result<bool, StoreError> {
        let owner = owner.to_owned();
        self.store
            .call(move |conn| {
                let tx = conn.transaction()?;
                let current: Option<String> = tx
                    .query_row(
                        "SELECT value FROM settings WHERE key = ?1",
                        params![LEASE_KEY],
                        |row| row.get(0),
                    )
                    .optional()?;
                let held_by_other = current
                    .and_then(|raw| serde_json::from_str::<(String, i64)>(&raw).ok())
                    .is_some_and(|(holder, until)| holder != owner && until > now);
                if held_by_other {
                    return Ok(false);
                }
                tx.execute(
                    "INSERT INTO settings (key, value) VALUES (?1, ?2)
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![LEASE_KEY, serde_json::to_string(&(owner, now + ttl_ms))?],
                )?;
                tx.commit()?;
                Ok(true)
            })
            .await
    }

    /// Gives the lease up (the process is stopping).
    ///
    /// # Errors
    /// Database errors.
    pub async fn release(&self, owner: &str) -> Result<(), StoreError> {
        let owner = owner.to_owned();
        self.store
            .call(move |conn| {
                let current: Option<String> = conn
                    .query_row(
                        "SELECT value FROM settings WHERE key = ?1",
                        params![LEASE_KEY],
                        |row| row.get(0),
                    )
                    .optional()?;
                let mine = current
                    .and_then(|raw| serde_json::from_str::<(String, i64)>(&raw).ok())
                    .is_some_and(|(holder, _)| holder == owner);
                if mine {
                    conn.execute("DELETE FROM settings WHERE key = ?1", params![LEASE_KEY])?;
                }
                Ok(())
            })
            .await
    }

    /// Stores a check and adds it to its hour.
    ///
    /// # Errors
    /// Database errors.
    pub async fn record(
        &self,
        route: &str,
        at: i64,
        outcome: CheckOutcome,
    ) -> Result<(), StoreError> {
        let route = route.to_owned();
        self.store
            .call(move |conn| {
                let tx = conn.transaction()?;
                tx.execute(
                    "INSERT OR REPLACE INTO uptime_checks (route, at, ok, status, latency_ms, cause)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![
                        route,
                        at,
                        i64::from(outcome.ok),
                        outcome.status,
                        outcome.latency_ms,
                        outcome.cause.map(Cause::as_str)
                    ],
                )?;
                let latency = outcome.latency_ms.filter(|_| outcome.ok);
                tx.execute(
                    "INSERT INTO uptime_hourly (route, hour, checks, up, latency_sum_ms, latency_samples)
                     VALUES (?1, ?2, 1, ?3, ?4, ?5)
                     ON CONFLICT(route, hour) DO UPDATE SET
                        checks = checks + 1,
                        up = up + excluded.up,
                        latency_sum_ms = latency_sum_ms + excluded.latency_sum_ms,
                        latency_samples = latency_samples + excluded.latency_samples",
                    params![
                        route,
                        at.div_euclid(HOUR_MS) * HOUR_MS,
                        i64::from(outcome.ok),
                        latency.unwrap_or(0),
                        i64::from(latency.is_some())
                    ],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await
    }

    /// Opens an incident (none may already be open for the route).
    ///
    /// # Errors
    /// Database errors.
    pub async fn open_incident(
        &self,
        target: &Target,
        started_at: i64,
        cause: Cause,
    ) -> Result<(), StoreError> {
        let (key, account, route) = (
            target.key(),
            target.account_id.clone(),
            target.route.clone(),
        );
        self.store
            .call(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO incidents (route, account_id, hostname, path, started_at, cause)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![key, account, route.hostname, route.path, started_at, cause.as_str()],
                )?;
                Ok(())
            })
            .await
    }

    /// Closes the route's open incident, if any; returns it.
    ///
    /// # Errors
    /// Database errors.
    pub async fn close_incident(
        &self,
        route: &str,
        at: i64,
    ) -> Result<Option<Incident>, StoreError> {
        let route = route.to_owned();
        self.store
            .call(move |conn| {
                let open = conn
                    .query_row(
                        &format!(
                            "SELECT {INCIDENT_COLUMNS} FROM incidents WHERE route = ?1 AND ended_at IS NULL"
                        ),
                        params![route],
                        incident_row,
                    )
                    .optional()?;
                if let Some(incident) = &open {
                    conn.execute(
                        "UPDATE incidents SET ended_at = ?2 WHERE id = ?1",
                        params![incident.id, at],
                    )?;
                }
                Ok(open.map(|i| Incident {
                    ended_at: Some(at),
                    ..i
                }))
            })
            .await
    }

    /// Incidents still open.
    ///
    /// # Errors
    /// Database errors.
    pub async fn open_incidents(&self) -> Result<Vec<Incident>, StoreError> {
        self.store
            .call(|conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {INCIDENT_COLUMNS} FROM incidents WHERE ended_at IS NULL"
                ))?;
                let rows = stmt
                    .query_map([], incident_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    /// Recent passing checks' response times of `route` since `since`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn latencies(&self, route: &str, since: i64) -> Result<Vec<f64>, StoreError> {
        let route = route.to_owned();
        self.store
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT latency_ms FROM uptime_checks
                     WHERE route = ?1 AND at >= ?2 AND ok = 1 AND latency_ms IS NOT NULL",
                )?;
                let rows = stmt
                    .query_map(params![route, since], |row| row.get::<_, i64>(0))?
                    .collect::<Result<Vec<_>, _>>()?;
                #[allow(clippy::cast_precision_loss)]
                Ok(rows.into_iter().map(|v| v as f64).collect())
            })
            .await
    }

    /// Deletes raw checks older than two days, and hourly sums and closed incidents
    /// older than 30.
    ///
    /// # Errors
    /// Database errors.
    pub async fn prune(&self, now: i64) -> Result<(), StoreError> {
        self.store
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM uptime_checks WHERE at < ?1",
                    params![now - ms(RAW_RETENTION)],
                )?;
                conn.execute(
                    "DELETE FROM uptime_hourly WHERE hour < ?1",
                    params![now - ms(RETENTION)],
                )?;
                conn.execute(
                    "DELETE FROM incidents WHERE ended_at IS NOT NULL AND ended_at < ?1",
                    params![now - ms(RETENTION)],
                )?;
                Ok(())
            })
            .await
    }

    /// Summaries of `targets` at `now`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn summaries(
        &self,
        targets: &[Target],
        now: i64,
    ) -> Result<Vec<UptimeSummary>, StoreError> {
        let targets = targets.to_vec();
        self.store
            .call(move |conn| {
                targets
                    .iter()
                    .map(|t| summary(conn, t, now))
                    .collect::<Result<Vec<_>, StoreError>>()
            })
            .await
    }

    /// One route over `range`: the strip, response times and incidents.
    ///
    /// # Errors
    /// Database errors.
    pub async fn detail(
        &self,
        target: &Target,
        range: AnalyticsRange,
        now: i64,
    ) -> Result<UptimeDetail, StoreError> {
        let target = target.clone();
        self.store
            .call(move |conn| {
                let key = target.key();
                let from = now - i64::try_from(range.seconds()).unwrap_or(0) * 1000;
                let raw_from = now - ms(RAW_RETENTION);
                let mut stmt = conn.prepare_cached(
                    "SELECT at, ok, latency_ms FROM uptime_checks WHERE route = ?1 AND at >= ?2 ORDER BY at",
                )?;
                let raw = stmt
                    .query_map(params![key, from], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)? == 1,
                            row.get::<_, Option<i64>>(2)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                let mut stmt = conn.prepare_cached(
                    "SELECT hour, checks, up, latency_sum_ms, latency_samples FROM uptime_hourly
                     WHERE route = ?1 AND hour >= ?2 ORDER BY hour",
                )?;
                let hourly = stmt
                    .query_map(params![key, from.div_euclid(HOUR_MS) * HOUR_MS], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, u32>(1)?,
                            row.get::<_, u32>(2)?,
                            row.get::<_, i64>(3)?,
                            row.get::<_, i64>(4)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                let checks: Vec<(i64, bool)> = raw.iter().map(|(at, ok, _)| (*at, *ok)).collect();
                let counts: Vec<(i64, u32, u32)> =
                    hourly.iter().map(|(h, n, up, _, _)| (*h, *n, *up)).collect();
                let bars: Vec<UptimeBar> = bars(&checks, &counts, raw_from, from, now, BARS);
                // Up to two days: every check; beyond: hourly means.
                #[allow(clippy::cast_precision_loss)]
                let latency = if range.seconds() <= 2 * 86_400 {
                    LatencySeries {
                        at: raw.iter().map(|(at, _, _)| *at).collect(),
                        ms: raw
                            .iter()
                            .map(|(_, ok, l)| l.filter(|_| *ok).map(|v| v as f64))
                            .collect(),
                    }
                } else {
                    LatencySeries {
                        at: hourly.iter().map(|(h, ..)| h + HOUR_MS).collect(),
                        ms: hourly
                            .iter()
                            .map(|(_, _, _, sum, n)| (*n > 0).then(|| *sum as f64 / *n as f64))
                            .collect(),
                    }
                };
                let mut stmt = conn.prepare_cached(&format!(
                    "SELECT {INCIDENT_COLUMNS} FROM incidents
                     WHERE route = ?1 AND (ended_at IS NULL OR ended_at >= ?2)
                     ORDER BY started_at DESC LIMIT 50"
                ))?;
                let incidents = stmt
                    .query_map(params![key, from], incident_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(UptimeDetail {
                    summary: summary(conn, &target, now)?,
                    bars,
                    latency,
                    incidents,
                })
            })
            .await
    }
}

fn summary(
    conn: &rusqlite::Connection,
    target: &Target,
    now: i64,
) -> Result<UptimeSummary, StoreError> {
    let key = target.key();
    let last = conn
        .query_row(
            "SELECT at, ok, latency_ms, cause FROM uptime_checks WHERE route = ?1 ORDER BY at DESC LIMIT 1",
            params![key],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)? == 1,
                    row.get::<_, Option<u32>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .optional()?;
    let counts = |row: &rusqlite::Row<'_>| -> rusqlite::Result<(u64, u64)> {
        let (n, up): (i64, i64) = (row.get(0)?, row.get(1)?);
        Ok((
            u64::try_from(n).unwrap_or(0),
            u64::try_from(up).unwrap_or(0),
        ))
    };
    let (day_checks, day_up) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(ok), 0) FROM uptime_checks WHERE route = ?1 AND at >= ?2",
        params![key, now - DAY_MS],
        counts,
    )?;
    let hourly = |days: i64| -> rusqlite::Result<(u64, u64)> {
        conn.query_row(
            "SELECT COALESCE(SUM(checks), 0), COALESCE(SUM(up), 0) FROM uptime_hourly
             WHERE route = ?1 AND hour >= ?2",
            params![key, (now - days * DAY_MS).div_euclid(HOUR_MS) * HOUR_MS],
            counts,
        )
    };
    let (week_checks, week_up) = hourly(7)?;
    let (month_checks, month_up) = hourly(30)?;
    let mut stmt = conn.prepare_cached(
        "SELECT latency_ms FROM uptime_checks
         WHERE route = ?1 AND at >= ?2 AND ok = 1 AND latency_ms IS NOT NULL",
    )?;
    #[allow(clippy::cast_precision_loss)]
    let mut latencies: Vec<f64> = stmt
        .query_map(params![key, now - DAY_MS], |row| row.get::<_, i64>(0))?
        .map(|r| r.map(|v| v as f64))
        .collect::<Result<Vec<_>, _>>()?;
    let open_incident = conn
        .query_row(
            &format!(
                "SELECT {INCIDENT_COLUMNS} FROM incidents WHERE route = ?1 AND ended_at IS NULL"
            ),
            params![key],
            incident_row,
        )
        .optional()?;
    Ok(UptimeSummary {
        account_id: target.account_id.clone(),
        route: target.route.clone(),
        up: last.as_ref().map(|l| l.1),
        last_checked: last.as_ref().map(|l| l.0),
        last_latency_ms: last.as_ref().and_then(|l| l.2),
        last_cause: last
            .as_ref()
            .and_then(|l| l.3.as_deref())
            .and_then(Cause::parse),
        uptime_day: share(day_checks, day_up),
        uptime_week: share(week_checks, week_up),
        uptime_month: share(month_checks, month_up),
        p95_ms: quantile(&mut latencies, 0.95),
        open_incident,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(host: &str) -> Target {
        Target {
            account_id: "acc".into(),
            tunnel_id: "t".into(),
            route: RouteRef {
                hostname: host.into(),
                path: None,
            },
        }
    }

    fn check(ok: bool, latency: u32) -> CheckOutcome {
        CheckOutcome {
            ok,
            status: Some(if ok { 200 } else { 502 }),
            latency_ms: Some(latency),
            cause: (!ok).then_some(Cause::OriginUnreachable),
        }
    }

    #[tokio::test]
    async fn keeps_checks_hours_and_incidents() {
        let uptime = UptimeStore::new(Store::open_in_memory().unwrap());
        let t = target("a.x.com");
        let now = 100 * DAY_MS;
        for i in 0..10 {
            let at = now - (10 - i) * 60_000;
            uptime
                .record(
                    &t.key(),
                    at,
                    check(i != 3 && i != 4, 100 + u32::try_from(i).unwrap()),
                )
                .await
                .unwrap();
        }
        uptime
            .open_incident(&t, now - 7 * 60_000, Cause::OriginUnreachable)
            .await
            .unwrap();
        // Opening twice keeps one.
        uptime
            .open_incident(&t, now - 6 * 60_000, Cause::OriginUnreachable)
            .await
            .unwrap();
        assert_eq!(uptime.open_incidents().await.unwrap().len(), 1);

        let summary = &uptime
            .summaries(std::slice::from_ref(&t), now)
            .await
            .unwrap()[0];
        assert_eq!(summary.uptime_day, Some(0.8));
        assert_eq!(summary.uptime_week, Some(0.8));
        assert_eq!(summary.up, Some(true));
        assert_eq!(summary.p95_ms, Some(109.0));
        assert!(summary.open_incident.is_some());

        let closed = uptime.close_incident(&t.key(), now).await.unwrap().unwrap();
        assert_eq!(closed.ended_at, Some(now));
        assert!(
            uptime
                .close_incident(&t.key(), now)
                .await
                .unwrap()
                .is_none()
        );

        let detail = uptime.detail(&t, AnalyticsRange::Day, now).await.unwrap();
        assert_eq!(detail.bars.len(), BARS);
        assert_eq!(detail.bars.iter().map(|b| b.checks).sum::<u32>(), 10);
        assert_eq!(detail.latency.at.len(), 10);
        assert_eq!(detail.latency.ms.iter().filter(|v| v.is_none()).count(), 2);
        assert_eq!(detail.incidents.len(), 1);
        let month = uptime.detail(&t, AnalyticsRange::Month, now).await.unwrap();
        assert_eq!(month.latency.at.len(), 1, "hourly means beyond two days");

        // Three days later the raw checks are gone, the hours stay.
        uptime.prune(now + 3 * DAY_MS).await.unwrap();
        let later = &uptime.summaries(&[t], now + 3 * DAY_MS).await.unwrap()[0];
        assert_eq!(later.uptime_day, None);
        assert_eq!(later.uptime_week, Some(0.8));
        assert_eq!(later.up, None);
    }

    #[tokio::test]
    async fn one_runner_at_a_time() {
        let uptime = UptimeStore::new(Store::open_in_memory().unwrap());
        assert!(uptime.claim("app", 0, 1_000).await.unwrap());
        assert!(!uptime.claim("cli-1", 500, 1_000).await.unwrap());
        assert!(uptime.claim("app", 900, 1_000).await.unwrap(), "renewed");
        assert!(
            uptime.claim("cli-1", 2_000, 1_000).await.unwrap(),
            "expired"
        );
        uptime.release("cli-1").await.unwrap();
        assert!(uptime.claim("app", 2_100, 1_000).await.unwrap(), "released");
    }
}
