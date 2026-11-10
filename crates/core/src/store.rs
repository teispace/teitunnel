//! Local persistence (SQLite) for what Cloudflare cannot hold: ownership index, run
//! modes, metrics ports, activity log, settings and metric rollups.
//!
//! One connection lives on a dedicated thread; callers send it closures and await the
//! result, so SQLite never blocks the async runtime and access is serialised without
//! locks. The database runs in WAL mode and its file is readable only by the user.

mod migrations;
pub use migrations::MigrationError;

use std::{
    fs,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
};

use rusqlite::Connection;
use tokio::sync::oneshot;

use crate::text::{Text, UserText, english_display, msg};

/// Errors from the local store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// SQLite reported an error.
    Sqlite(#[from] rusqlite::Error),
    /// A schema migration failed.
    Migration(#[from] MigrationError),
    /// Creating the database directory or file failed.
    Io(#[from] std::io::Error),
    /// A stored value couldn't be decoded.
    Decode(#[from] serde_json::Error),
    /// The store thread has stopped (the app is shutting down).
    Closed,
}

impl UserText for StoreError {
    fn text(&self) -> Text {
        match self {
            Self::Sqlite(detail) => msg::error::store::sqlite(detail),
            Self::Migration(detail) => msg::error::store::migration(detail),
            Self::Io(detail) => msg::error::store::io(detail),
            Self::Decode(detail) => msg::error::store::decode(detail),
            Self::Closed => msg::error::store::closed(),
        }
    }
}

english_display!(StoreError);

type Job = Box<dyn FnOnce(&mut Connection) + Send>;

/// Handle to the database thread. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Store {
    jobs: mpsc::Sender<Job>,
}

/// Switches the database to incremental auto-vacuum once (a full `VACUUM`, which on
/// a large inspector history takes a second or two), so pruned pages can be given
/// back to the disk instead of keeping the file at its largest size.
/// Gives free pages back to the disk. The pragma frees one page per step, so it's
/// stepped to the end (`execute_batch` would free a single page).
///
/// # Errors
/// Fails if SQLite does.
pub fn release_free_pages(conn: &Connection) -> rusqlite::Result<()> {
    let mut statement = conn.prepare("PRAGMA incremental_vacuum")?;
    let mut rows = statement.query([])?;
    while rows.next()?.is_some() {}
    Ok(())
}

/// Best effort: when another Teitunnel process has the database open, it's tried again
/// at the next start.
fn incremental_vacuum(conn: &Connection) {
    let switch = || -> rusqlite::Result<()> {
        let mode: i64 = conn.pragma_query_value(None, "auto_vacuum", |r| r.get(0))?;
        if mode != 2 {
            conn.pragma_update(None, "auto_vacuum", "INCREMENTAL")?;
            conn.execute_batch("VACUUM")?;
        }
        Ok(())
    };
    if let Err(err) = switch() {
        tracing::warn!(%err, "couldn't switch the database to incremental auto-vacuum");
    }
}

impl Store {
    /// Opens (creating if needed) the database at `path` and applies pending migrations.
    ///
    /// # Errors
    /// Fails if the file can't be created or opened, or a migration fails.
    pub fn open(path: &Path) -> Result<Self, StoreError> {
        prepare_file(path)?;
        Self::start(Connection::open(path)?, Some(path.to_path_buf()))
    }

    /// An in-memory database, for tests.
    ///
    /// # Errors
    /// Fails if SQLite can't open the database or a migration fails.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::start(Connection::open_in_memory()?, None)
    }

    fn start(mut conn: Connection, path: Option<PathBuf>) -> Result<Self, StoreError> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        incremental_vacuum(&conn);
        migrations::apply(&mut conn)?;

        let (jobs, queue) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("teitunnel-store".into())
            .spawn(move || {
                for job in queue {
                    job(&mut conn);
                }
                tracing::debug!(?path, "store thread stopped");
            })?;
        Ok(Self { jobs })
    }

    /// Runs `f` on the database thread and returns its result.
    ///
    /// # Errors
    /// Returns what `f` returns, or [`StoreError::Closed`] if the thread has stopped.
    pub async fn call<T, F>(&self, f: F) -> Result<T, StoreError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    {
        let (reply, result) = oneshot::channel();
        self.jobs
            .send(Box::new(move |conn| {
                // The caller may have gone away; nothing to do then.
                let _ = reply.send(f(conn));
            }))
            .map_err(|_| StoreError::Closed)?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}

/// Creates the parent directory (0700) and an empty database file (0600) if missing.
fn prepare_file(path: &Path) -> Result<(), StoreError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
        restrict(dir, 0o700)?;
    }
    if !path.exists() {
        fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)?;
    }
    restrict(path, 0o600)
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> Result<(), StoreError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> Result<(), StoreError> {
    // Windows: the per-user app data directory is already private to the user.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn gives_deleted_pages_back_to_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        // A database made before incremental auto-vacuum (mode 0).
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE blobs(b BLOB);
                 WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < 200)
                 INSERT INTO blobs SELECT randomblob(16384) FROM n;",
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let (mode, before, after) = store
            .call(|conn| {
                let mode: i64 = conn.pragma_query_value(None, "auto_vacuum", |r| r.get(0))?;
                let pages = |c: &Connection| {
                    c.pragma_query_value(None, "page_count", |r| r.get::<_, i64>(0))
                };
                let before = pages(conn)?;
                conn.execute("DELETE FROM blobs", [])?;
                release_free_pages(conn)?;
                Ok((mode, before, pages(conn)?))
            })
            .await
            .unwrap();
        assert_eq!(mode, 2, "incremental");
        assert!(after < before / 10, "{before} → {after} pages");
    }

    #[tokio::test]
    async fn runs_queries_on_the_store_thread() {
        let store = Store::open_in_memory().unwrap();
        let name = store
            .call(|_conn| Ok(thread::current().name().map(str::to_owned)))
            .await
            .unwrap();
        assert_eq!(name.as_deref(), Some("teitunnel-store"));
    }

    #[tokio::test]
    async fn propagates_errors() {
        let store = Store::open_in_memory().unwrap();
        let err = store
            .call(|conn| Ok(conn.execute("SELECT * FROM missing", [])?))
            .await
            .unwrap_err();
        assert!(matches!(err, StoreError::Sqlite(_)));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn database_file_is_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data").join("teitunnel.db");
        let store = Store::open(&path).unwrap();
        store
            .call(|conn| Ok(conn.execute("SELECT 1", [])?))
            .await
            .ok();
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(dir_mode, 0o700);
    }

    #[tokio::test]
    async fn reopening_keeps_data_and_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("teitunnel.db");
        {
            let store = Store::open(&path).unwrap();
            store
                .call(|conn| {
                    Ok(conn.execute(
                        "INSERT INTO settings (key, value) VALUES ('probe', '1')",
                        [],
                    )?)
                })
                .await
                .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let value: String = store
            .call(|conn| {
                Ok(conn.query_row(
                    "SELECT value FROM settings WHERE key = 'probe'",
                    [],
                    |row| row.get(0),
                )?)
            })
            .await
            .unwrap();
        assert_eq!(value, "1");
    }
}
