//! Comments kept in this computer's database (live shares and routes), and the list of
//! subjects with their counts (Snapshots' counts come from Cloudflare, see `remote`).

use rusqlite::{OptionalExtension, Transaction, params};

use super::{
    Anchor, Author, CommentsError, MAX_PER_THREAD, Row, Subject, SubjectKind, SubjectView, Thread,
    clean_anchor, clean_body, clean_path, new_id, threads_from,
};
use crate::{domain_shares::now_ms, store::Store};

/// Most comments one share or route keeps (older threads must be resolved and cleared
/// before more are accepted).
pub const MAX_COMMENTS_PER_SUBJECT: usize = 2000;

const COLUMNS: &str = "id, thread, path, anchor, author, email, verified, by_owner, body,
    created_at, resolved_at, resolved_by";

fn ms(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

fn db(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Row> {
    Ok(Row {
        id: r.get(0)?,
        thread: r.get(1)?,
        path: r.get(2)?,
        anchor: r.get(3)?,
        author: r.get(4)?,
        email: r.get(5)?,
        verified: r.get::<_, i64>(6)? != 0,
        by_owner: r.get::<_, i64>(7)? != 0,
        body: r.get(8)?,
        created_at: ms(r.get(9)?),
        resolved_at: r.get::<_, Option<i64>>(10)?.map(ms),
        resolved_by: r.get(11)?,
    })
}

fn upsert_subject(tx: &Transaction<'_>, subject: &Subject, now: i64) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO comment_subjects (subject, account_id, kind, label, url, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (subject) DO UPDATE SET label = ?4, url = coalesce(?5, url), updated_at = ?6",
        params![
            subject.key,
            subject.account_id,
            subject.kind.as_str(),
            subject.label,
            subject.url,
            now
        ],
    )?;
    Ok(())
}

fn load_thread(
    conn: &rusqlite::Connection,
    subject: &str,
    thread: &str,
) -> rusqlite::Result<Option<Thread>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM comments WHERE subject = ?1 AND thread = ?2"
    ))?;
    let rows = stmt
        .query_map(params![subject, thread], row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(threads_from(rows).into_iter().next())
}

pub(super) async fn register(store: &Store, subject: &Subject) -> Result<(), CommentsError> {
    let subject = subject.clone();
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            upsert_subject(&tx, &subject, db(now_ms()))?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(())
}

pub(super) async fn threads(
    store: &Store,
    subject: &str,
    path: Option<&str>,
) -> Result<Vec<Thread>, CommentsError> {
    let (subject, path) = (subject.to_owned(), path.map(str::to_owned));
    let rows = store
        .call(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM comments
                 WHERE subject = ?1 AND (?2 IS NULL OR path = ?2)
                 ORDER BY created_at LIMIT {MAX_COMMENTS_PER_SUBJECT}"
            ))?;
            let rows = stmt
                .query_map(params![subject, path], row)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    Ok(threads_from(rows))
}

#[allow(clippy::too_many_arguments)]
fn insert(
    tx: &Transaction<'_>,
    subject: &Subject,
    id: &str,
    thread: &str,
    path: &str,
    anchor: Option<&str>,
    body: &str,
    author: &Author,
    now: i64,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO comments (id, subject, account_id, thread, path, anchor, author, email,
            verified, by_owner, body, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        params![
            id,
            subject.key,
            subject.account_id,
            thread,
            path,
            anchor,
            author.name,
            author.email,
            i64::from(author.verified),
            i64::from(author.by_owner),
            body,
            now
        ],
    )?;
    Ok(())
}

fn count(tx: &Transaction<'_>, sql: &str, key: &str) -> rusqlite::Result<usize> {
    let n: i64 = tx.query_row(sql, params![key], |r| r.get(0))?;
    Ok(usize::try_from(n).unwrap_or(usize::MAX))
}

pub(super) async fn start(
    store: &Store,
    subject: &Subject,
    path: &str,
    anchor: Option<&Anchor>,
    body: &str,
    author: &Author,
) -> Result<Thread, CommentsError> {
    let path = clean_path(path)?;
    let body = clean_body(body)?;
    let anchor = anchor
        .map(clean_anchor)
        .transpose()?
        .map(|a| serde_json::to_string(&a).unwrap_or_default());
    let (subject, author) = (subject.clone(), author.clone());
    let id = new_id();
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            if count(
                &tx,
                "SELECT count(*) FROM comments WHERE subject = ?1",
                &subject.key,
            )? >= MAX_COMMENTS_PER_SUBJECT
            {
                return Ok(Err(CommentsError::TooMany));
            }
            let now = db(now_ms());
            upsert_subject(&tx, &subject, now)?;
            insert(
                &tx,
                &subject,
                &id,
                &id,
                &path,
                anchor.as_deref(),
                &body,
                &author,
                now,
            )?;
            let thread = load_thread(&tx, &subject.key, &id)?;
            tx.commit()?;
            Ok(thread.ok_or(CommentsError::NotFound))
        })
        .await?
}

pub(super) async fn reply(
    store: &Store,
    subject: &Subject,
    thread: &str,
    body: &str,
    author: &Author,
) -> Result<Thread, CommentsError> {
    let body = clean_body(body)?;
    let (subject, author, thread) = (subject.clone(), author.clone(), thread.to_owned());
    let id = new_id();
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            let path: Option<String> = tx
                .query_row(
                    "SELECT path FROM comments WHERE subject = ?1 AND id = ?2 AND thread = id",
                    params![subject.key, thread],
                    |r| r.get(0),
                )
                .optional()?;
            let Some(path) = path else {
                return Ok(Err(CommentsError::NotFound));
            };
            let in_thread: i64 = tx.query_row(
                "SELECT count(*) FROM comments WHERE subject = ?1 AND thread = ?2",
                params![subject.key, thread],
                |r| r.get(0),
            )?;
            if usize::try_from(in_thread).unwrap_or(usize::MAX) >= MAX_PER_THREAD
                || count(
                    &tx,
                    "SELECT count(*) FROM comments WHERE subject = ?1",
                    &subject.key,
                )? >= MAX_COMMENTS_PER_SUBJECT
            {
                return Ok(Err(CommentsError::TooMany));
            }
            let now = db(now_ms());
            upsert_subject(&tx, &subject, now)?;
            insert(
                &tx, &subject, &id, &thread, &path, None, &body, &author, now,
            )?;
            let loaded = load_thread(&tx, &subject.key, &thread)?;
            tx.commit()?;
            Ok(loaded.ok_or(CommentsError::NotFound))
        })
        .await?
}

pub(super) async fn resolve(
    store: &Store,
    subject: &str,
    thread: &str,
    resolved: bool,
    by: &str,
) -> Result<Thread, CommentsError> {
    let (subject, thread) = (subject.to_owned(), thread.to_owned());
    let by: String = by.trim().chars().take(super::MAX_NAME).collect();
    store
        .call(move |conn| {
            let now = db(now_ms());
            let changed = conn.execute(
                "UPDATE comments SET resolved_at = ?3, resolved_by = ?4
                 WHERE subject = ?1 AND id = ?2 AND thread = id",
                params![
                    subject,
                    thread,
                    resolved.then_some(now),
                    (resolved && !by.is_empty()).then_some(by)
                ],
            )?;
            if changed == 0 {
                return Ok(Err(CommentsError::NotFound));
            }
            Ok(load_thread(conn, &subject, &thread)?.ok_or(CommentsError::NotFound))
        })
        .await?
}

pub(super) async fn subject(store: &Store, key: &str) -> Result<Option<Subject>, CommentsError> {
    let key = key.to_owned();
    Ok(store
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT subject, kind, account_id, label, url FROM comment_subjects
                     WHERE subject = ?1",
                    params![key],
                    |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, Option<String>>(4)?,
                        ))
                    },
                )
                .optional()?)
        })
        .await?
        .and_then(|(key, kind, account_id, label, url)| {
            Some(Subject {
                key,
                kind: SubjectKind::parse(&kind)?,
                account_id,
                label,
                url,
            })
        }))
}

pub(super) async fn subjects(store: &Store) -> Result<Vec<SubjectView>, CommentsError> {
    let rows = store
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT s.subject, s.kind, s.account_id, s.label, s.url,
                    s.remote_comments, s.remote_open, s.remote_unread, s.latest_at,
                    (SELECT count(*) FROM comments c WHERE c.subject = s.subject),
                    (SELECT count(*) FROM comments c WHERE c.subject = s.subject
                        AND c.thread = c.id AND c.resolved_at IS NULL),
                    (SELECT count(*) FROM comments c WHERE c.subject = s.subject
                        AND c.by_owner = 0 AND c.created_at > s.seen_at),
                    (SELECT max(created_at) FROM comments c WHERE c.subject = s.subject)
                 FROM comment_subjects s",
            )?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((
                        (
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, Option<String>>(2)?,
                            r.get::<_, String>(3)?,
                            r.get::<_, Option<String>>(4)?,
                        ),
                        [
                            r.get::<_, i64>(5)?,
                            r.get::<_, i64>(6)?,
                            r.get::<_, i64>(7)?,
                            r.get::<_, i64>(8)?,
                            r.get::<_, i64>(9)?,
                            r.get::<_, i64>(10)?,
                            r.get::<_, i64>(11)?,
                            r.get::<_, Option<i64>>(12)?.unwrap_or_default(),
                        ],
                    ))
                })?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await?;
    let n = |v: i64| u32::try_from(v).unwrap_or(u32::MAX);
    let mut views: Vec<SubjectView> = rows
        .into_iter()
        .filter_map(|((key, kind, account_id, label, url), counts)| {
            let kind = SubjectKind::parse(&kind)?;
            let [
                r_comments,
                r_open,
                r_unread,
                r_latest,
                comments,
                open,
                unread,
                latest,
            ] = counts;
            let remote = kind == SubjectKind::Snapshot;
            let latest = if remote { r_latest } else { latest };
            Some(SubjectView {
                subject: Subject {
                    key,
                    kind,
                    account_id,
                    label,
                    url,
                },
                open: n(if remote { r_open } else { open }),
                comments: n(if remote { r_comments } else { comments }),
                unread: n(if remote { r_unread } else { unread }),
                latest_at: (latest > 0).then(|| ms(latest)),
            })
        })
        .collect();
    views.sort_by(|a, b| {
        b.latest_at
            .cmp(&a.latest_at)
            .then_with(|| a.subject.label.cmp(&b.subject.label))
    });
    Ok(views)
}

pub(super) async fn mark_seen(store: &Store, key: &str) -> Result<(), CommentsError> {
    let key = key.to_owned();
    store
        .call(move |conn| {
            let now = db(now_ms());
            conn.execute(
                "UPDATE comment_subjects SET seen_at = ?2, remote_unread = 0 WHERE subject = ?1",
                params![key, now],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

/// A Snapshot's counts as read from Cloudflare.
pub(super) async fn record_remote(
    store: &Store,
    key: &str,
    counts: super::remote::RemoteCounts,
) -> Result<bool, CommentsError> {
    let key = key.to_owned();
    Ok(store
        .call(move |conn| {
            let notified: Option<i64> = conn
                .query_row(
                    "SELECT notified_at FROM comment_subjects WHERE subject = ?1",
                    params![key],
                    |r| r.get(0),
                )
                .optional()?;
            let newer = notified.is_some_and(|n| db(counts.reviewer_latest_at) > n);
            conn.execute(
                "UPDATE comment_subjects SET latest_at = ?2, remote_comments = ?3,
                    remote_open = ?4, remote_unread = ?5, updated_at = ?6,
                    notified_at = max(notified_at, ?7)
                 WHERE subject = ?1",
                params![
                    key,
                    db(counts.latest_at),
                    i64::from(counts.comments),
                    i64::from(counts.open),
                    i64::from(counts.unread),
                    db(now_ms()),
                    db(counts.reviewer_latest_at)
                ],
            )?;
            Ok(newer)
        })
        .await?)
}

pub(super) async fn seen_at(store: &Store, key: &str) -> Result<u64, CommentsError> {
    let key = key.to_owned();
    Ok(store
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT seen_at FROM comment_subjects WHERE subject = ?1",
                    params![key],
                    |r| r.get::<_, i64>(0),
                )
                .optional()?
                .map_or(0, ms))
        })
        .await?)
}

pub(super) async fn forget(store: &Store, key: &str) -> Result<(), CommentsError> {
    let key = key.to_owned();
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute("DELETE FROM comments WHERE subject = ?1", params![key])?;
            tx.execute(
                "DELETE FROM comment_subjects WHERE subject = ?1",
                params![key],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(())
}
