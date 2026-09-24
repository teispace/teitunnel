//! The local CA's key in the OS keychain, through core's [`SecretStore`] port.

use localdomains::{CA_KEYCHAIN_ACCOUNT, CaKeyStore, CaSecret, KeyStoreError};

use crate::{Secret, secrets::Secrets};

/// Keeps the CA bundle in the keychain (service `com.teispace.teitunnel`, account
/// [`CA_KEYCHAIN_ACCOUNT`]). Calls block (the OS may prompt): use a blocking thread.
#[derive(Debug, Clone)]
pub struct KeychainCaStore {
    secrets: Secrets,
}

impl KeychainCaStore {
    /// A store over `secrets`.
    pub fn new(secrets: Secrets) -> Self {
        Self { secrets }
    }
}

fn keychain(err: impl std::fmt::Display) -> KeyStoreError {
    KeyStoreError::Keychain(err.to_string())
}

impl CaKeyStore for KeychainCaStore {
    fn load(&self) -> Result<Option<CaSecret>, KeyStoreError> {
        Ok(self
            .secrets
            .get(CA_KEYCHAIN_ACCOUNT)
            .map_err(keychain)?
            .map(|secret| CaSecret::new(secret.expose().clone())))
    }

    fn save(&self, secret: &CaSecret) -> Result<(), KeyStoreError> {
        self.secrets
            .set(
                CA_KEYCHAIN_ACCOUNT,
                &Secret::new(secret.expose().to_owned()),
            )
            .map_err(keychain)
    }

    fn delete(&self) -> Result<(), KeyStoreError> {
        self.secrets.delete(CA_KEYCHAIN_ACCOUNT).map_err(keychain)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::secrets::MemoryStore;

    #[test]
    fn round_trips_through_the_secret_store_under_its_account() {
        let memory = MemoryStore::default();
        let store = KeychainCaStore::new(Arc::new(memory.clone()));
        assert!(store.load().unwrap().is_none());
        store.save(&CaSecret::new("PEM".into())).unwrap();
        assert_eq!(memory.keys(), vec![CA_KEYCHAIN_ACCOUNT.to_owned()]);
        assert_eq!(store.load().unwrap().unwrap().expose(), "PEM");
        store.delete().unwrap();
        store.delete().unwrap();
        assert!(memory.keys().is_empty());
    }
}
