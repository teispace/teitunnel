//! Snapshot comments on Cloudflare: the D1 database the Snapshot's Worker writes to,
//! read and answered here through the D1 query endpoint with the account's own token
//! (docs/research/cloudflare-workers-features.md). The Worker's copy of the schema is in
//! `engine/snapshot-worker.js`; a test keeps the two identical.

use std::collections::BTreeMap;

use cf_api::D1Statement;
use serde_json::{Value, json};

use super::{
    Author, CommentsError, MAX_COMMENTS_PER_SUBJECT, MAX_PER_THREAD, Row, Thread, clean_body,
    new_id, threads_from,
};
use crate::{domain_shares::now_ms, engine::CloudApi};

/// The D1 database Teitunnel keeps on an account (comments and webhook inboxes).
pub const DATABASE_NAME: &str = "teitunnel-data";

/// The comments table, as both this app and the Worker create it.
pub const COMMENTS_SCHEMA: &[&str] = &[
    "CREATE TABLE IF NOT EXISTS teitunnel_comments (id TEXT PRIMARY KEY, site TEXT NOT NULL, thread TEXT NOT NULL, path TEXT NOT NULL, anchor TEXT, author TEXT NOT NULL, email TEXT, verified INTEGER NOT NULL DEFAULT 0, by_owner INTEGER NOT NULL DEFAULT 0, body TEXT NOT NULL, created_at INTEGER NOT NULL, resolved_at INTEGER, resolved_by TEXT, client TEXT)",
    "CREATE INDEX IF NOT EXISTS teitunnel_comments_site ON teitunnel_comments (site, created_at)",
    "CREATE INDEX IF NOT EXISTS teitunnel_comments_client ON teitunnel_comments (client, created_at)",
];

const COLUMNS: &str = "id, thread, path, anchor, author, email, verified, by_owner, body, created_at, resolved_at, resolved_by";

/// A Snapshot's comment counts on Cloudflare.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RemoteCounts {
    /// Newest comment (ms).
    pub latest_at: u64,
    /// Newest comment by a reviewer (ms).
    pub reviewer_latest_at: u64,
    /// Comments.
    pub comments: u32,
    /// Unresolved threads.
    pub open: u32,
    /// Reviewer comments after the owner last looked.
    pub unread: u32,
}

fn statement(sql: &str, params: Vec<Value>) -> D1Statement {
    D1Statement::new(sql, params)
}

fn missing_table(err: &cf_api::Error) -> bool {
    err.detail().contains("no such table")
}

/// Creates the comments table if it isn't there.
///
/// # Errors
/// API errors.
pub async fn ensure_schema<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
) -> Result<(), cf_api::Error> {
    let statements: Vec<D1Statement> = COMMENTS_SCHEMA
        .iter()
        .map(|sql| statement(sql, Vec::new()))
        .collect();
    api.d1_query(account, database, &statements).await?;
    Ok(())
}

/// Runs statements, creating the table first when it's missing (a database made by an
/// older Teitunnel, or by the inbox).
async fn query<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
    statements: &[D1Statement],
) -> Result<Vec<cf_api::D1Result>, CommentsError> {
    match api.d1_query(account, database, statements).await {
        Err(err) if missing_table(&err) => {
            ensure_schema(api, account, database).await?;
            Ok(api.d1_query(account, database, statements).await?)
        }
        other => Ok(other?),
    }
}

fn num(value: &Value) -> u64 {
    value
        .as_u64()
        .or_else(|| value.as_f64().map(|f| f.max(0.0) as u64))
        .unwrap_or_default()
}

fn text(value: &Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

fn row(value: &Value) -> Option<Row> {
    Some(Row {
        id: text(&value["id"])?,
        thread: text(&value["thread"])?,
        path: text(&value["path"])?,
        anchor: text(&value["anchor"]),
        author: text(&value["author"])?,
        email: text(&value["email"]),
        verified: num(&value["verified"]) != 0,
        by_owner: num(&value["by_owner"]) != 0,
        body: text(&value["body"])?,
        created_at: num(&value["created_at"]),
        resolved_at: value["resolved_at"]
            .as_f64()
            .map(|_| num(&value["resolved_at"])),
        resolved_by: text(&value["resolved_by"]),
    })
}

fn rows(results: &[cf_api::D1Result]) -> Vec<Row> {
    results
        .iter()
        .flat_map(|r| r.results.iter())
        .filter_map(row)
        .collect()
}

/// Every thread of a Snapshot (`site` is its Worker's name).
///
/// # Errors
/// API errors.
pub async fn threads<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
    site: &str,
) -> Result<Vec<Thread>, CommentsError> {
    let results = query(
        api,
        account,
        database,
        &[statement(
            &format!(
                "SELECT {COLUMNS} FROM teitunnel_comments WHERE site = ?1 ORDER BY created_at LIMIT {MAX_COMMENTS_PER_SUBJECT}"
            ),
            vec![json!(site)],
        )],
    )
    .await?;
    Ok(threads_from(rows(&results)))
}

async fn thread<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
    site: &str,
    id: &str,
) -> Result<Thread, CommentsError> {
    let results = query(
        api,
        account,
        database,
        &[statement(
            &format!("SELECT {COLUMNS} FROM teitunnel_comments WHERE site = ?1 AND thread = ?2"),
            vec![json!(site), json!(id)],
        )],
    )
    .await?;
    threads_from(rows(&results))
        .into_iter()
        .next()
        .ok_or(CommentsError::NotFound)
}

/// The owner's reply on a Snapshot thread.
///
/// # Errors
/// Invalid text, an unknown or full thread, API errors.
pub async fn reply<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
    site: &str,
    thread_id: &str,
    body: &str,
    author: &Author,
) -> Result<Thread, CommentsError> {
    let body = clean_body(body)?;
    let current = thread(api, account, database, site, thread_id).await?;
    if current.comments.len() >= MAX_PER_THREAD {
        return Err(CommentsError::TooMany);
    }
    query(
        api,
        account,
        database,
        &[statement(
            "INSERT INTO teitunnel_comments (id, site, thread, path, author, email, verified, by_owner, body, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            vec![
                json!(new_id()),
                json!(site),
                json!(thread_id),
                json!(current.path),
                json!(author.name),
                json!(author.email),
                json!(i64::from(author.verified)),
                json!(i64::from(author.by_owner)),
                json!(body),
                json!(now_ms()),
            ],
        )],
    )
    .await?;
    thread(api, account, database, site, thread_id).await
}

/// Resolves or reopens a Snapshot thread.
///
/// # Errors
/// An unknown thread, API errors.
pub async fn resolve<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
    site: &str,
    thread_id: &str,
    resolved: bool,
    by: &str,
) -> Result<Thread, CommentsError> {
    let by: String = by.trim().chars().take(super::MAX_NAME).collect();
    let results = query(
        api,
        account,
        database,
        &[statement(
            "UPDATE teitunnel_comments SET resolved_at = ?3, resolved_by = ?4 WHERE site = ?1 AND id = ?2 AND thread = id",
            vec![
                json!(site),
                json!(thread_id),
                if resolved { json!(now_ms()) } else { Value::Null },
                if resolved && !by.is_empty() { json!(by) } else { Value::Null },
            ],
        )],
    )
    .await?;
    if results.first().is_some_and(|r| r.meta.changes == 0) {
        return Err(CommentsError::NotFound);
    }
    thread(api, account, database, site, thread_id).await
}

/// Counts for several Snapshots in one query: `sites` maps a Worker name to when the
/// owner last looked (ms).
///
/// # Errors
/// API errors.
pub async fn counts<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
    sites: &BTreeMap<String, u64>,
) -> Result<BTreeMap<String, RemoteCounts>, CommentsError> {
    if sites.is_empty() {
        return Ok(BTreeMap::new());
    }
    // One grouped query; "unread" needs each site's own time, so it's computed from a
    // second, bounded read of recent reviewer comments.
    let placeholders: Vec<String> = (1..=sites.len()).map(|i| format!("?{i}")).collect();
    let params: Vec<Value> = sites.keys().map(|s| json!(s)).collect();
    let in_list = placeholders.join(", ");
    let oldest_seen = sites.values().copied().min().unwrap_or_default();
    let mut recent_params = params.clone();
    recent_params.push(json!(oldest_seen));
    let results = query(
        api,
        account,
        database,
        &[
            statement(
                &format!(
                    "SELECT site, count(*) AS comments, max(created_at) AS latest, \
                     max(CASE WHEN by_owner = 0 THEN created_at ELSE 0 END) AS reviewer_latest, \
                     sum(CASE WHEN thread = id AND resolved_at IS NULL THEN 1 ELSE 0 END) AS open \
                     FROM teitunnel_comments WHERE site IN ({in_list}) GROUP BY site"
                ),
                params,
            ),
            statement(
                &format!(
                    "SELECT site, created_at FROM teitunnel_comments WHERE site IN ({in_list}) \
                     AND by_owner = 0 AND created_at > ?{} ORDER BY created_at DESC LIMIT 1000",
                    sites.len() + 1
                ),
                recent_params,
            ),
        ],
    )
    .await?;
    let mut out: BTreeMap<String, RemoteCounts> = BTreeMap::new();
    if let Some(grouped) = results.first() {
        for r in &grouped.results {
            let Some(site) = r["site"].as_str() else {
                continue;
            };
            out.insert(
                site.to_owned(),
                RemoteCounts {
                    latest_at: num(&r["latest"]),
                    reviewer_latest_at: num(&r["reviewer_latest"]),
                    comments: u32::try_from(num(&r["comments"])).unwrap_or(u32::MAX),
                    open: u32::try_from(num(&r["open"])).unwrap_or(u32::MAX),
                    unread: 0,
                },
            );
        }
    }
    if let Some(recent) = results.get(1) {
        for r in &recent.results {
            let (Some(site), created) = (r["site"].as_str(), num(&r["created_at"])) else {
                continue;
            };
            if sites.get(site).is_some_and(|seen| created > *seen)
                && let Some(counts) = out.get_mut(site)
            {
                counts.unread = counts.unread.saturating_add(1);
            }
        }
    }
    Ok(out)
}

/// Deletes a Snapshot's comments (when the Snapshot is deleted).
///
/// # Errors
/// API errors.
pub async fn delete_site<C: CloudApi>(
    api: &C,
    account: &str,
    database: &str,
    site: &str,
) -> Result<(), CommentsError> {
    query(
        api,
        account,
        database,
        &[statement(
            "DELETE FROM teitunnel_comments WHERE site = ?1",
            vec![json!(site)],
        )],
    )
    .await?;
    Ok(())
}
