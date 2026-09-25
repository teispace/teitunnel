//! Clients and approved connections in the database (hashes only).

use rusqlite::{OptionalExtension, Row, params};

use super::protocol::Client;
use crate::store::{Store, StoreError};

/// Registered clients kept per hostname at most (the oldest go first).
const MAX_CLIENTS: i64 = 200;

fn to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

fn to_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

/// A registered client (Dynamic Client Registration).
pub(crate) async fn save_client(
    store: &Store,
    host: &str,
    client: &Client,
    now_ms: u64,
) -> Result<(), StoreError> {
    let host = host.to_owned();
    let client = client.clone();
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO mcp_oauth_clients (client_id, host, name, redirect_uris, secret_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    client.id,
                    host,
                    client.name,
                    serde_json::to_string(&client.redirect_uris).unwrap_or_default(),
                    client.secret_hash,
                    to_i64(now_ms),
                ],
            )?;
            // Keep the newest, and every client that still has a connection.
            tx.execute(
                "DELETE FROM mcp_oauth_clients WHERE host = ?1 AND client_id NOT IN (
                     SELECT client_id FROM mcp_oauth_grants WHERE host = ?1
                 ) AND client_id NOT IN (
                     SELECT client_id FROM mcp_oauth_clients WHERE host = ?1
                     ORDER BY created_at DESC LIMIT ?2
                 )",
                params![host, MAX_CLIENTS],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
}

/// A registered client of `host`.
pub(crate) async fn client(
    store: &Store,
    host: &str,
    client_id: &str,
) -> Result<Option<Client>, StoreError> {
    let (host, client_id) = (host.to_owned(), client_id.to_owned());
    store
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT client_id, name, redirect_uris, secret_hash FROM mcp_oauth_clients
                     WHERE host = ?1 AND client_id = ?2",
                    params![host, client_id],
                    |row| {
                        let uris: String = row.get(2)?;
                        Ok(Client {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            redirect_uris: serde_json::from_str(&uris).unwrap_or_default(),
                            secret_hash: row.get(3)?,
                            published_by: None,
                        })
                    },
                )
                .optional()?)
        })
        .await
}

/// An approved connection, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Grant {
    pub(crate) id: String,
    pub(crate) host: String,
    pub(crate) client_id: String,
    pub(crate) client_name: String,
    pub(crate) redirect_host: String,
    pub(crate) created_at: u64,
    pub(crate) last_used_at: u64,
    pub(crate) access_hash: Option<String>,
    pub(crate) access_expires_at: Option<u64>,
    pub(crate) refresh_hash: String,
    pub(crate) refresh_expires_at: u64,
}

const GRANT_COLUMNS: &str =
    "id, host, client_id, client_name, redirect_host, created_at, last_used_at,
     access_hash, access_expires_at, refresh_hash, refresh_expires_at";

fn grant(row: &Row<'_>) -> rusqlite::Result<Grant> {
    Ok(Grant {
        id: row.get(0)?,
        host: row.get(1)?,
        client_id: row.get(2)?,
        client_name: row.get(3)?,
        redirect_host: row.get(4)?,
        created_at: to_u64(row.get(5)?),
        last_used_at: to_u64(row.get(6)?),
        access_hash: row.get(7)?,
        access_expires_at: row.get::<_, Option<i64>>(8)?.map(to_u64),
        refresh_hash: row.get(9)?,
        refresh_expires_at: to_u64(row.get(10)?),
    })
}

pub(crate) async fn insert_grant(store: &Store, grant: &Grant) -> Result<(), StoreError> {
    let g = grant.clone();
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO mcp_oauth_grants (id, host, client_id, client_name, redirect_host,
                     created_at, last_used_at, access_hash, access_expires_at, refresh_hash,
                     previous_refresh_hash, refresh_expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, NULL, ?11)",
                params![
                    g.id,
                    g.host,
                    g.client_id,
                    g.client_name,
                    g.redirect_host,
                    to_i64(g.created_at),
                    to_i64(g.last_used_at),
                    g.access_hash,
                    g.access_expires_at.map(to_i64),
                    g.refresh_hash,
                    to_i64(g.refresh_expires_at),
                ],
            )?;
            Ok(())
        })
        .await
}

/// What a refresh token found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Refresh {
    /// The connection it belongs to.
    Current(Grant),
    /// A token that was already exchanged: someone replayed it.
    Reused(String),
    Unknown,
}

pub(crate) async fn find_refresh(
    store: &Store,
    host: &str,
    refresh_hash: &str,
) -> Result<Refresh, StoreError> {
    let (host, hash) = (host.to_owned(), refresh_hash.to_owned());
    store
        .call(move |conn| {
            if let Some(found) = conn
                .query_row(
                    &format!(
                        "SELECT {GRANT_COLUMNS} FROM mcp_oauth_grants WHERE host = ?1 AND refresh_hash = ?2"
                    ),
                    params![host, hash],
                    grant,
                )
                .optional()?
            {
                return Ok(Refresh::Current(found));
            }
            Ok(conn
                .query_row(
                    "SELECT id FROM mcp_oauth_grants WHERE host = ?1 AND previous_refresh_hash = ?2",
                    params![host, hash],
                    |row| row.get(0),
                )
                .optional()?
                .map_or(Refresh::Unknown, Refresh::Reused))
        })
        .await
}

/// New tokens for a connection (the old refresh token is remembered, to catch replays).
pub(crate) async fn rotate(
    store: &Store,
    id: &str,
    access: (&str, u64),
    refresh: (&str, u64),
    now_ms: u64,
) -> Result<bool, StoreError> {
    let (id, access_hash, refresh_hash) =
        (id.to_owned(), access.0.to_owned(), refresh.0.to_owned());
    let (access_expires, refresh_expires) = (access.1, refresh.1);
    store
        .call(move |conn| {
            let changed = conn.execute(
                "UPDATE mcp_oauth_grants SET previous_refresh_hash = refresh_hash,
                     refresh_hash = ?2, refresh_expires_at = ?3, access_hash = ?4,
                     access_expires_at = ?5, last_used_at = ?6
                 WHERE id = ?1",
                params![
                    id,
                    refresh_hash,
                    to_i64(refresh_expires),
                    access_hash,
                    to_i64(access_expires),
                    to_i64(now_ms),
                ],
            )?;
            Ok(changed == 1)
        })
        .await
}

/// Ends a connection. Returns whether it existed.
pub(crate) async fn delete_grant(store: &Store, id: &str) -> Result<bool, StoreError> {
    let id = id.to_owned();
    store
        .call(move |conn| {
            Ok(conn.execute("DELETE FROM mcp_oauth_grants WHERE id = ?1", params![id])? == 1)
        })
        .await
}

/// Connections (of one hostname, or all), newest first.
pub(crate) async fn grants(store: &Store, host: Option<&str>) -> Result<Vec<Grant>, StoreError> {
    let host = host.map(str::to_owned);
    store
        .call(move |conn| {
            let mut statement = conn.prepare(&format!(
                "SELECT {GRANT_COLUMNS} FROM mcp_oauth_grants
                 WHERE ?1 IS NULL OR host = ?1 ORDER BY created_at DESC"
            ))?;
            let rows = statement
                .query_map(params![host], grant)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
}

/// Removes connections whose refresh token expired.
pub(crate) async fn sweep(store: &Store, now_ms: u64) -> Result<usize, StoreError> {
    store
        .call(move |conn| {
            Ok(conn.execute(
                "DELETE FROM mcp_oauth_grants WHERE refresh_expires_at <= ?1",
                params![to_i64(now_ms)],
            )?)
        })
        .await
}
