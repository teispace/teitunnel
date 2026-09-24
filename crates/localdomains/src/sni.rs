//! TLS for Lens: a `rustls` certificate resolver that issues leaves on demand for the SNI
//! name of each handshake, and the server config around it.
//!
//! Handshakes for names no policy allows (anything outside the enabled suffixes, or not a
//! registered domain) get no certificate, so rustls aborts them.

use std::{
    fmt,
    sync::{Arc, RwLock},
};

use rustls::{
    ServerConfig,
    server::{ClientHello, ResolvesServerCert},
    sign::CertifiedKey,
};

use crate::{
    leaf::{CertRequest, LeafCache},
    name::{LocalName, Suffix},
    registry::DomainRegistry,
};

/// Decides which certificate (if any) a handshake for an SNI name gets.
pub trait CertPolicy: fmt::Debug + Send + Sync {
    /// The certificate to serve for `sni`, or `None` to refuse the handshake.
    fn cert_request(&self, sni: &str) -> Option<CertRequest>;
}

/// Serves only registered domains (and their wildcard subdomains). Share the lock with
/// whatever updates the registry; the next handshake sees the change.
impl CertPolicy for RwLock<DomainRegistry> {
    fn cert_request(&self, sni: &str) -> Option<CertRequest> {
        self.read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .cert_request(sni)
    }
}

/// Serves any valid name under the given suffixes (no registry), each with its own exact
/// certificate.
#[derive(Debug, Clone)]
pub struct SuffixPolicy {
    /// Suffixes to serve.
    pub allowed: Vec<Suffix>,
}

impl CertPolicy for SuffixPolicy {
    fn cert_request(&self, sni: &str) -> Option<CertRequest> {
        LocalName::parse(sni, &self.allowed)
            .ok()
            .map(CertRequest::exact)
    }
}

/// The on-demand certificate resolver.
pub struct SniResolver {
    policy: Arc<dyn CertPolicy>,
    leaves: Arc<LeafCache>,
}

impl fmt::Debug for SniResolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SniResolver")
            .field("policy", &self.policy)
            .field("leaves", &self.leaves)
            .finish()
    }
}

impl SniResolver {
    /// A resolver that asks `policy` which names to serve and gets leaves from `leaves`.
    #[must_use]
    pub fn new(policy: Arc<dyn CertPolicy>, leaves: Arc<LeafCache>) -> Self {
        Self { policy, leaves }
    }

    /// The certificate for `sni`, as a handshake would get it.
    #[must_use]
    pub fn resolve_name(&self, sni: &str) -> Option<Arc<CertifiedKey>> {
        let Some(request) = self.policy.cert_request(sni) else {
            tracing::debug!(sni, "refused TLS for a name outside local domains");
            return None;
        };
        match self.leaves.get(&request) {
            Ok(key) => Some(key),
            Err(err) => {
                tracing::warn!(error = %err, name = %request.name, "couldn't issue a local certificate");
                None
            }
        }
    }
}

impl ResolvesServerCert for SniResolver {
    fn resolve(&self, client_hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.resolve_name(client_hello.server_name()?)
    }
}

/// A TLS server config for Lens: TLS 1.2 and 1.3, no client auth, certificates from
/// `resolver`, ALPN `h2` then `http/1.1`.
///
/// # Errors
/// rustls rejected the protocol versions for the provider (not expected).
pub fn server_config(resolver: Arc<SniResolver>) -> Result<ServerConfig, rustls::Error> {
    let provider = Arc::clone(resolver.leaves.ca().provider());
    let mut config = ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_cert_resolver(resolver);
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};

    use rustls::{
        ClientConfig, ClientConnection, RootCertStore, ServerConnection, pki_types::ServerName,
    };

    use super::*;
    use crate::{
        ca::{crypto_provider, tests::test_ca},
        registry::{DomainTarget, LocalDomain},
    };

    fn resolver(policy: Arc<dyn CertPolicy>) -> (Arc<SniResolver>, RootCertStore) {
        let (ca, clock) = test_ca();
        let mut roots = RootCertStore::empty();
        roots.add(ca.cert_der().clone()).unwrap();
        let leaves = Arc::new(LeafCache::new(Arc::new(ca), Arc::new(clock)));
        (Arc::new(SniResolver::new(policy, leaves)), roots)
    }

    /// Runs a full in-memory handshake; returns the negotiated ALPN protocol.
    fn handshake(
        server: &Arc<SniResolver>,
        roots: RootCertStore,
        host: &str,
    ) -> Result<Option<Vec<u8>>, rustls::Error> {
        // The resolver's certificates are only valid around the test clock's time, so the
        // client verifies against that time.
        let mut client_config = ClientConfig::builder_with_provider(crypto_provider())
            .with_safe_default_protocol_versions()?
            .with_root_certificates(roots)
            .with_no_client_auth();
        client_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        client_config.time_provider = Arc::new(TestTime);
        let mut client = ClientConnection::new(
            Arc::new(client_config),
            ServerName::try_from(host.to_owned()).unwrap(),
        )?;
        let mut server_conn = ServerConnection::new(Arc::new(server_config(Arc::clone(server))?))?;
        for _ in 0..10 {
            let mut buf = Vec::new();
            client.write_tls(&mut buf).unwrap();
            server_conn.read_tls(&mut buf.as_slice()).unwrap();
            server_conn.process_new_packets()?;
            let mut buf = Vec::new();
            server_conn.write_tls(&mut buf).unwrap();
            client.read_tls(&mut buf.as_slice()).unwrap();
            client.process_new_packets()?;
            if !client.is_handshaking() && !server_conn.is_handshaking() {
                client.writer().write_all(b"ping").unwrap();
                let mut buf = Vec::new();
                client.write_tls(&mut buf).unwrap();
                server_conn.read_tls(&mut buf.as_slice()).unwrap();
                server_conn.process_new_packets()?;
                let mut got = [0_u8; 4];
                server_conn.reader().read_exact(&mut got).unwrap();
                assert_eq!(&got, b"ping");
                return Ok(client.alpn_protocol().map(<[u8]>::to_vec));
            }
        }
        Err(rustls::Error::General("handshake did not finish".into()))
    }

    #[derive(Debug)]
    struct TestTime;

    impl rustls::time_provider::TimeProvider for TestTime {
        fn current_time(&self) -> Option<rustls::pki_types::UnixTime> {
            let secs = crate::ca::tests::t0().unix_timestamp();
            Some(rustls::pki_types::UnixTime::since_unix_epoch(
                std::time::Duration::from_secs(u64::try_from(secs).ok()?),
            ))
        }
    }

    #[test]
    fn suffix_policy_serves_local_names_with_h2() {
        let (server, roots) = resolver(Arc::new(SuffixPolicy {
            allowed: vec![Suffix::Localhost],
        }));
        let alpn = handshake(&server, roots.clone(), "app.localhost").unwrap();
        assert_eq!(alpn.as_deref(), Some(&b"h2"[..]));
        assert!(handshake(&server, roots.clone(), "evil.com").is_err());
        assert!(handshake(&server, roots, "app.test").is_err());
        assert!(server.resolve_name("localhost").is_none());
    }

    #[test]
    fn registry_policy_follows_registry_changes() {
        let registry = Arc::new(RwLock::new(DomainRegistry::new(&Suffix::ALL)));
        let (server, roots) = resolver(Arc::clone(&registry) as Arc<dyn CertPolicy>);
        assert!(server.resolve_name("app.localhost").is_none());
        registry
            .write()
            .unwrap()
            .add(LocalDomain {
                name: LocalName::parse_any("app.localhost").unwrap(),
                target: DomainTarget::Port { port: 5173 },
                wildcard: true,
                created_at: 0,
            })
            .unwrap();
        handshake(&server, roots.clone(), "app.localhost").unwrap();
        handshake(&server, roots.clone(), "api.app.localhost").unwrap();
        assert!(handshake(&server, roots, "other.localhost").is_err());
        // Both names share the wildcard leaf.
        let a = server.resolve_name("app.localhost").unwrap();
        let b = server.resolve_name("api.app.localhost").unwrap();
        assert!(Arc::ptr_eq(&a, &b));
    }
}
