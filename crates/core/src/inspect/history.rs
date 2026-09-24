//! Where captures live: Lens's in-memory ring, plus a masked history in SQLite (the
//! last 24 hours by default, at most [`MAX_PER_TAP`] exchanges per tap and
//! [`HISTORY_BYTES`] in all) so a restart keeps recent traffic and other processes
//! (`teitunnel traffic`) can read it.
//!
//! Lens calls [`CaptureStore::put`] on its hot path, so writing never blocks: finished
//! exchanges go through a bounded channel to a writer task that masks them on the
//! blocking pool and stores them in batches. A full channel drops history writes (the
//! in-memory capture is unaffected).

use std::{
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use lens::{
    CaptureStore, Exchange, ExchangeId, Filter, MemoryStore, Page, Query, Redaction, TapId,
};
use rusqlite::{OptionalExtension, params};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use super::record::{self, Row};
use crate::store::{Store, StoreError};

/// Exchanges kept per tap in the history.
pub const MAX_PER_TAP: usize = 5_000;
/// Bytes the history may take in all (rows beyond it go, oldest first).
pub const HISTORY_BYTES: i64 = 256 * 1024 * 1024;
/// Exchanges restored into memory per tap at start.
const RESTORE_PER_TAP: i64 = 1_000;
/// Body bytes restored into memory in all.
const RESTORE_BYTES: usize = 64 * 1024 * 1024;
/// Writes waiting for the writer before new ones are dropped.
const QUEUE: usize = 4_096;
/// How long the writer collects a batch.
const BATCH_WINDOW: Duration = Duration::from_millis(200);
/// How often old history is pruned.
const PRUNE_EVERY: Duration = Duration::from_secs(60);

#[derive(Debug)]
pub(crate) enum Write {
    Put(Arc<Exchange>),
    Clear(Option<TapId>),
}

/// The capture store Lens uses.
#[derive(Debug)]
pub(crate) struct Captures {
    memory: MemoryStore,
    writer: Mutex<Option<mpsc::Sender<Write>>>,
    keep: AtomicBool,
}

impl Captures {
    pub(crate) fn new() -> Self {
        Self {
            memory: MemoryStore::default(),
            writer: Mutex::new(None),
            keep: AtomicBool::new(true),
        }
    }

    /// Starts sending finished exchanges to `writer`.
    pub(crate) fn attach(&self, writer: mpsc::Sender<Write>) {
        *self.writer.lock().unwrap_or_else(PoisonError::into_inner) = Some(writer);
    }

    /// Whether new captures go to the history.
    pub(crate) fn set_keep(&self, keep: bool) {
        self.keep.store(keep, Ordering::Relaxed);
    }

    /// Puts exchanges read from the history back in memory (not written again).
    pub(crate) fn restore(&self, exchanges: Vec<Exchange>) {
        for exchange in exchanges {
            self.memory.put(Arc::new(exchange));
        }
    }

    fn send(&self, write: Write) {
        let writer = self
            .writer
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some(writer) = writer
            && writer.try_send(write).is_err()
        {
            tracing::debug!("inspector history is behind; a capture wasn't kept on disk");
        }
    }
}

impl CaptureStore for Captures {
    fn configure(&self, tap: &TapId, capacity: usize) {
        self.memory.configure(tap, capacity);
    }

    fn put(&self, exchange: Arc<Exchange>) {
        let persist = exchange.is_finished() && self.keep.load(Ordering::Relaxed);
        if persist {
            self.send(Write::Put(Arc::clone(&exchange)));
        }
        self.memory.put(exchange);
    }

    fn get(&self, id: ExchangeId) -> Option<Arc<Exchange>> {
        self.memory.get(id)
    }

    fn list(&self, query: &Query, redaction: &Redaction) -> Page {
        self.memory.list(query, redaction)
    }

    fn clear(&self, tap: Option<&TapId>) {
        self.memory.clear(tap);
        self.send(Write::Clear(tap.cloned()));
    }
}

/// The channel the writer reads.
pub(crate) fn channel() -> (mpsc::Sender<Write>, mpsc::Receiver<Write>) {
    mpsc::channel(QUEUE)
}

/// Stores what arrives on `queue` until `stop` (then drains what's left).
pub(crate) async fn run_writer(
    store: Store,
    mut queue: mpsc::Receiver<Write>,
    retention_hours: Arc<AtomicU32>,
    stop: CancellationToken,
) {
    let mut last_prune = tokio::time::Instant::now();
    loop {
        let first = tokio::select! {
            write = queue.recv() => write,
            () = stop.cancelled() => None,
        };
        let Some(first) = first else { break };
        let mut batch = vec![first];
        if !stop.is_cancelled() {
            tokio::time::sleep(BATCH_WINDOW).await;
        }
        while batch.len() < 512 {
            match queue.try_recv() {
                Ok(write) => batch.push(write),
                Err(_) => break,
            }
        }
        let prune = last_prune.elapsed() >= PRUNE_EVERY;
        if prune {
            last_prune = tokio::time::Instant::now();
        }
        let hours = retention_hours.load(Ordering::Relaxed);
        if let Err(err) = write_batch(&store, batch, prune.then_some(hours)).await {
            tracing::warn!(%err, "couldn't keep inspector history");
        }
    }
    // Whatever arrived before the stop.
    let mut rest = Vec::new();
    while let Ok(write) = queue.try_recv() {
        rest.push(write);
    }
    if !rest.is_empty() {
        let _ = write_batch(&store, rest, None).await;
    }
}

enum Prepared {
    Put(Row),
    Clear(Option<String>),
}

async fn write_batch(
    store: &Store,
    batch: Vec<Write>,
    prune_hours: Option<u32>,
) -> Result<(), StoreError> {
    // Masking bodies is CPU work: off the async threads and off the database thread.
    let prepared = tokio::task::spawn_blocking(move || {
        batch
            .into_iter()
            .map(|write| match write {
                Write::Put(exchange) => Prepared::Put(record::to_row(&exchange)),
                Write::Clear(tap) => Prepared::Clear(tap.map(|t| t.to_string())),
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            for item in prepared {
                match item {
                    Prepared::Put(row) => insert(&tx, &row)?,
                    Prepared::Clear(Some(tap)) => {
                        tx.execute("DELETE FROM lens_exchanges WHERE tap = ?1", params![tap])?;
                    }
                    Prepared::Clear(None) => {
                        tx.execute("DELETE FROM lens_exchanges", [])?;
                    }
                }
            }
            if let Some(hours) = prune_hours {
                prune(&tx, hours)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
}

fn insert(conn: &rusqlite::Connection, row: &Row) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO lens_exchanges
            (id, tap, seq, started_at, method, host, path, status, kind, meta, request_body, response_body)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
         ON CONFLICT (id) DO UPDATE SET status = ?8, meta = ?10, request_body = ?11,
            response_body = ?12",
        params![
            row.id,
            row.tap,
            row.seq,
            row.started_at,
            row.method,
            row.host,
            row.path,
            row.status,
            row.kind,
            row.meta,
            row.request_body,
            row.response_body,
        ],
    )?;
    Ok(())
}

/// Drops history older than `hours`, beyond [`MAX_PER_TAP`] per tap, and beyond
/// [`HISTORY_BYTES`] in all; and taps stopped long ago with nothing left.
fn prune(conn: &rusqlite::Connection, hours: u32) -> rusqlite::Result<()> {
    let cutoff = now_ms() - i64::from(hours) * 3_600_000;
    conn.execute(
        "DELETE FROM lens_exchanges WHERE started_at < ?1",
        params![cutoff],
    )?;
    conn.execute(
        "DELETE FROM lens_exchanges WHERE rowid IN (
            SELECT rowid FROM (
                SELECT rowid, ROW_NUMBER() OVER (PARTITION BY tap ORDER BY seq DESC) AS n
                FROM lens_exchanges)
            WHERE n > ?1)",
        params![i64::try_from(MAX_PER_TAP).unwrap_or(i64::MAX)],
    )?;
    conn.execute(
        "DELETE FROM lens_exchanges WHERE rowid IN (
            SELECT rowid FROM (
                SELECT rowid, SUM(length(meta) + coalesce(length(request_body), 0)
                    + coalesce(length(response_body), 0))
                    OVER (ORDER BY started_at DESC, rowid DESC) AS total
                FROM lens_exchanges)
            WHERE total > ?1)",
        params![HISTORY_BYTES],
    )?;
    conn.execute(
        "DELETE FROM lens_taps WHERE stopped_at IS NOT NULL AND stopped_at < ?1
            AND id NOT IN (SELECT DISTINCT tap FROM lens_exchanges)",
        params![cutoff],
    )?;
    Ok(())
}

fn now_ms() -> i64 {
    i64::try_from(crate::domain_shares::now_ms()).unwrap_or(i64::MAX)
}

fn read_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<(i64, Row)> {
    Ok((
        row.get(0)?,
        Row {
            id: row.get(1)?,
            tap: row.get(2)?,
            seq: row.get(3)?,
            started_at: row.get(4)?,
            method: row.get(5)?,
            host: row.get(6)?,
            path: row.get(7)?,
            status: row.get(8)?,
            kind: row.get(9)?,
            meta: row.get(10)?,
            request_body: row.get::<_, Option<Vec<u8>>>(11)?.unwrap_or_default(),
            response_body: row.get::<_, Option<Vec<u8>>>(12)?.unwrap_or_default(),
        },
    ))
}

const COLUMNS: &str = "rowid, id, tap, seq, started_at, method, host, path, status, kind, meta,
    request_body, response_body";

/// Exchanges for memory at start: the last `hours`, newest first per tap, bounded.
pub(crate) async fn load_recent(store: &Store, hours: u32) -> Result<Vec<Exchange>, StoreError> {
    let cutoff = now_ms() - i64::from(hours) * 3_600_000;
    let rows = store
        .call(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM (
                    SELECT *, rowid, ROW_NUMBER() OVER (PARTITION BY tap ORDER BY seq DESC) AS n
                    FROM lens_exchanges WHERE started_at >= ?1)
                 WHERE n <= ?2 ORDER BY started_at DESC"
            ))?;
            let rows = stmt.query_map(params![cutoff, RESTORE_PER_TAP], read_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    let mut bytes = 0usize;
    let mut out = Vec::new();
    for (_, row) in rows {
        bytes += row.request_body.len() + row.response_body.len();
        if bytes > RESTORE_BYTES {
            break;
        }
        out.extend(record::from_row(&row));
    }
    Ok(out)
}

/// What to read from the history (another process's captures).
#[derive(Debug, Clone, Default)]
pub struct HistoryQuery {
    /// Criteria (text search runs on the masked capture).
    pub filter: Filter,
    /// At most this many (newest first).
    pub limit: usize,
}

/// The newest exchanges in the history matching `query`.
///
/// # Errors
/// The database can't be read.
pub async fn history(store: &Store, query: HistoryQuery) -> Result<Vec<Exchange>, StoreError> {
    let limit = query.limit.clamp(1, lens::MAX_PAGE);
    let filter = query.filter;
    store
        .call(move |conn| {
            let tap = filter.tap.as_ref().map(ToString::to_string);
            let since = filter
                .since_ms
                .map(|s| i64::try_from(s).unwrap_or(i64::MAX));
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM lens_exchanges
                 WHERE (?1 IS NULL OR tap = ?1) AND (?2 IS NULL OR started_at >= ?2)
                 ORDER BY started_at DESC, rowid DESC"
            ))?;
            let mut rows = stmt.query(params![tap, since])?;
            let redaction = Redaction::masked();
            let mut out = Vec::new();
            while let Some(row) = rows.next()? {
                let (_, row) = read_row(row)?;
                if let Some(exchange) = record::from_row(&row)
                    && filter.matches(&exchange, &redaction)
                {
                    out.push(exchange);
                    if out.len() == limit {
                        break;
                    }
                }
            }
            Ok(out)
        })
        .await
}

/// One exchange from the history.
///
/// # Errors
/// The database can't be read.
pub async fn history_get(store: &Store, id: ExchangeId) -> Result<Option<Exchange>, StoreError> {
    let id = id.to_string();
    store
        .call(move |conn| {
            let row = conn
                .query_row(
                    &format!("SELECT {COLUMNS} FROM lens_exchanges WHERE id = ?1"),
                    params![id],
                    read_row,
                )
                .optional()?;
            Ok(row.and_then(|(_, row)| record::from_row(&row)))
        })
        .await
}

/// Exchanges stored after the cursor `after` (a position from an earlier call; `None`
/// starts at the newest, returning nothing), oldest first, and the new cursor. For
/// following another process's traffic.
///
/// # Errors
/// The database can't be read.
pub async fn history_after(
    store: &Store,
    after: Option<i64>,
) -> Result<(Vec<Exchange>, i64), StoreError> {
    store
        .call(move |conn| {
            let Some(after) = after else {
                let newest: Option<i64> =
                    conn.query_row("SELECT max(rowid) FROM lens_exchanges", [], |r| r.get(0))?;
                return Ok((Vec::new(), newest.unwrap_or(0)));
            };
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM lens_exchanges WHERE rowid > ?1 ORDER BY rowid LIMIT 500"
            ))?;
            let rows = stmt.query_map(params![after], read_row)?;
            let mut cursor = after;
            let mut out = Vec::new();
            for row in rows {
                let (rowid, row) = row?;
                cursor = cursor.max(rowid);
                out.extend(record::from_row(&row));
            }
            Ok((out, cursor))
        })
        .await
}

/// Forgets the history of one tap, or all of it.
///
/// # Errors
/// The database can't be written.
pub async fn history_clear(store: &Store, tap: Option<&TapId>) -> Result<(), StoreError> {
    let tap = tap.map(ToString::to_string);
    store
        .call(move |conn| {
            conn.execute(
                "DELETE FROM lens_exchanges WHERE ?1 IS NULL OR tap = ?1",
                params![tap],
            )?;
            Ok(())
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::record::tests::sample;

    fn keep() -> Arc<AtomicU32> {
        Arc::new(AtomicU32::new(24))
    }

    #[tokio::test]
    async fn finished_exchanges_reach_the_history_masked() {
        let store = Store::open_in_memory().unwrap();
        let captures = Captures::new();
        let (tx, rx) = channel();
        captures.attach(tx);
        let stop = CancellationToken::new();
        let writer = tokio::spawn(run_writer(store.clone(), rx, keep(), stop.clone()));
        let mut pending = sample("t1", 1, "/a", 200);
        pending.state = lens::ExchangeState::Pending;
        captures.put(Arc::new(pending));
        captures.put(Arc::new(sample("t1", 2, "/b?token=zzz", 500)));
        captures.put(Arc::new(sample("t2", 1, "/c", 404)));
        stop.cancel();
        writer.await.unwrap();

        let all = history(
            &store,
            HistoryQuery {
                limit: 10,
                ..HistoryQuery::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(all.len(), 2, "only finished exchanges are kept");
        let errors = history(
            &store,
            HistoryQuery {
                filter: Filter {
                    status_classes: vec![5],
                    ..Filter::default()
                },
                limit: 10,
            },
        )
        .await
        .unwrap();
        assert_eq!(errors.len(), 1);
        assert!(!errors[0].request.uri.to_string().contains("zzz"));
        let got = history_get(&store, errors[0].id).await.unwrap().unwrap();
        assert_eq!(got.request.path(), "/b");

        // Following: nothing before the cursor, then what's new.
        let (none, cursor) = history_after(&store, None).await.unwrap();
        assert!(none.is_empty());
        let (tx, rx) = channel();
        captures.attach(tx);
        let stop = CancellationToken::new();
        let writer = tokio::spawn(run_writer(store.clone(), rx, keep(), stop.clone()));
        captures.put(Arc::new(sample("t1", 3, "/d", 200)));
        captures.clear(Some(&TapId::new("t2").unwrap()));
        stop.cancel();
        writer.await.unwrap();
        let (new, _) = history_after(&store, Some(cursor)).await.unwrap();
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].request.path(), "/d");
        let left = history(
            &store,
            HistoryQuery {
                limit: 10,
                ..HistoryQuery::default()
            },
        )
        .await
        .unwrap();
        assert!(left.iter().all(|e| e.tap.as_str() == "t1"), "t2 cleared");

        let restored = load_recent(&store, 24).await.unwrap();
        assert_eq!(restored.len(), 2);
        history_clear(&store, None).await.unwrap();
        assert!(load_recent(&store, 24).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn keeping_history_can_be_turned_off() {
        let store = Store::open_in_memory().unwrap();
        let captures = Captures::new();
        let (tx, rx) = channel();
        captures.attach(tx);
        captures.set_keep(false);
        let stop = CancellationToken::new();
        let writer = tokio::spawn(run_writer(store.clone(), rx, keep(), stop.clone()));
        captures.put(Arc::new(sample("t1", 1, "/a", 200)));
        stop.cancel();
        writer.await.unwrap();
        assert!(load_recent(&store, 24).await.unwrap().is_empty());
        assert_eq!(
            captures
                .list(&Query::default(), &Redaction::masked())
                .items
                .len(),
            1,
            "still captured in memory"
        );
    }

    #[tokio::test]
    async fn prunes_by_age_and_count() {
        let store = Store::open_in_memory().unwrap();
        let mut old = sample("t1", 1, "/old", 200);
        old.started_at_ms -= 3 * 3_600_000;
        let rows = vec![
            record::to_row(&old),
            record::to_row(&sample("t1", 2, "/new", 200)),
        ];
        store
            .call(move |conn| {
                for row in &rows {
                    insert(conn, row)?;
                }
                prune(conn, 1)?;
                Ok(())
            })
            .await
            .unwrap();
        let left = load_recent(&store, 24).await.unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].request.path(), "/new");
    }
}
