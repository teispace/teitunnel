//! What the engine remembers locally: this Mac's tunnel per account, which DNS records
//! Teitunnel created (the ownership index, a backup for the record comment), and the
//! activity log.

use std::{
    collections::HashSet,
    time::{SystemTime, UNIX_EPOCH},
};

use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use crate::store::{Store, StoreError};

/// This Mac's tunnel in an account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalTunnel {
    /// Tunnel id.
    pub tunnel_id: String,
    /// Name it was created with.
    pub name: String,
    /// The config version Teitunnel last wrote (drift detection).
    pub last_applied_version: Option<u64>,
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
    /// Step descriptions and any error, as shown in the inspector.
    pub detail: Vec<String>,
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

    /// This Mac's tunnel in `account`, if one was created.
    ///
    /// # Errors
    /// Database errors.
    pub async fn machine_tunnel(&self, account: &str) -> Result<Option<LocalTunnel>, StoreError> {
        let account = account.to_owned();
        self.store
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT tunnel_id, name, last_applied_version FROM tunnels_local
                         WHERE account_id = ?1",
                        params![account],
                        |row| {
                            Ok(LocalTunnel {
                                tunnel_id: row.get(0)?,
                                name: row.get(1)?,
                                last_applied_version: row
                                    .get::<_, Option<i64>>(2)?
                                    .and_then(|v| u64::try_from(v).ok()),
                            })
                        },
                    )
                    .optional()?)
            })
            .await
    }

    /// Remembers the tunnel created for this Mac.
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
                conn.execute(
                    "INSERT INTO tunnels_local (account_id, tunnel_id, name, created_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (account_id) DO UPDATE SET tunnel_id = ?2, name = ?3,
                       last_applied_version = NULL, created_at = ?4",
                    params![account, tunnel_id, name, now_ms()],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets this Mac's tunnel in `account` (after it was deleted).
    ///
    /// # Errors
    /// Database errors.
    pub async fn forget_machine_tunnel(&self, account: &str) -> Result<(), StoreError> {
        let account = account.to_owned();
        self.store
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM tunnels_local WHERE account_id = ?1",
                    params![account],
                )?;
                Ok(())
            })
            .await
    }

    /// Records the config version Teitunnel just wrote.
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_applied_version(&self, account: &str, version: u64) -> Result<(), StoreError> {
        let account = account.to_owned();
        let version = i64::try_from(version).unwrap_or(i64::MAX);
        self.store
            .call(move |conn| {
                conn.execute(
                    "UPDATE tunnels_local SET last_applied_version = ?2 WHERE account_id = ?1",
                    params![account, version],
                )?;
                Ok(())
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
    ) -> Result<(), StoreError> {
        let (account, summary, outcome) =
            (account.to_owned(), summary.to_owned(), outcome.to_owned());
        let detail = serde_json::to_string(detail)?;
        self.store
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO activity (account_id, at, summary, outcome, detail)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![account, now_ms(), summary, outcome, detail],
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
                    "SELECT id, at, summary, outcome, detail FROM activity
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
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                rows.into_iter()
                    .map(|(id, at, summary, outcome, detail)| {
                        Ok(ActivityEntry {
                            id,
                            at,
                            summary,
                            outcome,
                            detail: serde_json::from_str(&detail)?,
                        })
                    })
                    .collect()
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trips() {
        let local = Local::new(Store::open_in_memory().unwrap());
        assert_eq!(local.machine_tunnel("a").await.unwrap(), None);
        local.set_machine_tunnel("a", "t1", "Mac").await.unwrap();
        local.set_applied_version("a", 4).await.unwrap();
        assert_eq!(
            local.machine_tunnel("a").await.unwrap(),
            Some(LocalTunnel {
                tunnel_id: "t1".into(),
                name: "Mac".into(),
                last_applied_version: Some(4)
            })
        );
        // Re-creating resets the applied version.
        local.set_machine_tunnel("a", "t2", "Mac").await.unwrap();
        let tunnel = local.machine_tunnel("a").await.unwrap().unwrap();
        assert_eq!(
            (tunnel.tunnel_id.as_str(), tunnel.last_applied_version),
            ("t2", None)
        );

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
            .log("a", "first", "applied", &["x".into()])
            .await
            .unwrap();
        local.log("a", "second", "rolledBack", &[]).await.unwrap();
        let log = local.activity("a", 10).await.unwrap();
        assert_eq!(
            log.iter().map(|e| e.summary.as_str()).collect::<Vec<_>>(),
            ["second", "first"]
        );
        assert_eq!(log[1].detail, ["x"]);
    }
}
