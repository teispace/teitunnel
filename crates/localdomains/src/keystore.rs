//! Where the CA's private key lives.
//!
//! In the app the key is kept in the OS keychain: `core` implements [`CaKeyStore`] over its
//! `SecretStore` (service `com.teispace.teitunnel`, account [`CA_KEYCHAIN_ACCOUNT`]). This
//! crate can't depend on `core`, so it only defines the port, plus [`FileKeyStore`] for tests
//! and headless machines where the caller explicitly chooses a plain file, and
//! [`MemoryKeyStore`] for tests.

use std::{
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::Mutex,
};

use zeroize::Zeroizing;

/// Keychain account name for the CA bundle (key + certificate, PEM).
pub const CA_KEYCHAIN_ACCOUNT: &str = "localdomains:ca";

/// The CA's secret bundle: its certificate and PKCS#8 private key, both PEM. Redacted in
/// `Debug` and wiped from memory on drop.
#[derive(Clone)]
pub struct CaSecret(Zeroizing<String>);

impl CaSecret {
    /// Wraps a PEM bundle read from a store.
    #[must_use]
    pub fn new(pem_bundle: String) -> Self {
        Self(Zeroizing::new(pem_bundle))
    }

    /// The PEM text, for writing to the store. Never log it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for CaSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CaSecret([redacted])")
    }
}

/// A key store failure.
#[derive(Debug, thiserror::Error)]
pub enum KeyStoreError {
    /// The keychain refused or failed (locked, denied, unavailable).
    #[error("keychain: {0}")]
    Keychain(String),
    /// Reading or writing the file store failed.
    #[error("key file: {0}")]
    Io(#[from] io::Error),
}

/// Storage for the CA secret. Calls may block (the OS can show a prompt): from async code,
/// call them on a blocking thread.
pub trait CaKeyStore: fmt::Debug + Send + Sync {
    /// Reads the bundle; `None` if none was saved.
    ///
    /// # Errors
    /// The store refused the read.
    fn load(&self) -> Result<Option<CaSecret>, KeyStoreError>;

    /// Saves (or replaces) the bundle.
    ///
    /// # Errors
    /// The store refused the write.
    fn save(&self, secret: &CaSecret) -> Result<(), KeyStoreError>;

    /// Deletes the bundle; deleting a missing bundle is not an error.
    ///
    /// # Errors
    /// The store refused the deletion.
    fn delete(&self) -> Result<(), KeyStoreError>;
}

/// In-memory store, for tests.
#[derive(Debug, Default)]
pub struct MemoryKeyStore(Mutex<Option<CaSecret>>);

impl MemoryKeyStore {
    fn slot(&self) -> std::sync::MutexGuard<'_, Option<CaSecret>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl CaKeyStore for MemoryKeyStore {
    fn load(&self) -> Result<Option<CaSecret>, KeyStoreError> {
        Ok(self.slot().clone())
    }

    fn save(&self, secret: &CaSecret) -> Result<(), KeyStoreError> {
        *self.slot() = Some(secret.clone());
        Ok(())
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        *self.slot() = None;
        Ok(())
    }
}

/// A plain file (mode 0600 on Unix). **The key is not encrypted**: use it only for tests
/// or a headless machine without a keychain, and only when the user chose it.
#[derive(Debug, Clone)]
pub struct FileKeyStore {
    path: PathBuf,
}

impl FileKeyStore {
    /// A store at `path`. Logs a warning, since the key will sit on disk unencrypted.
    #[must_use]
    pub fn new_unencrypted(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        tracing::warn!(
            path = %path.display(),
            "local CA key stored in a plain file (0600), not the OS keychain"
        );
        Self { path }
    }

    /// The file's path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl CaKeyStore for FileKeyStore {
    fn load(&self) -> Result<Option<CaSecret>, KeyStoreError> {
        match fs::read_to_string(&self.path) {
            Ok(text) => Ok(Some(CaSecret::new(text))),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    fn save(&self, secret: &CaSecret) -> Result<(), KeyStoreError> {
        crate::fsutil::write_private(&self.path, secret.expose().as_bytes())?;
        Ok(())
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(err.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_debug_is_redacted() {
        let secret = CaSecret::new("-----BEGIN PRIVATE KEY-----".into());
        assert_eq!(format!("{secret:?}"), "CaSecret([redacted])");
    }

    #[test]
    fn file_store_round_trip_with_private_mode() {
        let dir = tempfile::tempdir().unwrap();
        let store = FileKeyStore::new_unencrypted(dir.path().join("ca/key.pem"));
        assert!(store.load().unwrap().is_none());
        store.save(&CaSecret::new("pem".into())).unwrap();
        assert_eq!(store.load().unwrap().unwrap().expose(), "pem");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(store.path()).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        store.delete().unwrap();
        store.delete().unwrap();
        assert!(store.load().unwrap().is_none());
    }
}
