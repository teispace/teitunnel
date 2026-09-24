//! Windows: the current user's Trusted Root store (`CurrentUser\Root`), through
//! `certutil -user`, so no administrator rights are needed. Windows shows its own security
//! warning asking the user to confirm adding (or deleting) a root.

use std::path::{Path, PathBuf};

use crate::process::Invocation;

/// `%SystemRoot%\System32\certutil.exe` (falls back to `C:\Windows`).
#[must_use]
pub fn certutil_path(system_root: Option<&Path>) -> PathBuf {
    system_root
        .unwrap_or_else(|| Path::new(r"C:\Windows"))
        .join("System32")
        .join("certutil.exe")
}

/// Adds the certificate to `CurrentUser\Root`.
#[must_use]
pub fn add_store(certutil: &Path, cert: &Path) -> Invocation {
    Invocation::new(certutil)
        .arg("-user")
        .arg("-addstore")
        .arg("Root")
        .arg(cert.as_os_str())
}

/// Deletes the certificate (by SHA-1) from `CurrentUser\Root`.
#[must_use]
pub fn del_store(certutil: &Path, sha1: &str) -> Invocation {
    Invocation::new(certutil)
        .arg("-user")
        .arg("-delstore")
        .arg("Root")
        .arg(sha1)
}

/// Looks the certificate (by SHA-1) up in `CurrentUser\Root`; exits 0 when found.
#[must_use]
pub fn find_in_store(certutil: &Path, sha1: &str) -> Invocation {
    Invocation::new(certutil)
        .arg("-user")
        .arg("-store")
        .arg("Root")
        .arg(sha1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv() {
        let certutil = certutil_path(Some(Path::new(r"C:\Windows")));
        assert!(certutil.ends_with("certutil.exe"));
        let add = add_store(Path::new("certutil.exe"), Path::new(r"C:\data\ca.pem"));
        assert_eq!(
            add.argv(),
            [
                "certutil.exe",
                "-user",
                "-addstore",
                "Root",
                r"C:\data\ca.pem"
            ]
        );
        assert_eq!(
            del_store(Path::new("certutil.exe"), "AB12").argv(),
            ["certutil.exe", "-user", "-delstore", "Root", "AB12"]
        );
        assert_eq!(
            find_in_store(Path::new("certutil.exe"), "AB12").argv(),
            ["certutil.exe", "-user", "-store", "Root", "AB12"]
        );
    }
}
