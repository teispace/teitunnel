//! Parsing `cert.pem` from `cloudflared tunnel login`.

use base64::Engine;
use serde::Deserialize;

use crate::Secret;

/// The credential inside a cert.pem: a token scoped to one zone.
#[derive(Debug)]
pub struct CertCredential {
    /// Zone the token works for.
    pub zone_id: String,
    /// Owning account.
    pub account_id: String,
    /// The token.
    pub api_token: Secret<String>,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(rename = "zoneID")]
    zone_id: String,
    #[serde(rename = "accountID")]
    account_id: String,
    #[serde(rename = "apiToken")]
    api_token: String,
}

const BEGIN: &str = "-----BEGIN ARGO TUNNEL TOKEN-----";
const END: &str = "-----END ARGO TUNNEL TOKEN-----";

/// Extracts the `ARGO TUNNEL TOKEN` block. Other blocks (certificate, private key)
/// in older files are ignored. Returns `None` for anything malformed.
pub fn parse_cert_pem(pem: &str) -> Option<CertCredential> {
    let start = pem.find(BEGIN)? + BEGIN.len();
    let end = start + pem[start..].find(END)?;
    let body: String = pem[start..end]
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let json = base64::engine::general_purpose::STANDARD
        .decode(body)
        .ok()?;
    let payload: Payload = serde_json::from_slice(&json).ok()?;
    let valid =
        |s: &str| !s.is_empty() && s.len() <= 256 && s.bytes().all(|b| b.is_ascii_graphic());
    (valid(&payload.zone_id) && valid(&payload.account_id) && valid(&payload.api_token)).then(
        || CertCredential {
            zone_id: payload.zone_id,
            account_id: payload.account_id,
            api_token: Secret::new(payload.api_token),
        },
    )
}

#[cfg(test)]
pub(crate) fn sample_pem(zone: &str, account: &str, token: &str) -> String {
    let json = format!(r#"{{"zoneID":"{zone}","accountID":"{account}","apiToken":"{token}"}}"#);
    let body = base64::engine::general_purpose::STANDARD.encode(json);
    format!(
        "-----BEGIN PRIVATE KEY-----\nAAAA\n-----END PRIVATE KEY-----\n{BEGIN}\n{}\n{}\n{END}\n",
        &body[..20],
        &body[20..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_token_block_across_lines() {
        let cert = parse_cert_pem(&sample_pem("zone1", "acc1", "tok-123")).unwrap();
        assert_eq!(
            (cert.zone_id.as_str(), cert.account_id.as_str()),
            ("zone1", "acc1")
        );
        assert_eq!(cert.api_token.expose(), "tok-123");
        assert!(!format!("{cert:?}").contains("tok-123"));
    }

    #[test]
    fn rejects_malformed_files() {
        assert!(parse_cert_pem("").is_none());
        assert!(parse_cert_pem(&format!("{BEGIN}\nnot base64!\n{END}")).is_none());
        assert!(parse_cert_pem(&sample_pem("", "acc", "tok")).is_none());
        assert!(parse_cert_pem(&format!("{BEGIN}\ne30=\n{END}")).is_none()); // "{}"
    }
}
