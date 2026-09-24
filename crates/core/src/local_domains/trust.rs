//! Who trusts the local CA: the OS and browser stores through `localdomains`'
//! installers, or (for tests and scripted checks) a file that stands in for them.

use std::{
    fmt,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
};

use localdomains::{
    CaCert, InstallOptions, Platform, StoreState, StoreStatus, SystemRunner, TrustEnv,
    TrustManager, TrustReport, TrustStore,
};

/// A boxed future (the trait is object-safe).
pub type BoxFuture<'a, T> = std::pin::Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Installs, removes and checks the CA's trust. [`SystemTrust`] changes the real stores
/// (the OS may ask for a password); [`FileTrust`] only writes a file.
pub trait TrustBackend: fmt::Debug + Send + Sync {
    /// The platform whose stores these are.
    fn platform(&self) -> Platform;

    /// Every store's state for `cert`.
    fn status<'a>(&'a self, cert: &'a CaCert) -> BoxFuture<'a, Vec<StoreStatus>>;

    /// Trusts `cert` wherever the user may; returns the steps needing an administrator.
    fn install<'a>(
        &'a self,
        cert: &'a CaCert,
        options: InstallOptions,
    ) -> BoxFuture<'a, TrustReport>;

    /// Stops trusting `cert`; returns the steps needing an administrator.
    fn uninstall<'a>(&'a self, cert: &'a CaCert) -> BoxFuture<'a, TrustReport>;
}

/// The real trust stores of this computer.
#[derive(Debug, Clone)]
pub struct SystemTrust {
    env: TrustEnv,
}

impl SystemTrust {
    /// The stores of the current user; `None` without a home folder. `work_dir` holds
    /// temporary files (Teitunnel's own folder).
    pub fn detect(work_dir: PathBuf) -> Option<Self> {
        TrustEnv::detect(work_dir).map(|env| Self { env })
    }

    fn manager(&self, cert: &CaCert) -> TrustManager<SystemRunner> {
        TrustManager::new(self.env.clone(), cert.clone(), SystemRunner)
    }
}

impl TrustBackend for SystemTrust {
    fn platform(&self) -> Platform {
        self.env.platform
    }

    fn status<'a>(&'a self, cert: &'a CaCert) -> BoxFuture<'a, Vec<StoreStatus>> {
        Box::pin(async move { self.manager(cert).status().await })
    }

    fn install<'a>(
        &'a self,
        cert: &'a CaCert,
        options: InstallOptions,
    ) -> BoxFuture<'a, TrustReport> {
        Box::pin(async move { self.manager(cert).install(options).await })
    }

    fn uninstall<'a>(&'a self, cert: &'a CaCert) -> BoxFuture<'a, TrustReport> {
        Box::pin(async move { self.manager(cert).uninstall().await })
    }
}

/// Stands in for the trust stores: the fingerprints it "trusts" are kept in a file
/// (or only in memory). Selected with `TEITUNNEL_TEST_TRUST_FILE`, so end-to-end tests
/// never touch the real stores; nothing is trusted by the system.
#[derive(Debug, Clone)]
pub struct FileTrust {
    path: Option<PathBuf>,
    memory: Arc<Mutex<Vec<String>>>,
    platform: Platform,
}

impl FileTrust {
    /// Keeps the list in `path` (created when first trusted).
    pub fn new(path: PathBuf) -> Self {
        Self {
            path: Some(path),
            memory: Arc::default(),
            platform: Platform::current(),
        }
    }

    /// Keeps the list in memory.
    pub fn in_memory() -> Self {
        Self {
            path: None,
            memory: Arc::default(),
            platform: Platform::current(),
        }
    }

    fn read(&self) -> Vec<String> {
        match &self.path {
            Some(path) => std::fs::read_to_string(path)
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .unwrap_or_default(),
            None => self
                .memory
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
        }
    }

    fn write(&self, trusted: Vec<String>) {
        match &self.path {
            Some(path) => {
                let text = serde_json::to_string(&trusted).unwrap_or_default();
                if let Err(err) = std::fs::write(path, text) {
                    tracing::warn!(%err, "couldn't write the test trust file");
                }
            }
            None => *self.memory.lock().unwrap_or_else(PoisonError::into_inner) = trusted,
        }
    }

    fn store(&self) -> TrustStore {
        match self.platform {
            Platform::Macos => TrustStore::MacosLoginKeychain,
            Platform::Windows => TrustStore::WindowsCurrentUserRoot,
            Platform::Linux => TrustStore::LinuxSystem { flavor: None },
        }
    }

    fn statuses(&self, cert: &CaCert) -> Vec<StoreStatus> {
        let state = if self.read().contains(&cert.sha1) {
            StoreState::Trusted
        } else {
            StoreState::Absent
        };
        vec![StoreStatus {
            store: self.store(),
            state,
        }]
    }
}

impl TrustBackend for FileTrust {
    fn platform(&self) -> Platform {
        self.platform
    }

    fn status<'a>(&'a self, cert: &'a CaCert) -> BoxFuture<'a, Vec<StoreStatus>> {
        Box::pin(async move { self.statuses(cert) })
    }

    fn install<'a>(
        &'a self,
        cert: &'a CaCert,
        _options: InstallOptions,
    ) -> BoxFuture<'a, TrustReport> {
        Box::pin(async move {
            let mut trusted = self.read();
            if !trusted.contains(&cert.sha1) {
                trusted.push(cert.sha1.clone());
                self.write(trusted);
            }
            TrustReport {
                stores: self.statuses(cert),
                privileged: Vec::new(),
            }
        })
    }

    fn uninstall<'a>(&'a self, cert: &'a CaCert) -> BoxFuture<'a, TrustReport> {
        Box::pin(async move {
            let trusted = self
                .read()
                .into_iter()
                .filter(|s| *s != cert.sha1)
                .collect();
            self.write(trusted);
            TrustReport {
                stores: self.statuses(cert),
                privileged: Vec::new(),
            }
        })
    }
}

/// The OS store's state among `stores` is "trusted".
pub fn system_trusted(stores: &[StoreStatus]) -> bool {
    stores.iter().any(|s| {
        matches!(
            s.store,
            TrustStore::MacosLoginKeychain
                | TrustStore::WindowsCurrentUserRoot
                | TrustStore::LinuxSystem { .. }
        ) && s.state == StoreState::Trusted
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cert() -> CaCert {
        CaCert {
            common_name: "Teitunnel Local CA (a@b)".into(),
            sha1: "AB".into(),
            pem: String::new(),
            path: PathBuf::from("/nonexistent/ca.pem"),
        }
    }

    #[tokio::test]
    async fn file_trust_records_and_forgets_fingerprints() {
        let dir = tempfile::tempdir().unwrap();
        let trust = FileTrust::new(dir.path().join("trust.json"));
        let cert = cert();
        assert!(!system_trusted(&trust.status(&cert).await));
        let report = trust.install(&cert, InstallOptions::default()).await;
        assert!(system_trusted(&report.stores));
        // Another instance over the same file (another process) agrees.
        let again = FileTrust::new(dir.path().join("trust.json"));
        assert!(system_trusted(&again.status(&cert).await));
        let report = again.uninstall(&cert).await;
        assert!(!system_trusted(&report.stores));
        assert!(!system_trusted(&trust.status(&cert).await));
    }
}
