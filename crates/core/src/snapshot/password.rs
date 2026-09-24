//! The Snapshot password as the Worker checks it: PBKDF2-HMAC-SHA256 with a random salt,
//! stored only as a Worker secret, `pbkdf2-sha256$<iterations>$<salt>$<hash>` (base64url).
//! The password itself is never stored anywhere; Teitunnel keeps only "has a password".

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use super::SnapshotError;
use crate::Secret;

/// Iterations: slow enough to make guessing costly, fast enough for the Worker's CPU
/// budget on a login attempt (docs/research/cloudflare-snapshots.md).
pub const ITERATIONS: u32 = 20_000;
/// Shortest password accepted.
pub const MIN_LENGTH: usize = 6;
const SALT_LEN: usize = 16;
const HASH_LEN: usize = 32;
const BLOCK: usize = 64;

fn hmac_sha256(key: &[u8], message: &[&[u8]]) -> [u8; 32] {
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(block.map(|b| b ^ 0x36));
    for part in message {
        inner.update(part);
    }
    let mut outer = Sha256::new();
    outer.update(block.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// PBKDF2-HMAC-SHA256 with one output block (32 bytes).
pub(crate) fn pbkdf2(password: &[u8], salt: &[u8], iterations: u32) -> [u8; HASH_LEN] {
    let mut u = hmac_sha256(password, &[salt, &1u32.to_be_bytes()]);
    let mut out = u;
    for _ in 1..iterations {
        u = hmac_sha256(password, &[&u]);
        for (o, b) in out.iter_mut().zip(u) {
            *o ^= b;
        }
    }
    out
}

/// The stored form for `password` with `salt`.
pub(crate) fn encode(password: &str, salt: &[u8], iterations: u32) -> String {
    let hash = pbkdf2(password.as_bytes(), salt, iterations);
    format!(
        "pbkdf2-sha256${iterations}${}${}",
        URL_SAFE_NO_PAD.encode(salt),
        URL_SAFE_NO_PAD.encode(hash)
    )
}

/// Hashes a new password with a fresh salt.
///
/// # Errors
/// A password shorter than [`MIN_LENGTH`], or no randomness.
pub fn hash(password: &str) -> Result<Secret<String>, SnapshotError> {
    if password.chars().count() < MIN_LENGTH {
        return Err(SnapshotError::PasswordTooShort(MIN_LENGTH));
    }
    let mut salt = [0u8; SALT_LEN];
    getrandom::fill(&mut salt).map_err(|_| SnapshotError::Random)?;
    Ok(Secret::new(encode(password, &salt, ITERATIONS)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn matches_the_rfc_7914_vectors() {
        // RFC 7914 §11, PBKDF2-HMAC-SHA256 test vectors (first 32 bytes).
        assert_eq!(
            hex(&pbkdf2(b"passwd", b"salt", 1)),
            "55ac046e56e3089fec1691c22544b605f94185216dde0465e68b9d57c20dacbc"
        );
        assert_eq!(
            hex(&pbkdf2(b"Password", b"NaCl", 80000)),
            "4ddcd8f60b98be21830cee5ef22701f9641a4418d04c0414aeff08876b34ab56"
        );
    }

    #[test]
    fn hmac_matches_rfc_4231() {
        // RFC 4231 test case 2.
        assert_eq!(
            hex(&hmac_sha256(
                b"Jefe",
                &[b"what do ya want ", b"for nothing?"]
            )),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    /// The same vector is checked by the Worker's tests
    /// (apps/desktop/src/test/snapshot-worker.test.ts), so both sides agree.
    #[test]
    fn encodes_what_the_worker_expects() {
        assert_eq!(
            encode("correct horse", b"0123456789abcdef", 1000),
            "pbkdf2-sha256$1000$MDEyMzQ1Njc4OWFiY2RlZg$cBg8D2DungRB9k76szThf5ehfyBz991ay6PT8Srwk4M"
        );
    }

    #[test]
    fn salts_every_hash_and_refuses_short_passwords() {
        let a = hash("open sesame").unwrap();
        let b = hash("open sesame").unwrap();
        assert_ne!(a.expose(), b.expose());
        assert!(a.expose().starts_with("pbkdf2-sha256$20000$"));
        assert!(matches!(
            hash("short"),
            Err(SnapshotError::PasswordTooShort(6))
        ));
    }
}
