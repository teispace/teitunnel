//! What the engine remembers locally: this Mac's tunnels per account, which DNS records
//! Teitunnel created (the ownership index, a backup for the record comment), the
//! activity log, and per-minute connector traffic.

use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

use cf_api::IngressRule;
use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::activity::ActivityRecord;
use crate::{
    store::{Store, StoreError},
    traffic::{MinuteRollup, RETENTION},
};

/// One of this Mac's tunnels in an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTunnel {
    /// Tunnel id.
    pub tunnel_id: String,
    /// Name it was created with.
    pub name: String,
    /// The machine tunnel: where routes go unless another tunnel is chosen.
    pub is_default: bool,
    /// The config version Teitunnel last wrote (drift detection).
    pub last_applied_version: Option<u64>,
    /// The connector's metrics port, kept stable across restarts.
    pub metrics_port: Option<u16>,
    /// Whether the connector runs as an OS service (survives app quit and reboot).
    pub always_on: bool,
}

/// One entry of the activity log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ActivityEntry {
    /// Row id.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub id: i64,
    /// Milliseconds since the epoch.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub at: i64,
    /// What was asked, e.g. "Add app.xyz.com → http://localhost:3000".
    pub summary: String,
    /// `applied`, `rolledBack` or `partiallyApplied`.
    pub outcome: String,
    /// Step descriptions and any error, as plain lines (search, and entries from
    /// before the structured record).
    pub detail: Vec<String>,
    /// Kind, hostnames, step states and before/after (absent in older entries).
    pub record: Option<ActivityRecord>,
}

const TUNNEL_COLUMNS: &str =
    "tunnel_id, name, is_default, last_applied_version, metrics_port, run_mode";

fn tunnel_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LocalTunnel> {
    Ok(LocalTunnel {
        tunnel_id: row.get(0)?,
        name: row.get(1)?,
        is_default: row.get::<_, i64>(2)? == 1,
        last_applied_version: row
            .get::<_, Option<i64>>(3)?
            .and_then(|v| u64::try_from(v).ok()),
        metrics_port: row
            .get::<_, Option<i64>>(4)?
            .and_then(|v| u16::try_from(v).ok()),
        always_on: row.get::<_, String>(5)? == "alwaysOn",
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// Typed access to the engine's tables.
#[derive(Debug, Clone)]
pub struct Local {
    store: Store,
}

impl Local {
    /// Wraps the store.
    pub fn new(store: Store) -> Self {
        Self { store }
    }

    /// This Mac's default tunnel in `account` (the machine tunnel), if one was created.
    ///
    /// # Errors
    /// Database errors.
    pub async fn machine_tunnel(&self, account: &str) -> Result<Option<LocalTunnel>, StoreError> {
        self.tunnel(account, None).await
    }

    /// A tunnel of this Mac in `account`: `id`, or the default one when `None`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn tunnel(
        &self,
        account: &str,
        id: Option<&str>,
    ) -> Result<Option<LocalTunnel>, StoreError> {
        let (account, id) = (account.to_owned(), id.map(str::to_owned));
        self.store
            .call(move |conn| {
                let sql = format!(
                    "SELECT {TUNNEL_COLUMNS} FROM local_tunnels WHERE account_id = ?1 AND {}",
                    if id.is_some() {
                        "tunnel_id = ?2"
                    } else {
                        "is_default = 1"
                    }
                );
                let mut stmt = conn.prepare(&sql)?;
                let row = match &id {
                    Some(id) => stmt.query_row(params![account, id], tunnel_row),
                    None => stmt.query_row(params![account], tunnel_row),
                };
                Ok(row.optional()?)
            })
            .await
    }

    /// Every tunnel of this Mac in `account`, the default first, then by name.
    ///
    /// # Errors
    /// Database errors.
    pub async fn tunnels(&self, account: &str) -> Result<Vec<LocalTunnel>, StoreError> {
        let account = account.to_owned();
        self.store
            .call(move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {TUNNEL_COLUMNS} FROM local_tunnels WHERE account_id = ?1
                     ORDER BY is_default DESC, name COLLATE NOCASE"
                ))?;
                let rows = stmt.query_map(params![account], tunnel_row)?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await
    }

    /// Which of this Mac's tunnels in `account` Teitunnel last configured to route
    /// `hostname` (`None`: none of them does).
    ///
    /// # Errors
    /// Database errors.
    pub async fn tunnel_routing(
        &self,
        account: &str,
        hostname: &str,
    ) -> Result<Option<String>, StoreError> {
        for tunnel in self.tunnels(account).await? {
            let ingress = self.applied_ingress(&tunnel.tunnel_id).await?;
            if ingress
                .unwrap_or_default()
                .iter()
                .any(|r| r.hostname.as_deref() == Some(hostname))
            {
                return Ok(Some(tunnel.tunnel_id));
            }
        }
        Ok(None)
    }

    /// Remembers the default tunnel created for this Mac, replacing the previous one.
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_machine_tunnel(
        &self,
        account: &str,
        tunnel_id: &str,
        name: &str,
    ) -> Result<(), StoreError> {
        let (account, tunnel_id, name) =
            (account.to_owned(), tunnel_id.to_owned(), name.to_owned());
        self.store
            .call(move |conn| {
                let tx = conn.transaction()?;
                tx.execute(
                    "DELETE FROM local_tunnels WHERE account_id = ?1 AND is_default = 1",
                    params![account],
                )?;
                tx.execute(
                    "INSERT INTO local_tunnels (tunnel_id, account_id, name, is_default, created_at)
                     VALUES (?1, ?2, ?3, 1, ?4)",
                    params![tunnel_id, account, name, now_ms()],
                )?;
                tx.commit()?;
                Ok(())
            })
            .await
    }

    /// Remembers another tunnel created for this Mac (not the default).
    ///
    /// # Errors
    /// Database errors.
    pub async fn add_tunnel(
        &self,
        account: &str,
        tunnel_id: &str,
        name: &str,
    ) -> Result<(), StoreError> {
        let (account, tunnel_id, name) =
            (account.to_owned(), tunnel_id.to_owned(), name.to_owned());
        self.store
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO local_tunnels (tunnel_id, account_id, name, is_default, created_at)
                     VALUES (?1, ?2, ?3, 0, ?4)",
                    params![tunnel_id, account, name, now_ms()],
                )?;
                Ok(())
            })
            .await
    }

    /// Points a tunnel's row at a new tunnel id (it was deleted elsewhere and recreated),
    /// keeping its name, default flag and run mode.
    ///
    /// # Errors
    /// Database errors.
    pub async fn replace_tunnel(&self, old_id: &str, new_id: &str) -> Result<(), StoreError> {
        let (old_id, new_id) = (old_id.to_owned(), new_id.to_owned());
        self.store
            .call(move |conn| {
                conn.execute(
                    "UPDATE local_tunnels SET tunnel_id = ?2, last_applied_version = NULL,
                       last_applied_ingress = NULL, created_at = ?3 WHERE tunnel_id = ?1",
                    params![old_id, new_id, now_ms()],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets one of this Mac's tunnels (after it was deleted).
    ///
    /// # Errors
    /// Database errors.
    pub async fn forget_tunnel(&self, tunnel_id: &str) -> Result<(), StoreError> {
        let tunnel_id = tunnel_id.to_owned();
        self.store
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM local_tunnels WHERE tunnel_id = ?1",
                    params![tunnel_id],
                )?;
                Ok(())
            })
            .await
    }

    /// Records the config version Teitunnel just wrote to a tunnel, and its ingress (to
    /// show what changed if someone edits it elsewhere).
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_applied(
        &self,
        tunnel_id: &str,
        version: u64,
        ingress: &[IngressRule],
    ) -> Result<(), StoreError> {
        let tunnel_id = tunnel_id.to_owned();
        let version = i64::try_from(version).unwrap_or(i64::MAX);
        let ingress = serde_json::to_string(ingress)?;
        self.store
            .call(move |conn| {
                conn.execute(
                    "UPDATE local_tunnels SET last_applied_version = ?2, last_applied_ingress = ?3
                     WHERE tunnel_id = ?1",
                    params![tunnel_id, version, ingress],
                )?;
                Ok(())
            })
            .await
    }

    /// The ingress Teitunnel last wrote to a tunnel.
    ///
    /// # Errors
    /// Database errors.
    pub async fn applied_ingress(
        &self,
        tunnel_id: &str,
    ) -> Result<Option<Vec<IngressRule>>, StoreError> {
        let tunnel_id = tunnel_id.to_owned();
        self.store
            .call(move |conn| {
                let json: Option<String> = conn
                    .query_row(
                        "SELECT last_applied_ingress FROM local_tunnels WHERE tunnel_id = ?1",
                        params![tunnel_id],
                        |row| row.get(0),
                    )
                    .optional()?
                    .flatten();
                Ok(json.map(|j| serde_json::from_str(&j)).transpose()?)
            })
            .await
    }

    /// Remembers a tunnel's connector metrics port.
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_metrics_port(&self, tunnel_id: &str, port: u16) -> Result<(), StoreError> {
        let tunnel_id = tunnel_id.to_owned();
        self.store
            .call(move |conn| {
                conn.execute(
                    "UPDATE local_tunnels SET metrics_port = ?2 WHERE tunnel_id = ?1",
                    params![tunnel_id, port],
                )?;
                Ok(())
            })
            .await
    }

    /// Records whether a tunnel's connector runs as an OS service.
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_always_on(&self, tunnel_id: &str, always_on: bool) -> Result<(), StoreError> {
        let tunnel_id = tunnel_id.to_owned();
        let mode = if always_on { "alwaysOn" } else { "session" };
        self.store
            .call(move |conn| {
                conn.execute(
                    "UPDATE local_tunnels SET run_mode = ?2 WHERE tunnel_id = ?1",
                    params![tunnel_id, mode],
                )?;
                Ok(())
            })
            .await
    }

    /// Remembers a temporary route ("share on your domain").
    ///
    /// # Errors
    /// Database errors.
    pub async fn record_share(
        &self,
        share: &crate::domain_shares::DomainShare,
    ) -> Result<(), StoreError> {
        let share = share.clone();
        self.store
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO domain_shares (account_id, hostname, origin, owner, expires_at, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT (account_id, hostname) DO UPDATE SET origin = ?3, owner = ?4,
                       expires_at = ?5, created_at = ?6",
                    params![
                        share.account_id,
                        share.hostname,
                        share.origin,
                        share.owner,
                        share.expires_at.and_then(|t| i64::try_from(t).ok()),
                        i64::try_from(share.created_at).unwrap_or(i64::MAX),
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets a temporary route (it was removed).
    ///
    /// # Errors
    /// Database errors.
    pub async fn forget_share(&self, account: &str, hostname: &str) -> Result<(), StoreError> {
        let (account, hostname) = (account.to_owned(), hostname.to_owned());
        self.store
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM domain_shares WHERE account_id = ?1 AND hostname = ?2",
                    params![account, hostname],
                )?;
                Ok(())
            })
            .await
    }

    /// Temporary routes, in `account` or everywhere (`None`), oldest first.
    ///
    /// # Errors
    /// Database errors.
    pub async fn shares(
        &self,
        account: Option<&str>,
    ) -> Result<Vec<crate::domain_shares::DomainShare>, StoreError> {
        let account = account.map(str::to_owned);
        self.store
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT account_id, hostname, origin, owner, expires_at, created_at
                     FROM domain_shares WHERE ?1 IS NULL OR account_id = ?1
                     ORDER BY created_at, hostname",
                )?;
                let rows = stmt.query_map(params![account], |row| {
                    Ok(crate::domain_shares::DomainShare {
                        account_id: row.get(0)?,
                        hostname: row.get(1)?,
                        origin: row.get(2)?,
                        owner: row.get(3)?,
                        expires_at: row
                            .get::<_, Option<i64>>(4)?
                            .and_then(|t| u64::try_from(t).ok()),
                        created_at: u64::try_from(row.get::<_, i64>(5)?).unwrap_or_default(),
                    })
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await
    }

    /// Ids of DNS records Teitunnel created in `account`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn owned_records(&self, account: &str) -> Result<HashSet<String>, StoreError> {
        let account = account.to_owned();
        self.store
            .call(move |conn| {
                let mut stmt =
                    conn.prepare("SELECT record_id FROM dns_ownership WHERE account_id = ?1")?;
                let ids = stmt
                    .query_map(params![account], |row| row.get(0))?
                    .collect::<Result<HashSet<String>, _>>()?;
                Ok(ids)
            })
            .await
    }

    /// Marks a record as created by Teitunnel.
    ///
    /// # Errors
    /// Database errors.
    pub async fn own_record(
        &self,
        account: &str,
        zone_id: &str,
        record_id: &str,
        hostname: &str,
        route_id: &str,
    ) -> Result<(), StoreError> {
        let values = [account, zone_id, record_id, hostname, route_id].map(str::to_owned);
        self.store
            .call(move |conn| {
                let [account, zone, record, hostname, route] = values;
                conn.execute(
                    "INSERT OR REPLACE INTO dns_ownership
                       (record_id, account_id, zone_id, hostname, route_id, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![record, account, zone, hostname, route, now_ms()],
                )?;
                Ok(())
            })
            .await
    }

    /// Removes a record from the ownership index.
    ///
    /// # Errors
    /// Database errors.
    pub async fn disown_record(&self, record_id: &str) -> Result<(), StoreError> {
        let record_id = record_id.to_owned();
        self.store
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM dns_ownership WHERE record_id = ?1",
                    params![record_id],
                )?;
                Ok(())
            })
            .await
    }

    /// Appends to the activity log.
    ///
    /// # Errors
    /// Database errors.
    pub async fn log(
        &self,
        account: &str,
        summary: &str,
        outcome: &str,
        detail: &[String],
        record: Option<&ActivityRecord>,
    ) -> Result<(), StoreError> {
        let (account, summary, outcome) =
            (account.to_owned(), summary.to_owned(), outcome.to_owned());
        let detail = serde_json::to_string(detail)?;
        let record = record.map(serde_json::to_string).transpose()?;
        self.store
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO activity (account_id, at, summary, outcome, detail, record)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![account, now_ms(), summary, outcome, detail, record],
                )?;
                Ok(())
            })
            .await
    }

    /// The most recent activity in `account`, newest first.
    ///
    /// # Errors
    /// Database errors.
    pub async fn activity(
        &self,
        account: &str,
        limit: u32,
    ) -> Result<Vec<ActivityEntry>, StoreError> {
        let account = account.to_owned();
        self.store
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, at, summary, outcome, detail, record FROM activity
                     WHERE account_id = ?1 ORDER BY at DESC, id DESC LIMIT ?2",
                )?;
                let rows = stmt
                    .query_map(params![account, limit], |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, String>(3)?,
                            row.get::<_, String>(4)?,
                            row.get::<_, Option<String>>(5)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows.into_iter()
                    .map(|(id, at, summary, outcome, detail, record)| {
                        Ok(ActivityEntry {
                            id,
                            at,
                            summary,
                            outcome,
                            detail: serde_json::from_str(&detail)?,
                            // A record this version can't read (written by a newer one)
                            // falls back to the plain lines rather than failing the list.
                            record: record.and_then(|r| serde_json::from_str(&r).ok()),
                        })
                    })
                    .collect()
            })
            .await
    }
}

impl Local {
    /// Access applications Teitunnel created in `account`: `(app id, domain)`.
    ///
    /// # Errors
    /// Database errors.
    pub async fn owned_access_apps(
        &self,
        account: &str,
    ) -> Result<Vec<(String, String)>, StoreError> {
        let account = account.to_owned();
        self.store
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT app_id, domain FROM access_ownership WHERE account_id = ?1 ORDER BY domain",
                )?;
                let rows = stmt
                    .query_map(params![account], |row| Ok((row.get(0)?, row.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    /// Records that Teitunnel created an Access application.
    ///
    /// # Errors
    /// Database errors.
    pub async fn own_access_app(
        &self,
        account: &str,
        app_id: &str,
        domain: &str,
    ) -> Result<(), StoreError> {
        let (account, app_id, domain) = (account.to_owned(), app_id.to_owned(), domain.to_owned());
        self.store
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO access_ownership (app_id, account_id, domain, created_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (app_id) DO UPDATE SET domain = excluded.domain",
                    params![app_id, account, domain, now_ms()],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets an Access application (deleted).
    ///
    /// # Errors
    /// Database errors.
    pub async fn disown_access_app(&self, app_id: &str) -> Result<(), StoreError> {
        let app_id = app_id.to_owned();
        self.store
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM access_ownership WHERE app_id = ?1",
                    params![app_id],
                )?;
                Ok(())
            })
            .await
    }
}

/// Rollups for the same minute (a connector restarted mid-minute) add up.
const UPSERT_ROLLUP: &str = "INSERT INTO metrics_rollup (tunnel_id, minute, requests, errors,
        status_2xx, status_3xx, status_4xx, status_5xx, concurrent_max, connections_min,
        rtt_sum_ms, rtt_samples)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
     ON CONFLICT (tunnel_id, minute) DO UPDATE SET
        requests = requests + excluded.requests,
        errors = errors + excluded.errors,
        status_2xx = status_2xx + excluded.status_2xx,
        status_3xx = status_3xx + excluded.status_3xx,
        status_4xx = status_4xx + excluded.status_4xx,
        status_5xx = status_5xx + excluded.status_5xx,
        concurrent_max = max(concurrent_max, excluded.concurrent_max),
        connections_min = min(connections_min, excluded.connections_min),
        rtt_sum_ms = rtt_sum_ms + excluded.rtt_sum_ms,
        rtt_samples = rtt_samples + excluded.rtt_samples";

#[allow(clippy::cast_possible_wrap)]
fn stored(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn loaded(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

impl Local {
    /// Saves finished minutes of traffic and drops those older than the retention.
    ///
    /// # Errors
    /// Database errors.
    pub async fn save_rollups(&self, rollups: Vec<MinuteRollup>) -> Result<(), StoreError> {
        if rollups.is_empty() {
            return Ok(());
        }
        let oldest = now_ms() / 60_000 - i64::try_from(RETENTION.as_secs() / 60).unwrap_or(0);
        self.store
            .call(move |conn| {
                let tx = conn.transaction()?;
                {
                    let mut insert = tx.prepare_cached(UPSERT_ROLLUP)?;
                    for r in &rollups {
                        let [s2, s3, s4, s5] = r.classes.map(stored);
                        insert.execute(params![
                            r.tunnel,
                            r.minute,
                            stored(r.requests),
                            stored(r.errors),
                            s2,
                            s3,
                            s4,
                            s5,
                            r.concurrent_max,
                            r.connections_min,
                            r.rtt_sum_ms,
                            r.rtt_samples,
                        ])?;
                    }
                    tx.execute(
                        "DELETE FROM metrics_rollup WHERE minute < ?1",
                        params![oldest],
                    )?;
                }
                tx.commit()?;
                Ok(())
            })
            .await
    }

    /// A tunnel's rollups from `since_minute` on, oldest first.
    ///
    /// # Errors
    /// Database errors.
    pub async fn rollups(
        &self,
        tunnel_id: &str,
        since_minute: i64,
    ) -> Result<Vec<MinuteRollup>, StoreError> {
        let tunnel = tunnel_id.to_owned();
        self.store
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT minute, requests, errors, status_2xx, status_3xx, status_4xx,
                            status_5xx, concurrent_max, connections_min, rtt_sum_ms, rtt_samples
                     FROM metrics_rollup WHERE tunnel_id = ?1 AND minute >= ?2 ORDER BY minute",
                )?;
                let rows = stmt
                    .query_map(params![tunnel, since_minute], |row| {
                        Ok(MinuteRollup {
                            tunnel: tunnel.clone(),
                            minute: row.get(0)?,
                            requests: loaded(row.get(1)?),
                            errors: loaded(row.get(2)?),
                            classes: [
                                loaded(row.get(3)?),
                                loaded(row.get(4)?),
                                loaded(row.get(5)?),
                                loaded(row.get(6)?),
                            ],
                            concurrent_max: row.get(7)?,
                            connections_min: row.get(8)?,
                            rtt_sum_ms: row.get(9)?,
                            rtt_samples: row.get(10)?,
                        })
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows)
            })
            .await
    }

    /// Deletes a tunnel's traffic history (the tunnel was deleted).
    ///
    /// # Errors
    /// Database errors.
    pub async fn forget_rollups(&self, tunnel_id: &str) -> Result<(), StoreError> {
        let tunnel = tunnel_id.to_owned();
        self.store
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM metrics_rollup WHERE tunnel_id = ?1",
                    params![tunnel],
                )?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn remembers_access_apps_it_created() {
        let local = Local::new(Store::open_in_memory().unwrap());
        local
            .own_access_app("a", "app1", "app.xyz.com")
            .await
            .unwrap();
        local
            .own_access_app("a", "app1", "web.xyz.com")
            .await
            .unwrap();
        local.own_access_app("b", "app2", "yx.com").await.unwrap();
        assert_eq!(
            local.owned_access_apps("a").await.unwrap(),
            [("app1".to_owned(), "web.xyz.com".to_owned())]
        );
        local.disown_access_app("app1").await.unwrap();
        assert!(local.owned_access_apps("a").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn rollups_merge_expire_and_forget() {
        let local = Local::new(Store::open_in_memory().unwrap());
        let now = now_ms() / 60_000;
        let rollup = |minute, requests, connections_min| MinuteRollup {
            tunnel: "t".into(),
            minute,
            requests,
            errors: 1,
            classes: [requests, 0, 0, 1],
            concurrent_max: 2,
            connections_min,
            rtt_sum_ms: 30.0,
            rtt_samples: 2,
        };
        local
            .save_rollups(vec![
                rollup(now - 1, 10, 4),
                rollup(now - 8 * 24 * 60, 5, 4),
            ])
            .await
            .unwrap();
        // Same minute again (restart): counts add, the gauges keep their extremes.
        local
            .save_rollups(vec![rollup(now - 1, 3, 2)])
            .await
            .unwrap();
        let rows = local.rollups("t", 0).await.unwrap();
        assert_eq!(rows.len(), 1, "older than 7 days is dropped");
        assert_eq!(
            (
                rows[0].requests,
                rows[0].errors,
                rows[0].connections_min,
                rows[0].rtt_samples
            ),
            (13, 2, 2, 4)
        );
        assert_eq!(rows[0].classes, [13, 0, 0, 2]);
        assert!(local.rollups("t", now).await.unwrap().is_empty());
        local.forget_rollups("t").await.unwrap();
        assert!(local.rollups("t", 0).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn round_trips() {
        let local = Local::new(Store::open_in_memory().unwrap());
        assert_eq!(local.machine_tunnel("a").await.unwrap(), None);
        local.set_machine_tunnel("a", "t1", "Mac").await.unwrap();
        local.set_applied("t1", 4, &[]).await.unwrap();
        assert_eq!(local.applied_ingress("t1").await.unwrap(), Some(Vec::new()));
        local.set_metrics_port("t1", 20300).await.unwrap();
        assert_eq!(
            local.machine_tunnel("a").await.unwrap(),
            Some(LocalTunnel {
                tunnel_id: "t1".into(),
                name: "Mac".into(),
                is_default: true,
                last_applied_version: Some(4),
                metrics_port: Some(20300),
                always_on: false,
            })
        );
        // Re-creating replaces the default and resets the applied version.
        local.set_machine_tunnel("a", "t2", "Mac").await.unwrap();
        let tunnel = local.machine_tunnel("a").await.unwrap().unwrap();
        assert_eq!(
            (tunnel.tunnel_id.as_str(), tunnel.last_applied_version),
            ("t2", None)
        );

        // More tunnels: listed after the default, found by id, not by other accounts.
        local.add_tunnel("a", "t3", "staging").await.unwrap();
        local.add_tunnel("b", "t4", "other").await.unwrap();
        local.set_always_on("t3", true).await.unwrap();
        let ids: Vec<String> = local
            .tunnels("a")
            .await
            .unwrap()
            .into_iter()
            .map(|t| t.tunnel_id)
            .collect();
        assert_eq!(ids, ["t2", "t3"]);
        let staging = local.tunnel("a", Some("t3")).await.unwrap().unwrap();
        assert!(!staging.is_default && staging.always_on);
        assert_eq!(local.tunnel("a", Some("t4")).await.unwrap(), None);
        local.replace_tunnel("t3", "t5").await.unwrap();
        let staging = local.tunnel("a", Some("t5")).await.unwrap().unwrap();
        assert_eq!(
            (staging.name.as_str(), staging.always_on),
            ("staging", true)
        );
        local.forget_tunnel("t5").await.unwrap();
        assert_eq!(local.tunnels("a").await.unwrap().len(), 1);

        local
            .own_record("a", "z", "r1", "app.xyz.com", "route")
            .await
            .unwrap();
        local
            .own_record("b", "z", "r2", "app.yx.com", "route")
            .await
            .unwrap();
        assert_eq!(
            local.owned_records("a").await.unwrap(),
            HashSet::from(["r1".to_owned()])
        );
        local.disown_record("r1").await.unwrap();
        assert!(local.owned_records("a").await.unwrap().is_empty());

        local
            .log("a", "first", "applied", &["x".into()], None)
            .await
            .unwrap();
        local
            .log("a", "second", "rolledBack", &[], None)
            .await
            .unwrap();
        let log = local.activity("a", 10).await.unwrap();
        assert_eq!(
            log.iter().map(|e| e.summary.as_str()).collect::<Vec<_>>(),
            ["second", "first"]
        );
        assert_eq!(log[1].detail, ["x"]);
    }
}
