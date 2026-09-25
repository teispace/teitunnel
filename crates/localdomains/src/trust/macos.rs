//! macOS: the user's login keychain plus user-domain trust settings for SSL.
//!
//! `security add-trusted-cert` without `-d` writes the *user* trust domain, which needs no
//! administrator account; macOS still asks for the user's password in a system dialog
//! before changing trust settings. Removal is `remove-trusted-cert` (trust settings) plus
//! `delete-certificate` (the keychain item).

use std::path::{Path, PathBuf};

use crate::process::Invocation;

const SECURITY: &str = "/usr/bin/security";

/// `~/Library/Keychains/login.keychain-db`.
#[must_use]
pub fn login_keychain(home: &Path) -> PathBuf {
    home.join("Library/Keychains/login.keychain-db")
}

/// Adds the certificate to the login keychain, trusted as a root for SSL in the user domain.
#[must_use]
pub fn add_trusted_cert(keychain: &Path, cert: &Path) -> Invocation {
    Invocation::new(SECURITY)
        .arg("add-trusted-cert")
        .arg("-r")
        .arg("trustRoot")
        .arg("-p")
        .arg("ssl")
        .arg("-k")
        .arg(keychain.as_os_str())
        .arg(cert.as_os_str())
}

/// Removes the certificate's user-domain trust settings.
#[must_use]
pub fn remove_trusted_cert(cert: &Path) -> Invocation {
    Invocation::new(SECURITY)
        .arg("remove-trusted-cert")
        .arg(cert.as_os_str())
}

/// Deletes the certificate (by SHA-1) from the keychain.
#[must_use]
pub fn delete_certificate(sha1: &str, keychain: &Path) -> Invocation {
    Invocation::new(SECURITY)
        .arg("delete-certificate")
        .arg("-Z")
        .arg(sha1)
        .arg(keychain.as_os_str())
}

/// Lists certificates named `common_name` in the keychain with their hashes.
#[must_use]
pub fn find_certificate(common_name: &str, keychain: &Path) -> Invocation {
    Invocation::new(SECURITY)
        .arg("find-certificate")
        .arg("-a")
        .arg("-Z")
        .arg("-c")
        .arg(common_name)
        .arg(keychain.as_os_str())
}

/// Exports the user-domain trust settings (a plist) to `out`.
#[must_use]
pub fn export_trust_settings(out: &Path) -> Invocation {
    Invocation::new(SECURITY)
        .arg("trust-settings-export")
        .arg(out.as_os_str())
}

/// Whether `find-certificate -Z` output lists a certificate with this SHA-1.
#[must_use]
pub fn listing_has_sha1(stdout: &str, sha1: &str) -> bool {
    stdout.lines().any(|line| {
        line.trim()
            .strip_prefix("SHA-1 hash:")
            .is_some_and(|h| h.trim().eq_ignore_ascii_case(sha1))
    })
}

/// Whether an exported trust-settings plist has an entry for this SHA-1. The `trustList`
/// dictionary is keyed by the certificates' SHA-1 in hex.
#[must_use]
pub fn trust_settings_have_sha1(plist_bytes: &[u8], sha1: &str) -> bool {
    let Ok(value) = plist::Value::from_reader(std::io::Cursor::new(plist_bytes)) else {
        return false;
    };
    value
        .as_dictionary()
        .and_then(|d| d.get("trustList"))
        .and_then(plist::Value::as_dictionary)
        .is_some_and(|list| list.keys().any(|k| k.eq_ignore_ascii_case(sha1)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)] // macOS paths: Windows would join them with `\`.
    fn argv() {
        let kc = login_keychain(Path::new("/Users/ana"));
        assert_eq!(
            add_trusted_cert(&kc, Path::new("/d/ca.pem")).argv(),
            [
                "/usr/bin/security",
                "add-trusted-cert",
                "-r",
                "trustRoot",
                "-p",
                "ssl",
                "-k",
                "/Users/ana/Library/Keychains/login.keychain-db",
                "/d/ca.pem"
            ]
        );
        assert_eq!(
            remove_trusted_cert(Path::new("/d/ca.pem")).argv(),
            ["/usr/bin/security", "remove-trusted-cert", "/d/ca.pem"]
        );
        assert_eq!(
            delete_certificate("AB12", &kc).argv(),
            [
                "/usr/bin/security",
                "delete-certificate",
                "-Z",
                "AB12",
                "/Users/ana/Library/Keychains/login.keychain-db"
            ]
        );
        assert_eq!(
            find_certificate("Teitunnel Local CA (a@b)", &kc).argv()[1..6],
            [
                "find-certificate",
                "-a",
                "-Z",
                "-c",
                "Teitunnel Local CA (a@b)"
            ]
        );
    }

    #[test]
    fn parses_listing_and_trust_settings() {
        let listing = "SHA-256 hash: 00FF\nSHA-1 hash: 0563B8630D62D75ABBC8AB1E4BDFB5A899B24D43\nkeychain: \"/x\"\n";
        assert!(listing_has_sha1(
            listing,
            "0563b8630d62d75abbc8ab1e4bdfb5a899b24d43"
        ));
        assert!(!listing_has_sha1(listing, "00FF"));

        let plist = br#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>trustList</key><dict>
<key>0563B8630D62D75ABBC8AB1E4BDFB5A899B24D43</key><dict><key>trustSettings</key><array/></dict>
</dict><key>trustVersion</key><integer>1</integer></dict></plist>"#;
        assert!(trust_settings_have_sha1(
            plist,
            "0563B8630D62D75ABBC8AB1E4BDFB5A899B24D43"
        ));
        assert!(!trust_settings_have_sha1(plist, "FFFF"));
        assert!(!trust_settings_have_sha1(b"not a plist", "FFFF"));
    }
}
