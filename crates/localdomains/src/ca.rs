//! Teitunnel's local root CA.
//!
//! An ECDSA P-256 root, valid for ten years, with a **critical NameConstraints** extension
//! that permits only the special-use names Teitunnel serves (`localhost`, `test`, `local`
//! and everything below them) and loopback/private IP ranges. Verifiers that honour
//! constraints on trust anchors (Chrome 112+, Firefox, Apple, Windows) then refuse any
//! certificate this CA signs for a real site, so a stolen key can't be used to intercept
//! `bank.com`, unlike an unconstrained development CA.
//!
//! DNS constraints are written in the RFC 5280 form (`localhost`, which matches the name
//! and every name below it), not the leading-dot form, which RFC 5280 only defines for URIs
//! and mail addresses and which some verifiers reject as malformed.

use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::Path,
    sync::Arc,
};

use rcgen::{
    BasicConstraints, CertificateParams, CidrSubnet, DistinguishedName, DnType,
    ExtendedKeyUsagePurpose, GeneralSubtree, IsCa, Issuer, KeyPair, KeyUsagePurpose,
    NameConstraints, PKCS_ECDSA_P256_SHA256, PublicKeyData, SanType, SerialNumber,
};
use rustls::{
    crypto::CryptoProvider,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject},
};
use sha1::Sha1;
use sha2::{Digest, Sha256};
use time::{Duration, OffsetDateTime};
use zeroize::Zeroizing;

use crate::{
    clock::Clock,
    keystore::{CaKeyStore, CaSecret, KeyStoreError},
    name::Suffix,
};

/// How long the root is valid.
pub const CA_VALIDITY: Duration = Duration::days(3650);
/// Backdating of `notBefore`, to tolerate small clock differences between machines.
const BACKDATE: Duration = Duration::hours(1);

/// IP ranges the CA may sign for: loopback and private (RFC 1918, RFC 4193) networks.
pub const PERMITTED_IP_RANGES: [(IpAddr, u8); 6] = [
    (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 0)), 8),
    (IpAddr::V4(Ipv4Addr::new(10, 0, 0, 0)), 8),
    (IpAddr::V4(Ipv4Addr::new(172, 16, 0, 0)), 12),
    (IpAddr::V4(Ipv4Addr::new(192, 168, 0, 0)), 16),
    (IpAddr::V6(Ipv6Addr::LOCALHOST), 128),
    (IpAddr::V6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0)), 7),
];

/// A CA failure.
#[derive(Debug, thiserror::Error)]
pub enum CaError {
    /// Generating, signing or parsing a certificate failed.
    #[error("certificate: {0}")]
    Certificate(#[from] rcgen::Error),
    /// The stored bundle is missing its certificate or key, or they don't parse.
    #[error("the stored local CA is damaged: {0}")]
    Corrupt(&'static str),
    /// The key store failed.
    #[error(transparent)]
    Store(#[from] KeyStoreError),
    /// rustls refused a key or certificate.
    #[error("tls: {0}")]
    Tls(#[from] rustls::Error),
    /// Writing the public certificate failed.
    #[error("writing the certificate: {0}")]
    Io(#[from] std::io::Error),
}

/// Who the CA belongs to, shown in its name so users can tell machines' CAs apart in
/// certificate managers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaIdentity {
    /// Login name.
    pub user: String,
    /// Host name.
    pub host: String,
}

impl CaIdentity {
    /// The current user and host, from the environment and the OS.
    #[must_use]
    pub fn current() -> Self {
        let user = ["USER", "USERNAME", "LOGNAME"]
            .iter()
            .find_map(|var| std::env::var(var).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| "user".to_owned());
        let host = sysinfo::System::host_name()
            .filter(|h| !h.is_empty())
            .unwrap_or_else(|| "localhost".to_owned());
        Self { user, host }
    }

    /// `Teitunnel Local CA (user@host)`.
    #[must_use]
    pub fn common_name(&self) -> String {
        let host = self.host.split('.').next().unwrap_or(&self.host);
        format!("Teitunnel Local CA ({}@{host})", self.user)
    }
}

/// The crypto provider used for every TLS object this crate builds (AWS-LC, the same
/// backend `reqwest` already uses in this workspace). Passing it explicitly keeps us
/// independent of the process-wide default provider.
#[must_use]
pub fn crypto_provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

/// A loaded local CA: its certificate and signing key.
pub struct LocalCa {
    cert_der: CertificateDer<'static>,
    cert_pem: String,
    common_name: String,
    not_after: OffsetDateTime,
    issuer: Issuer<'static, KeyPair>,
    provider: Arc<CryptoProvider>,
}

impl fmt::Debug for LocalCa {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalCa")
            .field("common_name", &self.common_name)
            .field("sha256", &self.sha256_fingerprint())
            .field("not_after", &self.not_after)
            .finish_non_exhaustive()
    }
}

impl LocalCa {
    /// Generates a new CA and returns it with the secret bundle to store.
    ///
    /// # Errors
    /// Key generation or signing failed.
    pub fn generate(identity: &CaIdentity, clock: &dyn Clock) -> Result<(Self, CaSecret), CaError> {
        let provider = crypto_provider();
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
        let now = clock.now();
        let mut params = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, identity.common_name());
        dn.push(DnType::OrganizationName, "Teitunnel");
        dn.push(DnType::OrganizationalUnitName, "Local development only");
        params.distinguished_name = dn;
        params.serial_number = Some(random_serial(&provider)?);
        params.not_before = now - BACKDATE;
        params.not_after = now + CA_VALIDITY;
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
        params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        params.name_constraints = Some(name_constraints());
        let cert = params.self_signed(&key)?;
        let bundle = Zeroizing::new(format!("{}{}", cert.pem(), key.serialize_pem()));
        let secret = CaSecret::new(bundle.to_string());
        let ca = Self::from_parts(cert.der().clone(), key, provider)?;
        Ok((ca, secret))
    }

    /// Loads a CA from its secret bundle.
    ///
    /// # Errors
    /// [`CaError::Corrupt`] if the bundle doesn't hold a certificate and a P-256 key.
    pub fn from_secret(secret: &CaSecret) -> Result<Self, CaError> {
        let bytes = secret.expose().as_bytes();
        let cert = CertificateDer::from_pem_slice(bytes)
            .map_err(|_| CaError::Corrupt("no certificate"))?;
        let key_der = PrivatePkcs8KeyDer::from_pem_slice(bytes)
            .map_err(|_| CaError::Corrupt("no private key"))?;
        let key = KeyPair::from_pkcs8_der_and_sign_algo(&key_der, &PKCS_ECDSA_P256_SHA256)
            .map_err(|_| CaError::Corrupt("unsupported private key"))?;
        Self::from_parts(cert, key, crypto_provider())
    }

    /// Loads the CA from `store`, or generates and saves a new one if there is none (or the
    /// stored one expires within `renew_within`). Blocks on the store: call it from a blocking
    /// thread in async code.
    ///
    /// Returns the CA and whether it was newly created (a new CA must be trusted again).
    ///
    /// # Errors
    /// The store failed, or the stored bundle is damaged.
    pub fn load_or_create(
        store: &dyn CaKeyStore,
        identity: &CaIdentity,
        clock: &dyn Clock,
        renew_within: Duration,
    ) -> Result<(Self, bool), CaError> {
        if let Some(secret) = store.load()? {
            let ca = Self::from_secret(&secret)?;
            if ca.not_after - clock.now() > renew_within {
                return Ok((ca, false));
            }
            tracing::info!("local CA expires soon; generating a new one");
        }
        let (ca, secret) = Self::generate(identity, clock)?;
        store.save(&secret)?;
        Ok((ca, true))
    }

    fn from_parts(
        cert_der: CertificateDer<'static>,
        key: KeyPair,
        provider: Arc<CryptoProvider>,
    ) -> Result<Self, CaError> {
        let (_, parsed) = x509_parser::parse_x509_certificate(&cert_der)
            .map_err(|_| CaError::Corrupt("certificate doesn't parse"))?;
        if parsed.public_key().raw != key.subject_public_key_info().as_slice() {
            return Err(CaError::Corrupt("key doesn't match the certificate"));
        }
        let common_name = parsed
            .subject()
            .iter_common_name()
            .next()
            .and_then(|cn| cn.as_str().ok())
            .unwrap_or_default()
            .to_owned();
        let not_after = parsed.validity().not_after.to_datetime();
        let cert_pem = pem_encode("CERTIFICATE", &cert_der);
        let issuer = Issuer::from_ca_cert_der(&cert_der, key)?;
        Ok(Self {
            cert_der,
            cert_pem,
            common_name,
            not_after,
            issuer,
            provider,
        })
    }

    /// The certificate, DER.
    #[must_use]
    pub fn cert_der(&self) -> &CertificateDer<'static> {
        &self.cert_der
    }

    /// The certificate, PEM.
    #[must_use]
    pub fn cert_pem(&self) -> &str {
        &self.cert_pem
    }

    /// The subject common name (`Teitunnel Local CA (user@host)`), also used as the nickname
    /// in NSS databases.
    #[must_use]
    pub fn common_name(&self) -> &str {
        &self.common_name
    }

    /// When the CA expires.
    #[must_use]
    pub fn not_after(&self) -> OffsetDateTime {
        self.not_after
    }

    /// SHA-256 of the certificate, uppercase hex.
    #[must_use]
    pub fn sha256_fingerprint(&self) -> String {
        hex_upper(&Sha256::digest(self.cert_der.as_ref()))
    }

    /// SHA-1 of the certificate, uppercase hex (how macOS trust settings and Windows
    /// `certutil` identify certificates).
    #[must_use]
    pub fn sha1_fingerprint(&self) -> String {
        hex_upper(&Sha1::digest(self.cert_der.as_ref()))
    }

    /// Writes the public certificate (PEM, mode 0644), e.g. to the data directory for the
    /// trust installers.
    ///
    /// # Errors
    /// The write failed.
    pub fn write_cert_pem(&self, path: &Path) -> Result<(), CaError> {
        crate::fsutil::write_with_mode(path, self.cert_pem.as_bytes(), 0o644)?;
        Ok(())
    }

    pub(crate) fn provider(&self) -> &Arc<CryptoProvider> {
        &self.provider
    }

    /// Signs a server leaf for `sans` without checking them against any policy. Callers
    /// check names first ([`crate::leaf::LeafCache`]); tests use it to prove the name
    /// constraints hold even when policy is bypassed.
    pub(crate) fn sign_leaf(
        &self,
        sans: &[SanType],
        common_name: &str,
        not_before: OffsetDateTime,
        not_after: OffsetDateTime,
    ) -> Result<SignedLeaf, CaError> {
        let key = KeyPair::generate_for(&PKCS_ECDSA_P256_SHA256)?;
        let mut params = CertificateParams::default();
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, common_name);
        params.distinguished_name = dn;
        params.subject_alt_names = sans.to_vec();
        params.serial_number = Some(random_serial(&self.provider)?);
        params.not_before = not_before;
        params.not_after = not_after.min(self.not_after);
        params.is_ca = IsCa::ExplicitNoCa;
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        let cert = params.signed_by(&key, &self.issuer)?;
        Ok(SignedLeaf {
            cert_der: cert.der().clone(),
            key_der: PrivatePkcs8KeyDer::from(key.serialize_der()),
            not_after: params.not_after,
        })
    }
}

/// A freshly signed leaf.
pub(crate) struct SignedLeaf {
    pub(crate) cert_der: CertificateDer<'static>,
    pub(crate) key_der: PrivatePkcs8KeyDer<'static>,
    pub(crate) not_after: OffsetDateTime,
}

impl SignedLeaf {
    pub(crate) fn key(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(self.key_der.clone_key())
    }
}

/// The constraint set: DNS subtrees for each special-use suffix plus the IP ranges.
fn name_constraints() -> NameConstraints {
    let dns = Suffix::ALL
        .iter()
        .map(|suffix| GeneralSubtree::DnsName(suffix.as_str().to_owned()));
    let ips = PERMITTED_IP_RANGES.iter().map(|(addr, prefix)| {
        GeneralSubtree::IpAddress(CidrSubnet::from_addr_prefix(*addr, *prefix))
    });
    NameConstraints {
        permitted_subtrees: dns.chain(ips).collect(),
        excluded_subtrees: Vec::new(),
    }
}

/// A positive 16-byte random serial (RFC 5280 §4.1.2.2: at most 20 octets, positive).
fn random_serial(provider: &CryptoProvider) -> Result<SerialNumber, CaError> {
    let mut bytes = [0_u8; 16];
    provider
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| CaError::Tls(rustls::Error::FailedToGetRandomBytes))?;
    bytes[0] &= 0x7f;
    bytes[0] |= 0x01;
    Ok(SerialNumber::from_slice(&bytes))
}

pub(crate) fn pem_encode(label: &str, der: &[u8]) -> String {
    const LINE: usize = 64;
    let b64 = base64_encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in b64.as_bytes().chunks(LINE) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push('\n');
    }
    out.push_str(&format!("-----END {label}-----\n"));
    out
}

fn base64_encode(bytes: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn hex_upper(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
            let _ = write!(s, "{b:02X}");
            s
        })
}

#[cfg(test)]
pub(crate) mod tests {
    use std::str::FromStr;

    use rcgen::string::Ia5String;
    use rustls::{
        RootCertStore,
        client::{WebPkiServerVerifier, danger::ServerCertVerifier},
        pki_types::{ServerName, UnixTime},
    };
    use x509_parser::{extensions::ParsedExtension, oid_registry::OID_X509_EXT_NAME_CONSTRAINTS};

    use super::*;
    use crate::{clock::ManualClock, keystore::MemoryKeyStore};

    pub(crate) fn t0() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_790_000_000).unwrap()
    }

    pub(crate) fn test_ca() -> (LocalCa, ManualClock) {
        let clock = ManualClock::new(t0());
        let identity = CaIdentity {
            user: "dev".into(),
            host: "laptop.example".into(),
        };
        (LocalCa::generate(&identity, &clock).unwrap().0, clock)
    }

    pub(crate) fn verify(
        ca: &LocalCa,
        leaf: &CertificateDer<'_>,
        host: &str,
        at: OffsetDateTime,
    ) -> Result<(), rustls::Error> {
        let mut roots = RootCertStore::empty();
        roots.add(ca.cert_der().clone()).unwrap();
        let verifier =
            WebPkiServerVerifier::builder_with_provider(Arc::new(roots), crypto_provider())
                .build()
                .unwrap();
        let server_name = ServerName::try_from(host.to_owned()).unwrap();
        let now = UnixTime::since_unix_epoch(std::time::Duration::from_secs(
            u64::try_from(at.unix_timestamp()).unwrap(),
        ));
        verifier
            .verify_server_cert(leaf, &[], &server_name, &[], now)
            .map(|_| ())
    }

    fn dns(name: &str) -> SanType {
        SanType::DnsName(Ia5String::try_from(name).unwrap())
    }

    #[test]
    fn common_name_uses_short_host() {
        let id = CaIdentity {
            user: "ana".into(),
            host: "ana-mbp.lan".into(),
        };
        assert_eq!(id.common_name(), "Teitunnel Local CA (ana@ana-mbp)");
    }

    #[test]
    fn root_has_critical_name_constraints_per_independent_parser() {
        let (ca, _) = test_ca();
        let (_, cert) = x509_parser::parse_x509_certificate(ca.cert_der()).unwrap();
        assert!(cert.is_ca());
        let bc = cert.basic_constraints().unwrap().unwrap();
        assert!(bc.critical);
        assert_eq!(bc.value.path_len_constraint, Some(0));
        let ext = cert
            .extensions()
            .iter()
            .find(|e| e.oid == OID_X509_EXT_NAME_CONSTRAINTS)
            .expect("name constraints present");
        assert!(ext.critical, "name constraints must be critical");
        let ParsedExtension::NameConstraints(nc) = ext.parsed_extension() else {
            panic!("unparsed name constraints");
        };
        assert!(nc.excluded_subtrees.is_none());
        let permitted = nc.permitted_subtrees.as_ref().unwrap();
        let mut dns_names = Vec::new();
        let mut ips = Vec::new();
        for subtree in permitted {
            match &subtree.base {
                x509_parser::extensions::GeneralName::DNSName(name) => {
                    dns_names.push(name.to_string());
                }
                x509_parser::extensions::GeneralName::IPAddress(bytes) => ips.push(bytes.to_vec()),
                other => panic!("unexpected subtree {other:?}"),
            }
        }
        assert_eq!(dns_names, ["localhost", "test", "local"]);
        assert_eq!(ips.len(), 6);
        assert!(ips.contains(&vec![127, 0, 0, 0, 255, 0, 0, 0]));
        assert!(ips.contains(&vec![172, 16, 0, 0, 255, 240, 0, 0]));
        assert!(ips.contains(&vec![192, 168, 0, 0, 255, 255, 0, 0]));
        let validity = cert.validity();
        assert_eq!(
            (validity.not_after.to_datetime() - validity.not_before.to_datetime()).whole_days(),
            3650
        );
        assert!(
            ca.common_name()
                .starts_with("Teitunnel Local CA (dev@laptop)")
        );
    }

    #[test]
    fn webpki_accepts_local_names_and_rejects_real_sites() {
        let (ca, clock) = test_ca();
        let now = clock.now();
        let sign = |sans: Vec<SanType>, cn: &str| {
            ca.sign_leaf(&sans, cn, now, now + Duration::days(30))
                .unwrap()
                .cert_der
        };
        let good = sign(
            vec![dns("app.localhost"), dns("*.app.localhost")],
            "app.localhost",
        );
        verify(&ca, &good, "app.localhost", now).unwrap();
        verify(&ca, &good, "api.app.localhost", now).unwrap();
        verify(
            &ca,
            &sign(vec![dns("api.test")], "api.test"),
            "api.test",
            now,
        )
        .unwrap();
        verify(
            &ca,
            &sign(vec![dns("phone.local")], "phone.local"),
            "phone.local",
            now,
        )
        .unwrap();
        let lan = sign(
            vec![SanType::IpAddress(
                IpAddr::from_str("192.168.1.20").unwrap(),
            )],
            "lan",
        );
        verify(&ca, &lan, "192.168.1.20", now).unwrap();

        // Signed by our CA with the policy bypassed: the constraints still stop them.
        for (san, host) in [
            (dns("evil.com"), "evil.com"),
            (dns("localhost.evil.com"), "localhost.evil.com"),
            (dns("notlocalhost"), "notlocalhost"),
            (
                SanType::IpAddress(IpAddr::from_str("8.8.8.8").unwrap()),
                "8.8.8.8",
            ),
        ] {
            let leaf = sign(vec![san], "x");
            let err = verify(&ca, &leaf, host, now).unwrap_err();
            assert!(
                format!("{err:?}").contains("NameConstraint")
                    || format!("{err:?}").contains("NotValidForName"),
                "{host}: {err:?}"
            );
        }
        // A mixed leaf (one allowed, one not) is rejected as a whole.
        let mixed = sign(vec![dns("app.localhost"), dns("evil.com")], "app.localhost");
        assert!(verify(&ca, &mixed, "app.localhost", now).is_err());
    }

    #[test]
    fn load_or_create_persists_and_reloads() {
        let store = MemoryKeyStore::default();
        let clock = ManualClock::new(t0());
        let id = CaIdentity {
            user: "u".into(),
            host: "h".into(),
        };
        let (first, created) =
            LocalCa::load_or_create(&store, &id, &clock, Duration::days(30)).unwrap();
        assert!(created);
        let (again, created) =
            LocalCa::load_or_create(&store, &id, &clock, Duration::days(30)).unwrap();
        assert!(!created);
        assert_eq!(first.sha256_fingerprint(), again.sha256_fingerprint());
        // Near expiry: rotated.
        clock.advance(CA_VALIDITY - Duration::days(10));
        let (rotated, created) =
            LocalCa::load_or_create(&store, &id, &clock, Duration::days(30)).unwrap();
        assert!(created);
        assert_ne!(rotated.sha256_fingerprint(), first.sha256_fingerprint());
    }

    #[test]
    fn corrupt_bundles_are_refused() {
        assert!(matches!(
            LocalCa::from_secret(&CaSecret::new(String::new())),
            Err(CaError::Corrupt(_))
        ));
        let (ca, _) = test_ca();
        let only_cert = CaSecret::new(ca.cert_pem().to_owned());
        assert!(matches!(
            LocalCa::from_secret(&only_cert),
            Err(CaError::Corrupt(_))
        ));
        // Another CA's key with this certificate.
        let (other, _) = test_ca();
        let (_, other_secret) = LocalCa::generate(
            &CaIdentity {
                user: "a".into(),
                host: "b".into(),
            },
            &ManualClock::new(t0()),
        )
        .unwrap();
        let key_start = other_secret
            .expose()
            .find("-----BEGIN PRIVATE KEY")
            .unwrap();
        let mismatched = CaSecret::new(format!(
            "{}{}",
            other.cert_pem(),
            &other_secret.expose()[key_start..]
        ));
        assert!(matches!(
            LocalCa::from_secret(&mismatched),
            Err(CaError::Corrupt(_))
        ));
    }

    #[test]
    fn pem_encoding_matches_rcgen() {
        let (ca, _) = test_ca();
        let parsed = CertificateDer::from_pem_slice(ca.cert_pem().as_bytes()).unwrap();
        assert_eq!(parsed.as_ref(), ca.cert_der().as_ref());
        assert_eq!(base64_encode(b"ab"), "YWI=");
        assert_eq!(base64_encode(b"abc"), "YWJj");
        assert_eq!(base64_encode(b"a"), "YQ==");
    }

    #[test]
    fn fingerprints_are_hex() {
        let (ca, _) = test_ca();
        assert_eq!(ca.sha1_fingerprint().len(), 40);
        assert_eq!(ca.sha256_fingerprint().len(), 64);
        assert!(!format!("{ca:?}").contains("PRIVATE"));
    }
}
