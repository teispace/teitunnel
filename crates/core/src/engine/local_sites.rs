//! What Teitunnel remembers about Snapshots: which Workers it created (its ownership
//! index for them), where they answer, and the manifests of their recent versions.
//! Cloudflare keeps the versions themselves.

use rusqlite::{OptionalExtension, params};

use super::{
    access::AccessRule,
    local::Local,
    sites::{SiteContent, SiteFile},
};
use crate::store::StoreError;

/// Versions whose manifests are kept (Cloudflare keeps the last 100 for rollback).
pub const KEPT_VERSIONS: u32 = 10;

/// A Snapshot as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteRow {
    /// Local id.
    pub id: String,
    /// Account id.
    pub account_id: String,
    /// Name.
    pub name: String,
    /// The Worker.
    pub script: String,
    /// Custom hostname (`None`: workers.dev).
    pub hostname: Option<String>,
    /// Where the files come from (JSON of `snapshot::SnapshotSource`).
    pub source: String,
    /// Single-page app fallback.
    pub spa: bool,
    /// Password protected.
    pub password: bool,
    /// Access login.
    pub access: Option<AccessRule>,
    /// When it's deleted by itself (ms since the epoch).
    pub expires_at: Option<u64>,
    /// `app` or the CLI process that made it.
    pub owner: String,
    /// Cloudflare's id of the live version (`None` until the first publish finished).
    pub live_version: Option<String>,
    /// Created (ms).
    pub created_at: u64,
    /// Last published (ms).
    pub updated_at: u64,
}

/// A version as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SiteVersionRow {
    /// 1, 2, 3…
    pub number: u32,
    /// Cloudflare's version id.
    pub version_id: String,
    /// Published (ms).
    pub created_at: u64,
    /// Files.
    pub files: u64,
    /// Bytes.
    pub bytes: u64,
    /// Single-page app fallback.
    pub spa: bool,
    /// Password protected.
    pub password: bool,
}

fn now_ms() -> i64 {
    i64::try_from(crate::domain_shares::now_ms()).unwrap_or(i64::MAX)
}

fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

const SITE_COLUMNS: &str = "id, account_id, name, script, hostname, source, spa, password, access,
     expires_at, owner, live_version, created_at, updated_at";

fn site_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SiteRow> {
    let access: Option<String> = row.get(8)?;
    Ok(SiteRow {
        id: row.get(0)?,
        account_id: row.get(1)?,
        name: row.get(2)?,
        script: row.get(3)?,
        hostname: row.get(4)?,
        source: row.get(5)?,
        spa: row.get::<_, i64>(6)? == 1,
        password: row.get::<_, i64>(7)? == 1,
        access: access.and_then(|a| serde_json::from_str(&a).ok()),
        expires_at: row.get::<_, Option<i64>>(9)?.map(to_u64),
        owner: row.get(10)?,
        live_version: row.get(11)?,
        created_at: to_u64(row.get(12)?),
        updated_at: to_u64(row.get(13)?),
    })
}

impl Local {
    /// Remembers a Snapshot (or updates its settings), before anything is published so
    /// a crash can't leave a Worker behind unnoticed.
    ///
    /// # Errors
    /// Database errors, e.g. a name already taken in the account.
    pub async fn save_site(&self, site: &SiteRow) -> Result<(), StoreError> {
        let site = site.clone();
        self.store()
            .call(move |conn| {
                let access = site
                    .access
                    .as_ref()
                    .map(serde_json::to_string)
                    .transpose()?;
                conn.execute(
                    "INSERT INTO snapshots (id, account_id, name, script, hostname, source, spa,
                       password, access, expires_at, owner, live_version, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT (id) DO UPDATE SET name = ?3, hostname = ?5, source = ?6,
                       spa = ?7, password = ?8, access = ?9, expires_at = ?10, owner = ?11,
                       updated_at = ?14",
                    params![
                        site.id,
                        site.account_id,
                        site.name,
                        site.script,
                        site.hostname,
                        site.source,
                        i64::from(site.spa),
                        i64::from(site.password),
                        access,
                        site.expires_at.and_then(|t| i64::try_from(t).ok()),
                        site.owner,
                        site.live_version,
                        i64::try_from(site.created_at).unwrap_or(i64::MAX),
                        i64::try_from(site.updated_at).unwrap_or(i64::MAX),
                    ],
                )?;
                Ok(())
            })
            .await
    }

    /// Snapshots in `account` (or every account), by name.
    ///
    /// # Errors
    /// Database errors.
    pub async fn sites(&self, account: Option<&str>) -> Result<Vec<SiteRow>, StoreError> {
        let account = account.map(str::to_owned);
        self.store()
            .call(move |conn| {
                let mut stmt = conn.prepare(&format!(
                    "SELECT {SITE_COLUMNS} FROM snapshots
                     WHERE ?1 IS NULL OR account_id = ?1 ORDER BY name COLLATE NOCASE"
                ))?;
                let rows = stmt.query_map(params![account], site_row)?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await
    }

    /// One Snapshot, by id.
    ///
    /// # Errors
    /// Database errors.
    pub async fn site(&self, id: &str) -> Result<Option<SiteRow>, StoreError> {
        let id = id.to_owned();
        self.store()
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        &format!("SELECT {SITE_COLUMNS} FROM snapshots WHERE id = ?1"),
                        params![id],
                        site_row,
                    )
                    .optional()?)
            })
            .await
    }

    /// Forgets a Snapshot and its versions (it was deleted, or never published).
    ///
    /// # Errors
    /// Database errors.
    pub async fn forget_site(&self, id: &str) -> Result<(), StoreError> {
        let id = id.to_owned();
        self.store()
            .call(move |conn| {
                conn.execute("DELETE FROM snapshots WHERE id = ?1", params![id])?;
                Ok(())
            })
            .await
    }

    /// Recent versions, newest first.
    ///
    /// # Errors
    /// Database errors.
    pub async fn site_versions(&self, id: &str) -> Result<Vec<SiteVersionRow>, StoreError> {
        let id = id.to_owned();
        self.store()
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT number, version_id, created_at, files, bytes, spa, password
                     FROM snapshot_versions WHERE snapshot_id = ?1 ORDER BY number DESC",
                )?;
                let rows = stmt.query_map(params![id], |row| {
                    Ok(SiteVersionRow {
                        number: u32::try_from(row.get::<_, i64>(0)?).unwrap_or_default(),
                        version_id: row.get(1)?,
                        created_at: to_u64(row.get(2)?),
                        files: to_u64(row.get(3)?),
                        bytes: to_u64(row.get(4)?),
                        spa: row.get::<_, i64>(5)? == 1,
                        password: row.get::<_, i64>(6)? == 1,
                    })
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await
    }

    /// A version's files and its `_headers` / `_redirects` rules.
    ///
    /// # Errors
    /// Database errors.
    pub async fn site_version_content(
        &self,
        id: &str,
        version_id: &str,
    ) -> Result<Option<SiteContent>, StoreError> {
        let (id, version_id) = (id.to_owned(), version_id.to_owned());
        self.store()
            .call(move |conn| {
                let row: Option<(String, Option<String>, Option<String>)> = conn
                    .query_row(
                        "SELECT manifest, headers, redirects FROM snapshot_versions
                         WHERE snapshot_id = ?1 AND version_id = ?2",
                        params![id, version_id],
                        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                    )
                    .optional()?;
                row.map(|(manifest, headers, redirects)| {
                    Ok(SiteContent {
                        root: None,
                        files: serde_json::from_str::<Vec<SiteFile>>(&manifest)?,
                        headers,
                        redirects,
                    })
                })
                .transpose()
            })
            .await
    }

    /// Records a version that just went live; keeps the newest [`KEPT_VERSIONS`].
    /// Returns its number.
    ///
    /// # Errors
    /// Database errors.
    pub async fn record_site_version(
        &self,
        id: &str,
        version_id: &str,
        content: &SiteContent,
        spa: bool,
        password: bool,
    ) -> Result<u32, StoreError> {
        let (id, version_id) = (id.to_owned(), version_id.to_owned());
        let manifest = serde_json::to_string(&content.files)?;
        let (files, bytes) = (content.files.len() as u64, content.bytes());
        let (headers, redirects) = (content.headers.clone(), content.redirects.clone());
        self.store()
            .call(move |conn| {
                let tx = conn.transaction()?;
                let number: i64 = tx.query_row(
                    "SELECT COALESCE(MAX(number), 0) + 1 FROM snapshot_versions WHERE snapshot_id = ?1",
                    params![id],
                    |r| r.get(0),
                )?;
                let now = now_ms();
                tx.execute(
                    "INSERT INTO snapshot_versions (snapshot_id, number, version_id, created_at,
                       files, bytes, manifest, headers, redirects, spa, password)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                    params![
                        id,
                        number,
                        version_id,
                        now,
                        i64::try_from(files).unwrap_or(i64::MAX),
                        i64::try_from(bytes).unwrap_or(i64::MAX),
                        manifest,
                        headers,
                        redirects,
                        i64::from(spa),
                        i64::from(password),
                    ],
                )?;
                tx.execute(
                    "UPDATE snapshots SET live_version = ?2, spa = ?3, password = ?4, updated_at = ?5
                     WHERE id = ?1",
                    params![id, version_id, i64::from(spa), i64::from(password), now],
                )?;
                tx.execute(
                    "DELETE FROM snapshot_versions WHERE snapshot_id = ?1 AND number <= ?2",
                    params![id, number - i64::from(KEPT_VERSIONS)],
                )?;
                tx.commit()?;
                Ok(u32::try_from(number).unwrap_or_default())
            })
            .await
    }

    /// Makes `version_id` the live version again (a rollback, or undoing a publish),
    /// with its settings.
    ///
    /// # Errors
    /// Database errors.
    pub async fn set_site_live(&self, id: &str, version_id: &str) -> Result<(), StoreError> {
        let (id, version_id) = (id.to_owned(), version_id.to_owned());
        self.store()
            .call(move |conn| {
                conn.execute(
                    "UPDATE snapshots SET live_version = ?2,
                       spa = COALESCE((SELECT spa FROM snapshot_versions
                         WHERE snapshot_id = ?1 AND version_id = ?2), spa),
                       password = COALESCE((SELECT password FROM snapshot_versions
                         WHERE snapshot_id = ?1 AND version_id = ?2), password)
                     WHERE id = ?1",
                    params![id, version_id],
                )?;
                Ok(())
            })
            .await
    }

    /// Forgets one version (its publish was undone).
    ///
    /// # Errors
    /// Database errors.
    pub async fn drop_site_version(&self, id: &str, version_id: &str) -> Result<(), StoreError> {
        let (id, version_id) = (id.to_owned(), version_id.to_owned());
        self.store()
            .call(move |conn| {
                conn.execute(
                    "DELETE FROM snapshot_versions WHERE snapshot_id = ?1 AND version_id = ?2",
                    params![id, version_id],
                )?;
                conn.execute(
                    "UPDATE snapshots SET live_version = NULL WHERE id = ?1 AND live_version = ?2",
                    params![id, version_id],
                )?;
                Ok(())
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;

    fn row(id: &str, name: &str) -> SiteRow {
        SiteRow {
            id: id.into(),
            account_id: "acc".into(),
            name: name.into(),
            script: format!("teitunnel-{name}"),
            hostname: Some(format!("{name}.xyz.com")),
            source: "{}".into(),
            spa: false,
            password: false,
            access: None,
            expires_at: None,
            owner: "app".into(),
            live_version: None,
            created_at: 1,
            updated_at: 1,
        }
    }

    fn content(n: usize) -> SiteContent {
        SiteContent {
            root: None,
            files: (0..n)
                .map(|i| SiteFile {
                    path: format!("/{i}.html"),
                    hash: format!("{i:032x}"),
                    size: 10,
                    content_type: "text/html".into(),
                })
                .collect(),
            headers: Some("/*\n  X-A: b".into()),
            redirects: None,
        }
    }

    #[tokio::test]
    async fn keeps_snapshots_and_their_recent_versions() {
        let local = Local::new(Store::open_in_memory().unwrap());
        local.save_site(&row("s1", "demo")).await.unwrap();
        assert!(
            local.save_site(&row("s2", "demo")).await.is_err(),
            "names are unique"
        );
        for n in 1..=12 {
            let number = local
                .record_site_version("s1", &format!("v{n}"), &content(n), n % 2 == 0, false)
                .await
                .unwrap();
            assert_eq!(number, u32::try_from(n).unwrap());
        }
        let versions = local.site_versions("s1").await.unwrap();
        assert_eq!(versions.len(), KEPT_VERSIONS as usize);
        assert_eq!((versions[0].number, versions[0].files), (12, 12));
        assert_eq!(versions.last().unwrap().number, 3);
        let site = local.site("s1").await.unwrap().unwrap();
        assert_eq!(site.live_version.as_deref(), Some("v12"));
        assert!(site.spa);

        local.set_site_live("s1", "v5").await.unwrap();
        let site = local.site("s1").await.unwrap().unwrap();
        assert_eq!(site.live_version.as_deref(), Some("v5"));
        assert!(!site.spa, "the version's settings come back too");
        let files = local
            .site_version_content("s1", "v5")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(files.files.len(), 5);
        assert_eq!(files.headers.as_deref(), Some("/*\n  X-A: b"));

        local.drop_site_version("s1", "v5").await.unwrap();
        assert_eq!(local.site("s1").await.unwrap().unwrap().live_version, None);
        local.forget_site("s1").await.unwrap();
        assert!(local.site_versions("s1").await.unwrap().is_empty());
        assert!(local.sites(None).await.unwrap().is_empty());
    }
}
