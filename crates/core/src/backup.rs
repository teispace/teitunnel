//! Moving to a new computer: Teitunnel's setup in one encrypted file (M12-12).
//!
//! What's in it: settings (alert rules and the project list included), this machine's
//! tunnels (ids and names; they run on the new computer instead), the indexes of the DNS
//! records and Access applications Teitunnel created (so it still only changes its own),
//! load-balanced routes, Snapshots with their recent versions, and local domains (names
//! and targets; the new computer makes and trusts its own certificate authority).
//! Accounts by name only.
//!
//! What's never in it: API tokens, OAuth grants, tunnel run tokens, webhook secrets,
//! the local CA's key, dashboard password and API-key hashes. After restoring, accounts
//! are connected again and run tokens fetched again from Cloudflare.
//!
//! The format (version 1): a 70-byte header (magic, version, the argon2id parameters, a
//! 16-byte salt, a 24-byte nonce), then the gzipped JSON sealed with XChaCha20-Poly1305
//! under a key derived from the passphrase with argon2id; the header is authenticated
//! too, so any change to the file is detected.

use std::collections::BTreeMap;

use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use zeroize::Zeroize;

use crate::{
    Secret,
    store::{Store, StoreError},
    text::{Text, UserText, english_display, msg::backup as m},
};

/// The file starts with this.
const MAGIC: &[u8; 16] = b"TEITUNNEL-BACKUP";
/// The format this version writes and reads.
pub const FORMAT: u8 = 1;
const KDF_ARGON2ID: u8 = 1;
const HEADER_LEN: usize = 16 + 1 + 1 + 12 + 16 + 24;
/// Shortest passphrase accepted for a new backup.
pub const MIN_PASSPHRASE: usize = 10;
/// Larger files aren't backups.
const MAX_FILE: usize = 64 * 1024 * 1024;
/// The suggested file extension.
pub const EXTENSION: &str = "teitunnel-backup";

/// Tables copied, with the columns cleared because they belong to this machine.
const TABLES: &[(&str, &[&str])] = &[
    ("local_tunnels", &["metrics_port"]),
    ("dns_ownership", &[]),
    ("access_ownership", &[]),
    ("balanced_routes", &[]),
    ("snapshots", &[]),
    ("snapshot_versions", &[]),
    ("edge_rules", &[]),
    ("service_tokens", &[]),
    ("local_domains", &[]),
    ("cloud_databases", &[]),
    ("front_workers", &[]),
];

/// Settings that belong to this machine (a lease held by a running process).
const LOCAL_SETTINGS: &[&str] = &["uptimeRunner"];

/// Why a backup couldn't be made or read. Messages are shown to the user.
#[derive(Debug, thiserror::Error)]
pub enum BackupError {
    /// The passphrase is too short.
    WeakPassphrase,
    /// The file isn't a Teitunnel backup.
    NotABackup,
    /// Made by a newer Teitunnel (a format this one can't read).
    UnsupportedFormat(u8),
    /// The passphrase is wrong, or the file was changed or damaged.
    WrongPassphrase,
    /// The backup is of a newer database than this Teitunnel's.
    NewerSchema,
    /// Reading or writing the file failed.
    Io {
        /// The path.
        path: String,
        /// What went wrong.
        detail: String,
    },
    /// The local database.
    Store(#[from] StoreError),
    /// Encoding or encryption failed (unexpected).
    Internal(String),
}

impl UserText for BackupError {
    fn text(&self) -> Text {
        match self {
            Self::WeakPassphrase => m::weak_passphrase(MIN_PASSPHRASE),
            Self::NotABackup => m::not_a_backup(),
            Self::UnsupportedFormat(version) => m::unsupported(version),
            Self::WrongPassphrase => m::wrong_passphrase(),
            Self::NewerSchema => m::newer_schema(),
            Self::Io { path, detail } => m::io(path, detail),
            Self::Store(err) => err.text(),
            Self::Internal(detail) => m::internal(detail),
        }
    }
}

english_display!(BackupError);

/// An account as the backup names it (reconnect it after restoring).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct AccountRef {
    /// Account id.
    pub id: String,
    /// Its name.
    pub name: String,
}

/// What a backup holds (the sealed plaintext).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Contents {
    /// When it was made (ms since the epoch).
    pub created_at: u64,
    /// The Teitunnel that made it.
    pub app_version: String,
    /// The computer it was made on.
    pub machine: String,
    /// The database schema version it was read from.
    pub schema: u32,
    /// Connected accounts, by name.
    pub accounts: Vec<AccountRef>,
    /// Settings (key → value).
    pub settings: BTreeMap<String, Value>,
    /// Rows of the copied tables.
    pub tables: BTreeMap<String, Vec<Map<String, Value>>>,
}

/// One part of a backup and its size.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SectionCount {
    /// `settings`, `local_tunnels`, `snapshots`…
    pub section: String,
    /// Entries in the backup.
    pub count: u32,
    /// Entries here now, which restoring replaces.
    pub existing: u32,
}

/// What restoring a backup would bring, for review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    /// When it was made (ms since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: u64,
    /// The Teitunnel that made it.
    pub app_version: String,
    /// The computer it was made on.
    pub machine: String,
    /// Accounts to connect again.
    pub accounts: Vec<AccountRef>,
    /// Projects it knows (their files come with the repositories).
    pub projects: Vec<String>,
    /// Each part and its size.
    pub sections: Vec<SectionCount>,
    /// Restoring replaces something here.
    pub overwrites: bool,
}

/// argon2id cost: memory in KiB, passes, lanes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    /// Memory (KiB).
    pub memory_kib: u32,
    /// Passes.
    pub passes: u32,
    /// Lanes.
    pub lanes: u32,
}

impl Default for KdfParams {
    /// 64 MiB, 3 passes, 1 lane (above OWASP's argon2id minimum; about a second).
    fn default() -> Self {
        Self {
            memory_kib: 64 * 1024,
            passes: 3,
            lanes: 1,
        }
    }
}

impl KdfParams {
    /// Parameters a file may ask for (so a crafted file can't exhaust memory).
    fn acceptable(self) -> bool {
        (8..=1024 * 1024).contains(&self.memory_kib)
            && (1..=16).contains(&self.passes)
            && (1..=16).contains(&self.lanes)
    }
}

fn internal(err: impl std::fmt::Display) -> BackupError {
    BackupError::Internal(err.to_string())
}

fn derive_key(
    passphrase: &Secret<String>,
    salt: &[u8],
    params: KdfParams,
) -> Result<[u8; 32], BackupError> {
    let argon = argon2::Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        argon2::Params::new(params.memory_kib, params.passes, params.lanes, Some(32))
            .map_err(internal)?,
    );
    let mut key = [0u8; 32];
    argon
        .hash_password_into(passphrase.expose().as_bytes(), salt, &mut key)
        .map_err(internal)?;
    Ok(key)
}

fn header(params: KdfParams, salt: &[u8; 16], nonce: &[u8; 24]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN);
    out.extend_from_slice(MAGIC);
    out.push(FORMAT);
    out.push(KDF_ARGON2ID);
    out.extend_from_slice(&params.memory_kib.to_le_bytes());
    out.extend_from_slice(&params.passes.to_le_bytes());
    out.extend_from_slice(&params.lanes.to_le_bytes());
    out.extend_from_slice(salt);
    out.extend_from_slice(nonce);
    out
}

/// Encrypts `contents` with `passphrase`. Slow on purpose (argon2id): call it off the
/// async threads.
///
/// # Errors
/// [`BackupError::WeakPassphrase`], or an unexpected encoding failure.
pub fn seal(
    contents: &Contents,
    passphrase: &Secret<String>,
    params: KdfParams,
) -> Result<Vec<u8>, BackupError> {
    if passphrase.expose().chars().count() < MIN_PASSPHRASE {
        return Err(BackupError::WeakPassphrase);
    }
    let json = serde_json::to_vec(contents).map_err(internal)?;
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut gz, &json).map_err(internal)?;
    let mut plain = gz.finish().map_err(internal)?;
    let (mut salt, mut nonce) = ([0u8; 16], [0u8; 24]);
    getrandom::fill(&mut salt).map_err(internal)?;
    getrandom::fill(&mut nonce).map_err(internal)?;
    let head = header(params, &salt, &nonce);
    let mut key = derive_key(passphrase, &salt, params)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(internal);
    key.zeroize();
    let sealed = cipher?
        .encrypt(
            &nonce.into(),
            Payload {
                msg: &plain,
                aad: &head,
            },
        )
        .map_err(internal)?;
    plain.zeroize();
    let mut out = head;
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// Decrypts and checks a backup. Slow on purpose (argon2id).
///
/// # Errors
/// Not a backup, a format this version can't read, or a wrong passphrase or changed
/// file (indistinguishable by design).
pub fn open(bytes: &[u8], passphrase: &Secret<String>) -> Result<Contents, BackupError> {
    if bytes.len() < HEADER_LEN + 16 || !bytes.starts_with(MAGIC) || bytes.len() > MAX_FILE {
        return Err(BackupError::NotABackup);
    }
    let (head, sealed) = bytes.split_at(HEADER_LEN);
    let (format, kdf) = (head[16], head[17]);
    if format != FORMAT || kdf != KDF_ARGON2ID {
        return Err(BackupError::UnsupportedFormat(format));
    }
    let word = |at: usize| {
        let mut b = [0u8; 4];
        b.copy_from_slice(&head[at..at + 4]);
        u32::from_le_bytes(b)
    };
    let params = KdfParams {
        memory_kib: word(18),
        passes: word(22),
        lanes: word(26),
    };
    if !params.acceptable() {
        return Err(BackupError::WrongPassphrase);
    }
    let salt = &head[30..46];
    let mut nonce = [0u8; 24];
    nonce.copy_from_slice(&head[46..70]);
    let mut key = derive_key(passphrase, salt, params)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&key).map_err(internal);
    key.zeroize();
    let mut plain = cipher?
        .decrypt(
            &nonce.into(),
            Payload {
                msg: sealed,
                aad: head,
            },
        )
        .map_err(|_| BackupError::WrongPassphrase)?;
    let mut json = Vec::new();
    let read = std::io::Read::read_to_end(
        &mut std::io::Read::take(
            flate2::read::GzDecoder::new(plain.as_slice()),
            MAX_FILE as u64 * 4,
        ),
        &mut json,
    );
    plain.zeroize();
    read.map_err(|_| BackupError::NotABackup)?;
    serde_json::from_slice(&json).map_err(|_| BackupError::NotABackup)
}

fn quote(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

fn columns(conn: &rusqlite::Connection, table: &str) -> Result<Vec<String>, StoreError> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({})", quote(table)))?;
    let names = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(names)
}

fn to_json(value: rusqlite::types::ValueRef<'_>) -> Value {
    use rusqlite::types::ValueRef;
    match value {
        ValueRef::Null => Value::Null,
        ValueRef::Integer(n) => Value::from(n),
        ValueRef::Real(n) => Value::from(n),
        ValueRef::Text(t) => Value::String(String::from_utf8_lossy(t).into_owned()),
        // No copied table has blobs; kept as text if one ever does.
        ValueRef::Blob(b) => Value::String(String::from_utf8_lossy(b).into_owned()),
    }
}

fn to_sql(value: &Value) -> rusqlite::types::Value {
    use rusqlite::types::Value as Sql;
    match value {
        Value::Null => Sql::Null,
        Value::Bool(b) => Sql::Integer(i64::from(*b)),
        Value::Number(n) => n
            .as_i64()
            .map(Sql::Integer)
            .or_else(|| n.as_f64().map(Sql::Real))
            .unwrap_or(Sql::Null),
        Value::String(s) => Sql::Text(s.clone()),
        other => Sql::Text(other.to_string()),
    }
}

/// Whether a setting may go into a backup.
fn portable_setting(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    !LOCAL_SETTINGS.contains(&key)
        && !["secret", "token", "password", "apikey", "credential"]
            .iter()
            .any(|word| lower.contains(word))
}

fn read_all(conn: &rusqlite::Connection) -> Result<Contents, StoreError> {
    let schema: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let mut settings = BTreeMap::new();
    {
        let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (key, value) = row?;
            if portable_setting(&key)
                && let Ok(value) = serde_json::from_str(&value)
            {
                settings.insert(key, value);
            }
        }
    }
    let mut accounts = Vec::new();
    {
        let mut stmt =
            conn.prepare("SELECT id, name FROM accounts ORDER BY name COLLATE NOCASE")?;
        let rows = stmt.query_map([], |row| {
            Ok(AccountRef {
                id: row.get(0)?,
                name: row.get(1)?,
            })
        })?;
        for row in rows {
            accounts.push(row?);
        }
    }
    let mut tables = BTreeMap::new();
    for (table, cleared) in TABLES {
        let names = columns(conn, table)?;
        if names.is_empty() {
            continue;
        }
        let mut stmt = conn.prepare(&format!("SELECT * FROM {}", quote(table)))?;
        let mut rows = stmt.query([])?;
        let mut out = Vec::new();
        while let Some(row) = rows.next()? {
            let mut map = Map::new();
            for (i, name) in names.iter().enumerate() {
                let value = if cleared.contains(&name.as_str()) {
                    Value::Null
                } else if *table == "local_tunnels" && name == "run_mode" {
                    // Always-on is a service of the old computer; turn it on again here.
                    Value::String("session".into())
                } else {
                    to_json(row.get_ref(i)?)
                };
                map.insert(name.clone(), value);
            }
            out.push(map);
        }
        tables.insert((*table).to_owned(), out);
    }
    Ok(Contents {
        created_at: crate::domain_shares::now_ms(),
        app_version: env!("CARGO_PKG_VERSION").to_owned(),
        machine: String::new(),
        schema,
        accounts,
        settings,
        tables,
    })
}

/// Reads what goes into a backup from the database.
///
/// # Errors
/// Database errors.
pub async fn collect(store: &Store, machine: &str) -> Result<Contents, BackupError> {
    let mut contents = store.call(|conn| read_all(conn)).await?;
    machine.clone_into(&mut contents.machine);
    Ok(contents)
}

fn count(conn: &rusqlite::Connection, table: &str) -> Result<u32, StoreError> {
    if columns(conn, table)?.is_empty() {
        return Ok(0);
    }
    Ok(conn.query_row(
        &format!("SELECT COUNT(*) FROM {}", quote(table)),
        [],
        |row| row.get(0),
    )?)
}

/// What restoring `contents` would bring and replace.
///
/// # Errors
/// Database errors.
pub async fn summarize(store: &Store, contents: &Contents) -> Result<BackupSummary, BackupError> {
    let tables: Vec<(String, u32)> = contents
        .tables
        .iter()
        .map(|(name, rows)| (name.clone(), u32::try_from(rows.len()).unwrap_or(u32::MAX)))
        .collect();
    let settings = u32::try_from(contents.settings.len()).unwrap_or(u32::MAX);
    let sections = store
        .call(move |conn| {
            let existing_settings: u32 =
                conn.query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))?;
            let mut out = vec![SectionCount {
                section: "settings".into(),
                count: settings,
                existing: existing_settings,
            }];
            for (table, count_in_backup) in tables {
                out.push(SectionCount {
                    existing: count(conn, &table)?,
                    section: table,
                    count: count_in_backup,
                });
            }
            Ok(out)
        })
        .await?;
    let projects = contents
        .settings
        .get("projects")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|p| p.get("name").and_then(Value::as_str).map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Ok(BackupSummary {
        created_at: contents.created_at,
        app_version: contents.app_version.clone(),
        machine: contents.machine.clone(),
        accounts: contents.accounts.clone(),
        projects,
        overwrites: sections
            .iter()
            .any(|s| s.section != "settings" && s.existing > 0),
        sections,
    })
}

/// Replaces this computer's setup with the backup's, in one transaction: the copied
/// tables are emptied and filled from the backup, and its settings are written over
/// the ones here. Accounts aren't created: connect them again afterwards.
///
/// # Errors
/// [`BackupError::NewerSchema`] for a backup of a newer database; database errors
/// (nothing is changed then).
pub async fn restore(store: &Store, contents: Contents) -> Result<(), BackupError> {
    store
        .call(move |conn| {
            let schema: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
            if contents.schema > schema {
                return Ok(Err(BackupError::NewerSchema));
            }
            let tx = conn.transaction()?;
            for (key, value) in &contents.settings {
                if portable_setting(key) {
                    crate::settings::write(&tx, key, value)?;
                }
            }
            // Children after parents; emptied in the reverse order.
            let known: Vec<&str> = TABLES.iter().map(|(t, _)| *t).collect();
            for table in known.iter().rev() {
                if contents.tables.contains_key(*table) && !columns(&tx, table)?.is_empty() {
                    tx.execute(&format!("DELETE FROM {}", quote(table)), [])?;
                }
            }
            for table in &known {
                let Some(rows) = contents.tables.get(*table) else {
                    continue;
                };
                let here = columns(&tx, table)?;
                for row in rows {
                    let cols: Vec<&String> = row.keys().filter(|c| here.contains(c)).collect();
                    if cols.is_empty() {
                        continue;
                    }
                    let sql = format!(
                        "INSERT INTO {} ({}) VALUES ({})",
                        quote(table),
                        cols.iter().map(|c| quote(c)).collect::<Vec<_>>().join(", "),
                        vec!["?"; cols.len()].join(", ")
                    );
                    let values: Vec<rusqlite::types::Value> =
                        cols.iter().map(|c| to_sql(&row[c.as_str()])).collect();
                    tx.execute(&sql, rusqlite::params_from_iter(values))?;
                }
            }
            tx.commit()?;
            Ok(Ok(()))
        })
        .await?
}

/// Makes a backup file at `path` (written with owner-only permissions).
///
/// # Errors
/// See [`seal`] and [`collect`]; writing the file.
pub async fn create_file(
    store: &Store,
    machine: &str,
    path: &std::path::Path,
    passphrase: Secret<String>,
    params: KdfParams,
) -> Result<(), BackupError> {
    let contents = collect(store, machine).await?;
    let bytes = tokio::task::spawn_blocking(move || seal(&contents, &passphrase, params))
        .await
        .map_err(internal)??;
    let io = |e: std::io::Error| BackupError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    };
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).map_err(io)?;
    std::io::Write::write_all(&mut file, &bytes).map_err(io)?;
    file.sync_all().map_err(io)
}

/// Reads and decrypts a backup file.
///
/// # Errors
/// See [`open`]; reading the file.
pub async fn read_file(
    path: &std::path::Path,
    passphrase: Secret<String>,
) -> Result<Contents, BackupError> {
    let io = |e: std::io::Error| BackupError::Io {
        path: path.display().to_string(),
        detail: e.to_string(),
    };
    let size = std::fs::metadata(path).map_err(io)?.len();
    if size > MAX_FILE as u64 {
        return Err(BackupError::NotABackup);
    }
    let bytes = std::fs::read(path).map_err(io)?;
    tokio::task::spawn_blocking(move || open(&bytes, &passphrase))
        .await
        .map_err(internal)?
}

#[cfg(test)]
mod tests;
