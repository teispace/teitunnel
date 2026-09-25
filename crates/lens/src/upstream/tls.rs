//! TLS client configuration for https origins.

use std::sync::Arc;

use rustls::{
    ClientConfig, DigitallySignedStruct, SignatureScheme,
    client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier},
    crypto::{CryptoProvider, WebPkiSupportedAlgorithms},
    pki_types::{CertificateDer, ServerName, UnixTime},
};
use rustls_platform_verifier::BuilderVerifierExt;

use crate::LensError;

/// The crypto provider Lens uses, chosen explicitly: the workspace compiles rustls
/// with more than one provider, so relying on the process default would panic.
fn provider() -> Arc<CryptoProvider> {
    Arc::new(rustls::crypto::aws_lc_rs::default_provider())
}

/// A client config for an origin: the platform's verifier (or none), and ALPN for the
/// HTTP version the origin speaks.
pub(crate) fn client_config(verify: bool, http2: bool) -> Result<Arc<ClientConfig>, LensError> {
    let provider = provider();
    let builder = ClientConfig::builder_with_provider(Arc::clone(&provider))
        .with_safe_default_protocol_versions()
        .map_err(|err| LensError::Tls(err.to_string()))?;
    let mut config = if verify {
        builder
            .with_platform_verifier()
            .map_err(|err| LensError::Tls(err.to_string()))?
            .with_no_client_auth()
    } else {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(NoVerify(
                provider.signature_verification_algorithms,
            )))
            .with_no_client_auth()
    };
    config.alpn_protocols = if http2 {
        vec![b"h2".to_vec()]
    } else {
        vec![b"http/1.1".to_vec()]
    };
    Ok(Arc::new(config))
}

/// The name to present (SNI) and verify: an IP or a DNS name.
pub(crate) fn server_name(host: &str) -> Result<ServerName<'static>, LensError> {
    ServerName::try_from(host.to_owned())
        .map_err(|_| LensError::InvalidUpstream(format!("{host:?} isn't a valid TLS server name")))
}

/// Accepts any certificate (development origins with self-signed certificates), while
/// still checking that the handshake is signed by the presented key.
#[derive(Debug)]
struct NoVerify(WebPkiSupportedAlgorithms);

impl ServerCertVerifier for NoVerify {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(message, cert, dss, &self.0)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(message, cert, dss, &self.0)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0.supported_schemes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_configs() {
        let insecure = client_config(false, true).unwrap();
        assert_eq!(insecure.alpn_protocols, vec![b"h2".to_vec()]);
        let verified = client_config(true, false).unwrap();
        assert_eq!(verified.alpn_protocols, vec![b"http/1.1".to_vec()]);
    }

    #[test]
    fn server_names() {
        assert!(server_name("localhost").is_ok());
        assert!(server_name("127.0.0.1").is_ok());
        assert!(server_name("::1").is_ok());
        assert!(server_name("bad name").is_err());
    }
}
