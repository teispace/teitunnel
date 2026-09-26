//! Sign-in for the web dashboard `teitunnel serve` offers on servers: one password (argon2id) and named API keys for automation (random 256-bit,
//! stored as SHA-256 since they're high-entropy). Only hashes are stored; a key is shown
//! once, when it's made.

use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::store::{Store, StoreError};

/// The prefix of an API key, so a leaked one is recognisable (and scannable).
pub const KEY_PREFIX: &str = "ttk_";

/// Why a credential couldn't be set.
#[derive(Debug, thiserror::Error)]
pub enum WebAuthError {
    /// The database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// The password is too short.
    #[error("Use a password of at least 12 characters.")]
    WeakPassword,
    /// Hashing failed (no randomness).
    #[error("Couldn't hash the password: {0}")]
    Hash(String),
    /// A key needs a name.
    #[error("Give the API key a name.")]
    NoName,
}

/// An API key as listed (never its value).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKey {
    /// Row id (to revoke it).
    pub id: i64,
    /// What it's for.
    pub name: String,
    /// When it was made (milliseconds since the epoch).
    pub created_at: i64,
}

fn now_ms() -> i64 {
    crate::domain_shares::now_ms()
        .try_into()
        .unwrap_or(i64::MAX)
}

/// 32 random bytes, URL-safe base64.
///
/// # Errors
/// The OS random source failed.
pub fn random_token() -> Result<String, WebAuthError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| WebAuthError::Hash(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn key_hash(key: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(key.as_bytes()))
}

/// Sets (or replaces) the dashboard password.
///
/// # Errors
/// A weak password, hashing or database errors.
pub async fn set_password(store: &Store, password: &str) -> Result<(), WebAuthError> {
    if password.chars().count() < 12 {
        return Err(WebAuthError::WeakPassword);
    }
    let mut salt = [0u8; 16];
    getrandom::fill(&mut salt).map_err(|e| WebAuthError::Hash(e.to_string()))?;
    // A PHC string (`$argon2id$v=19$m=…$salt$hash`), the same format as before.
    let hash = Argon2::default()
        .hash_password_with_salt(password.as_bytes(), &salt)
        .map_err(|e| WebAuthError::Hash(e.to_string()))?
        .to_string();
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO web_credentials (kind, name, hash, created_at)
                 VALUES ('password', 'password', ?1, ?2)
                 ON CONFLICT (kind) WHERE kind = 'password' DO UPDATE SET hash = ?1, created_at = ?2",
                params![hash, now_ms()],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

/// Whether a dashboard password is set.
///
/// # Errors
/// Database errors.
pub async fn has_password(store: &Store) -> Result<bool, WebAuthError> {
    Ok(password_hash(store).await?.is_some())
}

async fn password_hash(store: &Store) -> Result<Option<String>, StoreError> {
    store
        .call(|conn| {
            Ok(conn
                .query_row(
                    "SELECT hash FROM web_credentials WHERE kind = 'password'",
                    [],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .await
}

/// Whether `password` is the dashboard password (constant-time comparison by argon2).
///
/// # Errors
/// Database errors.
pub async fn verify_password(store: &Store, password: &str) -> Result<bool, WebAuthError> {
    let Some(hash) = password_hash(store).await? else {
        return Ok(false);
    };
    let password = password.to_owned();
    // argon2 is deliberately slow: keep it off the async threads.
    Ok(tokio::task::spawn_blocking(move || {
        Argon2::default()
            .verify_password(password.as_bytes(), hash.as_str())
            .is_ok()
    })
    .await
    .unwrap_or(false))
}

/// Makes an API key called `name`. Returns the key, which isn't stored and can't be
/// shown again.
///
/// # Errors
/// No name, randomness or database errors.
pub async fn create_api_key(store: &Store, name: &str) -> Result<String, WebAuthError> {
    let name = name.trim().to_owned();
    if name.is_empty() {
        return Err(WebAuthError::NoName);
    }
    let key = format!("{KEY_PREFIX}{}", random_token()?);
    let hash = key_hash(&key);
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO web_credentials (kind, name, hash, created_at)
                 VALUES ('apiKey', ?1, ?2, ?3)",
                params![name, hash, now_ms()],
            )?;
            Ok(())
        })
        .await?;
    Ok(key)
}

/// The name of the API key `key`, if it's a valid one.
///
/// # Errors
/// Database errors.
pub async fn verify_api_key(store: &Store, key: &str) -> Result<Option<String>, WebAuthError> {
    if !key.starts_with(KEY_PREFIX) {
        return Ok(None);
    }
    let hash = key_hash(key);
    Ok(store
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT name FROM web_credentials WHERE kind = 'apiKey' AND hash = ?1",
                    params![hash],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .await?)
}

/// API keys, oldest first.
///
/// # Errors
/// Database errors.
pub async fn api_keys(store: &Store) -> Result<Vec<ApiKey>, WebAuthError> {
    Ok(store
        .call(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, name, created_at FROM web_credentials WHERE kind = 'apiKey' ORDER BY id",
            )?;
            let rows = stmt.query_map([], |row| {
                Ok(ApiKey {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await?)
}

/// Revokes an API key. Returns whether one was.
///
/// # Errors
/// Database errors.
pub async fn revoke_api_key(store: &Store, id: i64) -> Result<bool, WebAuthError> {
    Ok(store
        .call(move |conn| {
            Ok(conn.execute(
                "DELETE FROM web_credentials WHERE kind = 'apiKey' AND id = ?1",
                params![id],
            )? > 0)
        })
        .await?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn accepts_passwords_hashed_before_an_upgrade() {
        // A standard argon2id PHC string made by another implementation (argon2-cffi,
        // the parameters Teitunnel uses): what a dashboard saved with an older argon2
        // crate looks like. It must keep working after the dependency changes.
        let store = Store::open_in_memory().unwrap();
        let old = "$argon2id$v=19$m=19456,t=2,p=1$lkQBW0HPMohimeWFom+5yw$IrOECBs+fozQyBUdO0Z4My1hKPX9qX6gRNcYhqR1S/g";
        store
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO web_credentials (kind, name, hash, created_at)
                     VALUES ('password', 'password', ?1, 0)",
                    params![old],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        assert!(
            verify_password(&store, "correct horse battery staple")
                .await
                .unwrap()
        );
        assert!(
            !verify_password(&store, "correct horse battery")
                .await
                .unwrap()
        );
    }

    #[tokio::test]
    async fn passwords_are_hashed_and_checked() {
        let store = Store::open_in_memory().unwrap();
        assert!(!has_password(&store).await.unwrap());
        assert!(!verify_password(&store, "anything at all").await.unwrap());
        assert!(matches!(
            set_password(&store, "short").await,
            Err(WebAuthError::WeakPassword)
        ));
        set_password(&store, "correct horse battery").await.unwrap();
        assert!(
            verify_password(&store, "correct horse battery")
                .await
                .unwrap()
        );
        assert!(
            !verify_password(&store, "correct horse battery!")
                .await
                .unwrap()
        );
        // Replacing it keeps one password.
        set_password(&store, "another long password").await.unwrap();
        assert!(
            !verify_password(&store, "correct horse battery")
                .await
                .unwrap()
        );
        assert!(
            verify_password(&store, "another long password")
                .await
                .unwrap()
        );
        // Only a hash is stored.
        let stored: String = store
            .call(|c| {
                Ok(c.query_row(
                    "SELECT hash FROM web_credentials WHERE kind='password'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .await
            .unwrap();
        assert!(stored.starts_with("$argon2id$"), "{stored}");
    }

    #[tokio::test]
    async fn api_keys_are_shown_once_and_revocable() {
        let store = Store::open_in_memory().unwrap();
        let key = create_api_key(&store, "deploy").await.unwrap();
        assert!(key.starts_with(KEY_PREFIX) && key.len() > 40);
        assert_eq!(
            verify_api_key(&store, &key).await.unwrap().as_deref(),
            Some("deploy")
        );
        assert_eq!(verify_api_key(&store, "ttk_guess").await.unwrap(), None);
        assert_eq!(verify_api_key(&store, "no-prefix").await.unwrap(), None);
        let keys = api_keys(&store).await.unwrap();
        assert_eq!(keys.len(), 1);
        assert!(revoke_api_key(&store, keys[0].id).await.unwrap());
        assert_eq!(verify_api_key(&store, &key).await.unwrap(), None);
        assert!(matches!(
            create_api_key(&store, " ").await,
            Err(WebAuthError::NoName)
        ));
    }
}
