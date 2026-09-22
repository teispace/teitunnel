//! Secret storage port. Secrets live only in the OS keychain (macOS Keychain, Windows
//! Credential Manager, Secret Service on Linux); nothing here writes them to disk or
//! logs. Keys follow ARCHITECTURE §6: service `com.teispace.teitunnel`, account
//! `cf:<account-id>:<kind>` or `tunnel:<tunnel-id>`.

use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

use crate::Secret;

/// Keychain service name (D-018).
pub const SERVICE: &str = "com.teispace.teitunnel";

/// Errors from the secret store.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The keychain refused or failed (locked, denied, unavailable).
    #[error("the keychain couldn't be used: {0}")]
    Keychain(String),
}

/// Where secrets are kept. Calls may block (the OS can show a prompt), so async code
/// should call them from `spawn_blocking`.
pub trait SecretStore: fmt::Debug + Send + Sync {
    /// Stores (or replaces) a secret.
    ///
    /// # Errors
    /// The keychain refused the write.
    fn set(&self, key: &str, value: &Secret<String>) -> Result<(), SecretError>;

    /// Reads a secret; `None` if there's no such item.
    ///
    /// # Errors
    /// The keychain refused the read.
    fn get(&self, key: &str) -> Result<Option<Secret<String>>, SecretError>;

    /// Deletes a secret; deleting a missing item is not an error.
    ///
    /// # Errors
    /// The keychain refused the deletion.
    fn delete(&self, key: &str) -> Result<(), SecretError>;
}

/// Runs a keychain call off the async runtime (the OS may block on a prompt).
///
/// # Errors
/// What `f` returns, or [`SecretError::Keychain`] if the task panicked.
pub(crate) async fn spawn_blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, SecretError> + Send + 'static,
) -> Result<T, SecretError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|err| SecretError::Keychain(err.to_string()))?
}

/// Shared handle to a secret store.
pub type Secrets = Arc<dyn SecretStore>;

/// The OS keychain.
#[derive(Debug, Default)]
pub struct KeychainStore;

impl KeychainStore {
    fn entry(key: &str) -> Result<keyring::Entry, SecretError> {
        keyring::Entry::new(SERVICE, key).map_err(|err| SecretError::Keychain(err.to_string()))
    }
}

impl SecretStore for KeychainStore {
    fn set(&self, key: &str, value: &Secret<String>) -> Result<(), SecretError> {
        Self::entry(key)?
            .set_password(value.expose())
            .map_err(|err| SecretError::Keychain(err.to_string()))
    }

    fn get(&self, key: &str) -> Result<Option<Secret<String>>, SecretError> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(SecretError::Keychain(err.to_string())),
        }
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(SecretError::Keychain(err.to_string())),
        }
    }
}

/// An in-memory store for tests.
#[derive(Debug, Default, Clone)]
pub struct MemoryStore {
    items: Arc<Mutex<HashMap<String, String>>>,
}

impl MemoryStore {
    /// Keys currently stored (tests assert that sign-out removes everything).
    pub fn keys(&self) -> Vec<String> {
        let mut keys: Vec<_> = self.lock().keys().cloned().collect();
        keys.sort();
        keys
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, String>> {
        self.items
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl SecretStore for MemoryStore {
    fn set(&self, key: &str, value: &Secret<String>) -> Result<(), SecretError> {
        self.lock().insert(key.to_owned(), value.expose().clone());
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Secret<String>>, SecretError> {
        Ok(self.lock().get(key).cloned().map(Secret::new))
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        self.lock().remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trips() {
        let store = MemoryStore::default();
        store.set("cf:a:token", &Secret::new("s".into())).unwrap();
        assert_eq!(store.get("cf:a:token").unwrap().unwrap().expose(), "s");
        store.delete("cf:a:token").unwrap();
        store.delete("cf:a:token").unwrap();
        assert!(store.get("cf:a:token").unwrap().is_none());
    }

    /// Real keychain round trip; ignored by default (may prompt, needs a login keychain).
    #[test]
    #[ignore = "touches the real OS keychain"]
    fn keychain_round_trips() {
        let store = KeychainStore;
        let key = format!("test:{}", uuid::Uuid::new_v4());
        store.set(&key, &Secret::new("value".into())).unwrap();
        assert_eq!(store.get(&key).unwrap().unwrap().expose(), "value");
        store.delete(&key).unwrap();
        assert!(store.get(&key).unwrap().is_none());
    }
}
