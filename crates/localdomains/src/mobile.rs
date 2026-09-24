//! Letting phones trust the CA: an Apple configuration profile (iOS/iPadOS) and the plain
//! certificate (Android), which the app serves on the LAN behind a QR link.
//!
//! iOS: after installing the profile (Settings > General > VPN & Device Management), full
//! trust for TLS must still be switched on in Settings > General > About > Certificate
//! Trust Settings. Android: Settings > Security > Encryption & credentials > Install a
//! certificate > CA certificate; Chrome on Android trusts user CAs, many apps don't.

use sha2::{Digest, Sha256};

use crate::ca::LocalCa;

/// MIME type of a configuration profile.
pub const MOBILECONFIG_CONTENT_TYPE: &str = "application/x-apple-aspen-config";
/// MIME type of a CA certificate download (DER).
pub const CA_CERT_CONTENT_TYPE: &str = "application/x-x509-ca-cert";
/// MIME type of the PEM form.
pub const PEM_CONTENT_TYPE: &str = "application/x-pem-file";

/// A profile failure.
#[derive(Debug, thiserror::Error)]
#[error("building the profile: {0}")]
pub struct ProfileError(#[from] plist::Error);

/// An XML configuration profile with one `com.apple.security.root` payload holding the CA.
/// UUIDs derive from the certificate, so downloading it again gives the same profile.
///
/// # Errors
/// Serializing the plist failed (not expected).
pub fn apple_mobileconfig(ca: &LocalCa) -> Result<Vec<u8>, ProfileError> {
    use plist::{Dictionary, Value};

    let der = ca.cert_der().as_ref();
    let digest = Sha256::digest(der);
    let fingerprint = crate::ca::hex_upper(&digest[..8]);
    let payload_uuid = uuid_from(&digest[..16]);
    let profile_uuid = uuid_from(&digest[16..32]);

    let mut cert = Dictionary::new();
    cert.insert(
        "PayloadCertificateFileName".into(),
        "teitunnel-local-ca.cer".into(),
    );
    cert.insert("PayloadContent".into(), Value::Data(der.to_vec()));
    cert.insert(
        "PayloadDescription".into(),
        "Lets this device trust Teitunnel's local HTTPS domains. The CA can only sign .localhost, .test, .local and private addresses.".into(),
    );
    cert.insert("PayloadDisplayName".into(), ca.common_name().into());
    cert.insert(
        "PayloadIdentifier".into(),
        format!("com.teispace.teitunnel.localca.{fingerprint}.cert").into(),
    );
    cert.insert("PayloadType".into(), "com.apple.security.root".into());
    cert.insert("PayloadUUID".into(), payload_uuid.into());
    cert.insert("PayloadVersion".into(), 1_i64.into());

    let mut profile = Dictionary::new();
    profile.insert(
        "PayloadContent".into(),
        Value::Array(vec![Value::Dictionary(cert)]),
    );
    profile.insert(
        "PayloadDescription".into(),
        "Trust for local HTTPS domains served by Teitunnel on your computer.".into(),
    );
    profile.insert("PayloadDisplayName".into(), ca.common_name().into());
    profile.insert(
        "PayloadIdentifier".into(),
        format!("com.teispace.teitunnel.localca.{fingerprint}").into(),
    );
    profile.insert("PayloadOrganization".into(), "Teitunnel".into());
    profile.insert("PayloadRemovalDisallowed".into(), false.into());
    profile.insert("PayloadType".into(), "Configuration".into());
    profile.insert("PayloadUUID".into(), profile_uuid.into());
    profile.insert("PayloadVersion".into(), 1_i64.into());

    let mut out = Vec::new();
    Value::Dictionary(profile).to_writer_xml(&mut out)?;
    Ok(out)
}

/// The certificate as DER (`.crt`/`.cer`, what Android's installer expects).
#[must_use]
pub fn ca_der(ca: &LocalCa) -> Vec<u8> {
    ca.cert_der().to_vec()
}

/// The certificate as PEM.
#[must_use]
pub fn ca_pem(ca: &LocalCa) -> String {
    ca.cert_pem().to_owned()
}

fn uuid_from(bytes: &[u8]) -> String {
    let mut raw = [0_u8; 16];
    raw.copy_from_slice(&bytes[..16]);
    uuid::Builder::from_custom_bytes(raw)
        .into_uuid()
        .hyphenated()
        .to_string()
        .to_uppercase()
}

#[cfg(test)]
mod tests {
    use plist::Value;

    use super::*;
    use crate::ca::tests::test_ca;

    #[test]
    fn mobileconfig_parses_back_with_the_certificate() {
        let (ca, _) = test_ca();
        let bytes = apple_mobileconfig(&ca).unwrap();
        assert!(bytes.starts_with(b"<?xml"));
        let value = Value::from_reader_xml(bytes.as_slice()).unwrap();
        let profile = value.as_dictionary().unwrap();
        assert_eq!(profile["PayloadType"].as_string(), Some("Configuration"));
        assert_eq!(profile["PayloadVersion"].as_signed_integer(), Some(1));
        let payloads = profile["PayloadContent"].as_array().unwrap();
        assert_eq!(payloads.len(), 1);
        let cert = payloads[0].as_dictionary().unwrap();
        assert_eq!(
            cert["PayloadType"].as_string(),
            Some("com.apple.security.root")
        );
        assert_eq!(
            cert["PayloadContent"].as_data(),
            Some(ca.cert_der().as_ref())
        );
        let uuid = cert["PayloadUUID"].as_string().unwrap();
        assert!(uuid::Uuid::parse_str(uuid).is_ok());
        assert_ne!(uuid, profile["PayloadUUID"].as_string().unwrap());
        // Stable across downloads.
        assert_eq!(apple_mobileconfig(&ca).unwrap(), bytes);
    }

    #[test]
    fn plain_downloads() {
        let (ca, _) = test_ca();
        assert_eq!(ca_der(&ca), ca.cert_der().to_vec());
        assert!(ca_pem(&ca).starts_with("-----BEGIN CERTIFICATE-----\n"));
    }
}
