//! What Teitunnel remembers about its Workers in front of routes and its D1 database
//! (migrations 18 and 19): the ownership index, with the settings each Worker was
//! deployed with. Secrets are never stored here.

use rusqlite::{OptionalExtension, params};

use super::{
    front::{FrontConfig, FrontKind, FrontRow},
    local::Local,
};
use crate::store::StoreError;

fn now_ms() -> i64 {
    i64::try_from(crate::domain_shares::now_ms()).unwrap_or(i64::MAX)
}

fn front_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Option<(String, FrontRow)>> {
    let config: String = row.get(3)?;
    let Ok(config) = serde_json::from_str::<FrontConfig>(&config) else {
        return Ok(None);
    };
    Ok(Some((
        row.get(0)?,
        FrontRow {
            hostname: row.get(1)?,
            config,
            script: row.get(2)?,
            zone_id: row.get(4)?,
            route_id: row.get(5)?,
        },
    )))
}

impl Local {
    /// Teitunnel's front Workers in `account` (on `hostname`, or everywhere), with the
    /// account of each.
    ///
    /// # Errors
    /// Database errors.
    pub async fn fronts(
        &self,
        account: Option<&str>,
        hostname: Option<&str>,
    ) -> Result<Vec<(String, FrontRow)>, StoreError> {
        let (account, hostname) = (
            account.map(str::to_owned),
            hostname.map(str::to_ascii_lowercase),
        );
        self.store()
            .call(move |conn| {
                let mut stmt = conn.prepare_cached(
                    "SELECT account_id, hostname, script, config, zone_id, route_id
                     FROM front_workers
                     WHERE (?1 IS NULL OR account_id = ?1) AND (?2 IS NULL OR hostname = ?2)
                     ORDER BY hostname, kind, path",
                )?;
                let rows = stmt
                    .query_map(params![account, hostname], front_row)?
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(rows.into_iter().flatten().collect())
            })
            .await
    }

    /// Records a front Worker Teitunnel deployed (replacing what it had).
    ///
    /// # Errors
    /// Database errors.
    pub async fn save_front(
        &self,
        account: &str,
        hostname: &str,
        zone_id: &str,
        script: &str,
        config: &FrontConfig,
    ) -> Result<(), StoreError> {
        let (account, hostname, zone_id, script) = (
            account.to_owned(),
            hostname.to_ascii_lowercase(),
            zone_id.to_owned(),
            script.to_owned(),
        );
        let (kind, path) = (config.kind().as_str(), config.path().to_owned());
        let config = serde_json::to_string(config).unwrap_or_default();
        self.store()
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO front_workers
                        (account_id, hostname, kind, path, script, zone_id, config, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT (account_id, hostname, kind, path)
                     DO UPDATE SET script = ?5, zone_id = ?6, config = ?7",
                    params![
                        account,
                        hostname,
                        kind,
                        path,
                        script,
                        zone_id,
                        config,
                        now_ms()
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Records (or clears) a front Worker's route.
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_front_route(
        &self,
        account: &str,
        hostname: &str,
        kind: FrontKind,
        path: &str,
        route_id: Option<&str>,
    ) -> Result<(), StoreError> {
        let (account, hostname, path, route_id) = (
            account.to_owned(),
            hostname.to_ascii_lowercase(),
            path.to_owned(),
            route_id.map(str::to_owned),
        );
        self.store()
            .call(move |conn| {
                conn.execute(
                    "UPDATE front_workers SET route_id = ?5
                     WHERE account_id = ?1 AND hostname = ?2 AND kind = ?3 AND path = ?4",
                    params![account, hostname, kind.as_str(), path, route_id],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets a front Worker (it was deleted).
    ///
    /// # Errors
    /// Database errors.
    pub async fn forget_front(
        &self,
        account: &str,
        hostname: &str,
        kind: FrontKind,
        path: &str,
    ) -> Result<(), StoreError> {
        let (account, hostname, path) = (
            account.to_owned(),
            hostname.to_ascii_lowercase(),
            path.to_owned(),
        );
        self.store()
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM front_workers
                     WHERE account_id = ?1 AND hostname = ?2 AND kind = ?3 AND path = ?4",
                    params![account, hostname, kind.as_str(), path],
                )?;
                Ok(())
            })
            .await
    }

    /// Whether a Snapshot's live version takes comments.
    ///
    /// # Errors
    /// Database errors.
    pub async fn site_comments(&self, snapshot: &str) -> Result<bool, StoreError> {
        let snapshot = snapshot.to_owned();
        self.store()
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT comments FROM snapshots WHERE id = ?1",
                        params![snapshot],
                        |row| row.get::<_, i64>(0),
                    )
                    .optional()?
                    .is_some_and(|n| n != 0))
            })
            .await
    }

    /// Records whether a Snapshot's live version takes comments.
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_site_comments(&self, snapshot: &str, on: bool) -> Result<(), StoreError> {
        let snapshot = snapshot.to_owned();
        self.store()
            .call(move |conn| {
                conn.execute(
                    "UPDATE snapshots SET comments = ?2 WHERE id = ?1",
                    params![snapshot, i64::from(on)],
                )?;
                Ok(())
            })
            .await
    }

    /// The D1 database Teitunnel created on `account`, if any.
    ///
    /// # Errors
    /// Database errors.
    pub async fn cloud_database(&self, account: &str) -> Result<Option<String>, StoreError> {
        let account = account.to_owned();
        self.store()
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT database_id FROM cloud_databases WHERE account_id = ?1",
                        params![account],
                        |row| row.get(0),
                    )
                    .optional()?)
            })
            .await
    }

    /// Records the D1 database Teitunnel created (`None` forgets it).
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_cloud_database(
        &self,
        account: &str,
        database: Option<(&str, &str)>,
    ) -> Result<(), StoreError> {
        let account = account.to_owned();
        let database = database.map(|(id, name)| (id.to_owned(), name.to_owned()));
        self.store()
            .call(move |conn| {
                match database {
                    Some((id, name)) => conn.execute(
                        "INSERT INTO cloud_databases (account_id, database_id, name, created_at)
                         VALUES (?1, ?2, ?3, ?4)
                         ON CONFLICT (account_id) DO UPDATE SET database_id = ?2, name = ?3",
                        params![account, id, name, now_ms()],
                    )?,
                    None => conn.execute(
                        "DELETE FROM cloud_databases WHERE account_id = ?1",
                        params![account],
                    )?,
                };
                Ok(())
            })
            .await
    }
}
