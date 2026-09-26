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

use crate::text::{Text, UserText, english_display, msg};

#[cfg(target_os = "macos")]
#[allow(unsafe_code)] // The keychain's access-list API.
mod macos;

/// Keychain service name.
pub const SERVICE: &str = "com.teispace.teitunnel";

/// Errors from the secret store.
#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// The keychain refused or failed (locked, denied).
    Keychain(String),
    /// There's no credential store at all: on Linux, no Secret Service is running.
    Unavailable,
}

impl UserText for SecretError {
    fn text(&self) -> Text {
        match self {
            Self::Keychain(detail) => msg::error::secret::keychain(detail),
            Self::Unavailable => msg::error::secret::unavailable(),
        }
    }
}

english_display!(SecretError);

impl From<keyring::Error> for SecretError {
    fn from(err: keyring::Error) -> Self {
        match err {
            keyring::Error::NoDefaultStore => Self::Unavailable,
            other => Self::Keychain(other.to_string()),
        }
    }
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
        Ok(keyring::Entry::new(SERVICE, key)?)
    }
}

impl SecretStore for KeychainStore {
    fn set(&self, key: &str, value: &Secret<String>) -> Result<(), SecretError> {
        // A new item is shared with the other Teitunnel programs; an existing one keeps
        // its access list when its value changes.
        #[cfg(target_os = "macos")]
        if macos::add(SERVICE, key, value.expose().as_bytes(), shared_with())
            .map_err(|status| SecretError::Keychain(format!("OSStatus {status}")))?
            == macos::Added::Yes
        {
            return Ok(());
        }
        Ok(Self::entry(key)?.set_password(value.expose())?)
    }

    fn get(&self, key: &str) -> Result<Option<Secret<String>>, SecretError> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(Secret::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn delete(&self, key: &str) -> Result<(), SecretError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

/// The other Teitunnel programs on this Mac, found once per process.
#[cfg(target_os = "macos")]
fn shared_with() -> &'static [std::path::PathBuf] {
    static PROGRAMS: std::sync::OnceLock<Vec<std::path::PathBuf>> = std::sync::OnceLock::new();
    PROGRAMS.get_or_init(|| {
        let current = std::env::current_exe()
            .and_then(std::fs::canonicalize)
            .unwrap_or_default();
        macos::programs(&current, std::env::home_dir().as_deref())
    })
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

    #[test]
    fn a_missing_store_says_what_to_do() {
        let err = SecretError::from(keyring::Error::NoDefaultStore);
        assert!(matches!(err, SecretError::Unavailable));
        assert!(err.to_string().contains("Secret Service"));
        assert!(matches!(
            SecretError::from(keyring::Error::NoStorageAccess("locked".into())),
            SecretError::Keychain(_)
        ));
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
