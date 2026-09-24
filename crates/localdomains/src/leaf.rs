//! Leaf certificates, issued on demand and cached.
//!
//! Each leaf is ECDSA P-256 with a fresh key, valid for [`LEAF_VALIDITY`] (30 days), with
//! the requested name as SAN (plus `*.name` when wildcard subdomains are on). Leaves are kept
//! in memory and, optionally, on disk (certificate + key PEM, mode 0600) in a directory per
//! CA, so a restart doesn't re-issue everything and a new CA never reuses old leaves. A leaf
//! is replaced (with a new key) once it's within [`RENEW_BEFORE`] of expiry.

use std::{
    collections::HashMap,
    fmt, fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

use rcgen::{SanType, string::Ia5String};
use rustls::{
    pki_types::{CertificateDer, PrivateKeyDer, pem::PemObject},
    sign::CertifiedKey,
};
use time::{Duration, OffsetDateTime};

use crate::{
    ca::{CaError, LocalCa, pem_encode},
    clock::Clock,
    name::LocalName,
};

/// How long a leaf is valid.
pub const LEAF_VALIDITY: Duration = Duration::days(30);
/// A leaf this close to expiry is replaced.
pub const RENEW_BEFORE: Duration = Duration::days(10);
/// Backdating of `notBefore`, for small clock differences.
const BACKDATE: Duration = Duration::hours(1);

/// What certificate to serve: a name, and whether its wildcard sibling is included.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CertRequest {
    /// The certificate's primary name (subject CN and first SAN).
    pub name: LocalName,
    /// Also cover `*.name` (one level of subdomains).
    pub wildcard: bool,
}

impl CertRequest {
    /// A request for `name` alone.
    #[must_use]
    pub fn exact(name: LocalName) -> Self {
        Self {
            name,
            wildcard: false,
        }
    }

    fn sans(&self) -> Result<Vec<SanType>, rcgen::Error> {
        let mut sans = vec![SanType::DnsName(Ia5String::try_from(self.name.as_str())?)];
        if self.wildcard {
            sans.push(SanType::DnsName(Ia5String::try_from(self.name.wildcard())?));
        }
        Ok(sans)
    }

    fn file_name(&self) -> String {
        if self.wildcard {
            format!("{}+wildcard.pem", self.name)
        } else {
            format!("{}.pem", self.name)
        }
    }
}

/// A leaf failure.
#[derive(Debug, thiserror::Error)]
pub enum LeafError {
    /// Signing failed.
    #[error(transparent)]
    Ca(#[from] CaError),
    /// rustls refused the key.
    #[error("tls: {0}")]
    Tls(#[from] rustls::Error),
    /// The disk cache failed.
    #[error("leaf cache: {0}")]
    Io(#[from] io::Error),
}

impl From<rcgen::Error> for LeafError {
    fn from(err: rcgen::Error) -> Self {
        Self::Ca(CaError::Certificate(err))
    }
}

struct Cached {
    key: Arc<CertifiedKey>,
    not_after: OffsetDateTime,
}

/// Issues and caches leaves for one CA.
pub struct LeafCache {
    ca: Arc<LocalCa>,
    clock: Arc<dyn Clock>,
    dir: Option<PathBuf>,
    entries: Mutex<HashMap<CertRequest, Cached>>,
}

impl fmt::Debug for LeafCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LeafCache")
            .field("ca", &self.ca.common_name())
            .field("dir", &self.dir)
            .field("entries", &self.lock().len())
            .finish_non_exhaustive()
    }
}

impl LeafCache {
    /// A memory-only cache.
    #[must_use]
    pub fn new(ca: Arc<LocalCa>, clock: Arc<dyn Clock>) -> Self {
        Self {
            ca,
            clock,
            dir: None,
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Also keeps leaves under `root` (e.g. `<data dir>/localdomains/leaves`), in a
    /// subdirectory named after the CA's fingerprint.
    #[must_use]
    pub fn with_disk_cache(mut self, root: &Path) -> Self {
        let fingerprint = self.ca.sha256_fingerprint();
        self.dir = Some(root.join(&fingerprint[..16]));
        self
    }

    /// The CA leaves are signed by.
    #[must_use]
    pub fn ca(&self) -> &Arc<LocalCa> {
        &self.ca
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<CertRequest, Cached>> {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn is_due(&self, not_after: OffsetDateTime) -> bool {
        self.clock.now() + RENEW_BEFORE >= not_after
    }

    /// The certificate for `request`: cached if still fresh, otherwise loaded from disk or
    /// newly issued. The caller must already have checked the name is allowed.
    ///
    /// # Errors
    /// Issuing or saving the leaf failed.
    pub fn get(&self, request: &CertRequest) -> Result<Arc<CertifiedKey>, LeafError> {
        let mut entries = self.lock();
        if let Some(cached) = entries.get(request)
            && !self.is_due(cached.not_after)
        {
            return Ok(Arc::clone(&cached.key));
        }
        if let Some(cached) = self.load_from_disk(request)
            && !self.is_due(cached.not_after)
        {
            let key = Arc::clone(&cached.key);
            entries.insert(request.clone(), cached);
            return Ok(key);
        }
        let cached = self.issue(request)?;
        let key = Arc::clone(&cached.key);
        entries.insert(request.clone(), cached);
        Ok(key)
    }

    /// When the cached leaf for `request` expires, if one is cached.
    #[must_use]
    pub fn not_after(&self, request: &CertRequest) -> Option<OffsetDateTime> {
        self.lock().get(request).map(|c| c.not_after)
    }

    /// Replaces every cached leaf that is due for renewal; returns how many were replaced.
    /// Call it periodically (e.g. daily) so renewal doesn't wait for a handshake.
    ///
    /// # Errors
    /// Issuing a leaf failed; leaves renewed before the failure are kept.
    pub fn renew_due(&self) -> Result<usize, LeafError> {
        let mut entries = self.lock();
        let due: Vec<CertRequest> = entries
            .iter()
            .filter(|(_, c)| self.is_due(c.not_after))
            .map(|(r, _)| r.clone())
            .collect();
        for request in &due {
            let cached = self.issue(request)?;
            entries.insert(request.clone(), cached);
        }
        Ok(due.len())
    }

    /// Forgets the leaf for `request` (memory and disk), e.g. when a domain is removed.
    ///
    /// # Errors
    /// Deleting the file failed.
    pub fn remove(&self, request: &CertRequest) -> Result<(), LeafError> {
        self.lock().remove(request);
        if let Some(dir) = &self.dir {
            match fs::remove_file(dir.join(request.file_name())) {
                Ok(()) => {}
                Err(err) if err.kind() == io::ErrorKind::NotFound => {}
                Err(err) => return Err(err.into()),
            }
        }
        Ok(())
    }

    /// Deletes leaves left behind by previous CAs (sibling directories of this CA's).
    ///
    /// # Errors
    /// Listing or deleting failed.
    pub fn purge_other_cas(&self) -> Result<(), LeafError> {
        let Some(dir) = &self.dir else { return Ok(()) };
        let Some(root) = dir.parent() else {
            return Ok(());
        };
        let entries = match fs::read_dir(root) {
            Ok(entries) => entries,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        };
        for entry in entries {
            let path = entry?.path();
            if path != *dir && path.is_dir() {
                fs::remove_dir_all(&path)?;
            }
        }
        Ok(())
    }

    fn issue(&self, request: &CertRequest) -> Result<Cached, LeafError> {
        let now = self.clock.now();
        let signed = self.ca.sign_leaf(
            &request.sans()?,
            request.name.as_str(),
            now - BACKDATE,
            now + LEAF_VALIDITY,
        )?;
        if let Some(dir) = &self.dir {
            let pem = zeroize::Zeroizing::new(format!(
                "{}{}",
                pem_encode("CERTIFICATE", &signed.cert_der),
                pem_encode("PRIVATE KEY", signed.key_der.secret_pkcs8_der())
            ));
            if let Err(err) =
                crate::fsutil::write_private(&dir.join(request.file_name()), pem.as_bytes())
            {
                // A read-only data dir must not break TLS: serve from memory.
                tracing::warn!(error = %err, "couldn't save a local certificate");
            }
        }
        let key = CertifiedKey::from_der(
            vec![signed.cert_der.clone()],
            signed.key(),
            self.ca.provider(),
        )?;
        tracing::debug!(name = %request.name, wildcard = request.wildcard, "issued a local certificate");
        Ok(Cached {
            key: Arc::new(key),
            not_after: signed.not_after,
        })
    }

    fn load_from_disk(&self, request: &CertRequest) -> Option<Cached> {
        let path = self.dir.as_ref()?.join(request.file_name());
        let bytes = zeroize::Zeroizing::new(fs::read(&path).ok()?);
        let cert = CertificateDer::from_pem_slice(&bytes).ok()?;
        let key = PrivateKeyDer::from_pem_slice(&bytes).ok()?;
        let (_, parsed) = x509_parser::parse_x509_certificate(&cert).ok()?;
        let not_after = parsed.validity().not_after.to_datetime();
        if parsed.validity().not_before.to_datetime() > self.clock.now() {
            return None;
        }
        let sans: Vec<String> = parsed
            .subject_alternative_name()
            .ok()??
            .value
            .general_names
            .iter()
            .filter_map(|n| match n {
                x509_parser::extensions::GeneralName::DNSName(d) => Some((*d).to_owned()),
                _ => None,
            })
            .collect();
        let expected: Vec<String> = request
            .sans()
            .ok()?
            .into_iter()
            .filter_map(|s| match s {
                SanType::DnsName(d) => Some(d.as_str().to_owned()),
                _ => None,
            })
            .collect();
        if sans != expected {
            return None;
        }
        let key =
            CertifiedKey::from_der(vec![cert.into_owned()], key.clone_key(), self.ca.provider())
                .ok()?;
        Some(Cached {
            key: Arc::new(key),
            not_after,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ca::tests::{test_ca, verify},
        clock::ManualClock,
    };

    fn req(name: &str, wildcard: bool) -> CertRequest {
        CertRequest {
            name: LocalName::parse_any(name).unwrap(),
            wildcard,
        }
    }

    fn setup() -> (Arc<LocalCa>, Arc<ManualClock>) {
        let (ca, clock) = test_ca();
        (Arc::new(ca), Arc::new(clock))
    }

    fn leaf_der(key: &CertifiedKey) -> CertificateDer<'static> {
        key.cert[0].clone()
    }

    #[test]
    fn issues_verifiable_leaves_with_wildcard_sibling() {
        let (ca, clock) = setup();
        let cache = LeafCache::new(Arc::clone(&ca), clock.clone());
        let key = cache.get(&req("app.localhost", true)).unwrap();
        let der = leaf_der(&key);
        verify(&ca, &der, "app.localhost", clock.now()).unwrap();
        verify(&ca, &der, "api.app.localhost", clock.now()).unwrap();
        assert!(verify(&ca, &der, "other.localhost", clock.now()).is_err());

        let (_, parsed) = x509_parser::parse_x509_certificate(&der).unwrap();
        let validity = parsed.validity();
        assert_eq!(
            validity.not_after.to_datetime() - validity.not_before.to_datetime(),
            LEAF_VALIDITY + BACKDATE
        );
        assert!(!parsed.is_ca());

        let exact = cache.get(&req("app.localhost", false)).unwrap();
        assert!(verify(&ca, &leaf_der(&exact), "api.app.localhost", clock.now()).is_err());
    }

    #[test]
    fn caches_until_due_then_renews_with_a_new_key() {
        let (ca, clock) = setup();
        let cache = LeafCache::new(ca, clock.clone());
        let request = req("app.localhost", false);
        let first = cache.get(&request).unwrap();
        let again = cache.get(&request).unwrap();
        assert!(Arc::ptr_eq(&first, &again));

        clock.advance(LEAF_VALIDITY - RENEW_BEFORE - Duration::days(1));
        assert!(Arc::ptr_eq(&first, &cache.get(&request).unwrap()));
        assert_eq!(cache.renew_due().unwrap(), 0);

        clock.advance(Duration::days(2));
        assert_eq!(cache.renew_due().unwrap(), 1);
        let renewed = cache.get(&request).unwrap();
        assert!(!Arc::ptr_eq(&first, &renewed));
        assert_ne!(first.cert[0], renewed.cert[0]);
        assert_ne!(
            x509_parser::parse_x509_certificate(&first.cert[0])
                .unwrap()
                .1
                .public_key()
                .raw,
            x509_parser::parse_x509_certificate(&renewed.cert[0])
                .unwrap()
                .1
                .public_key()
                .raw,
            "a renewed leaf has a new key"
        );
    }

    #[test]
    fn disk_cache_survives_restarts_and_is_private() {
        let dir = tempfile::tempdir().unwrap();
        let (ca, clock) = setup();
        let request = req("app.test", true);
        let first = LeafCache::new(Arc::clone(&ca), clock.clone()).with_disk_cache(dir.path());
        let issued = first.get(&request).unwrap();
        let second = LeafCache::new(Arc::clone(&ca), clock.clone()).with_disk_cache(dir.path());
        let loaded = second.get(&request).unwrap();
        assert_eq!(issued.cert[0], loaded.cert[0]);

        let file = second.dir.as_ref().unwrap().join("app.test+wildcard.pem");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        // Expired on disk: re-issued.
        clock.advance(LEAF_VALIDITY);
        let third = LeafCache::new(Arc::clone(&ca), clock.clone()).with_disk_cache(dir.path());
        assert_ne!(third.get(&request).unwrap().cert[0], issued.cert[0]);

        // Garbage on disk: re-issued, not an error.
        fs::write(&file, "garbage").unwrap();
        let fourth = LeafCache::new(Arc::clone(&ca), clock.clone()).with_disk_cache(dir.path());
        fourth.get(&request).unwrap();

        fourth.remove(&request).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn a_new_ca_does_not_reuse_old_leaves_and_purges_them() {
        let dir = tempfile::tempdir().unwrap();
        let (old_ca, clock) = setup();
        let request = req("app.localhost", false);
        let old = LeafCache::new(old_ca, clock.clone()).with_disk_cache(dir.path());
        old.get(&request).unwrap();
        let (new_ca, _) = setup();
        let new = LeafCache::new(Arc::clone(&new_ca), clock.clone()).with_disk_cache(dir.path());
        let leaf = new.get(&request).unwrap();
        verify(&new_ca, &leaf.cert[0], "app.localhost", clock.now()).unwrap();
        new.purge_other_cas().unwrap();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
