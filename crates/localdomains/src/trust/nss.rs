//! NSS databases: Chrome/Chromium on Linux (`~/.pki/nssdb` or, since Chromium 146,
//! `~/.local/share/pki/nssdb`) and every Firefox profile
//! (which keeps its own store on every OS). Changed with NSS's `certutil` (not Windows'
//! `certutil.exe`, a different tool), per user, no administrator rights.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{platform::Platform, process::Invocation};

/// Which app owns a database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum NssApp {
    /// Chrome or Chromium (Linux shared NSS database).
    Chromium,
    /// A Firefox profile.
    Firefox,
}

/// An NSS database directory (`sql:` format, holding `cert9.db`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NssDb {
    /// The owning app.
    pub app: NssApp,
    /// The directory.
    pub path: PathBuf,
}

/// Firefox profile roots under `home` (or `%APPDATA%` on Windows), per platform.
#[must_use]
pub fn firefox_profile_roots(
    platform: Platform,
    home: &Path,
    app_data: Option<&Path>,
) -> Vec<PathBuf> {
    match platform {
        Platform::Macos => vec![home.join("Library/Application Support/Firefox/Profiles")],
        Platform::Windows => app_data
            .map(|d| vec![d.join("Mozilla").join("Firefox").join("Profiles")])
            .unwrap_or_default(),
        Platform::Linux => vec![
            home.join(".mozilla/firefox"),
            home.join(".config/mozilla/firefox"),
            home.join("snap/firefox/common/.mozilla/firefox"),
            home.join(".var/app/org.mozilla.firefox/.mozilla/firefox"),
        ],
    }
}

/// Profile directories (children of the roots that contain `marker`, e.g. `cert9.db` or
/// `prefs.js`), sorted.
#[must_use]
pub fn profiles_with(roots: &[PathBuf], marker: &str) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| std::fs::read_dir(root).ok())
        .flatten()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.join(marker).is_file())
        .collect();
    found.sort();
    found
}

/// Every NSS database for this user: the shared Chromium database on Linux and every
/// Firefox profile with a `cert9.db`.
#[must_use]
pub fn discover(platform: Platform, home: &Path, app_data: Option<&Path>) -> Vec<NssDb> {
    let mut dbs = Vec::new();
    if platform == Platform::Linux {
        // `~/.pki/nssdb` is the historical location; Chromium 146+ defaults to
        // `~/.local/share/pki/nssdb` when the old one doesn't exist.
        for dir in [
            home.join(".pki/nssdb"),
            home.join(".local/share/pki/nssdb"),
            home.join("snap/chromium/current/.pki/nssdb"),
        ] {
            if dir.join("cert9.db").is_file() {
                dbs.push(NssDb {
                    app: NssApp::Chromium,
                    path: dir,
                });
            }
        }
    }
    let roots = firefox_profile_roots(platform, home, app_data);
    dbs.extend(
        profiles_with(&roots, "cert9.db")
            .into_iter()
            .map(|path| NssDb {
                app: NssApp::Firefox,
                path,
            }),
    );
    dbs
}

/// Where NSS `certutil` may live besides `PATH` (Homebrew's `nss` keeps it unlinked on
/// some setups).
#[must_use]
pub fn certutil_extra_dirs(platform: Platform) -> Vec<PathBuf> {
    match platform {
        Platform::Macos => vec![
            PathBuf::from("/opt/homebrew/opt/nss/bin"),
            PathBuf::from("/usr/local/opt/nss/bin"),
            PathBuf::from("/opt/homebrew/bin"),
            PathBuf::from("/usr/local/bin"),
        ],
        Platform::Linux => vec![PathBuf::from("/usr/bin")],
        Platform::Windows => Vec::new(),
    }
}

fn sql_dir(db: &Path) -> std::ffi::OsString {
    let mut arg = std::ffi::OsString::from("sql:");
    arg.push(db.as_os_str());
    arg
}

/// Adds the CA as a trusted SSL issuer (`-t C,,`) under `nickname`.
#[must_use]
pub fn add(certutil: &Path, db: &Path, nickname: &str, cert: &Path) -> Invocation {
    Invocation::new(certutil)
        .arg("-A")
        .arg("-d")
        .arg(sql_dir(db))
        .arg("-t")
        .arg("C,,")
        .arg("-n")
        .arg(nickname)
        .arg("-i")
        .arg(cert.as_os_str())
}

/// Deletes the certificate `nickname`.
#[must_use]
pub fn delete(certutil: &Path, db: &Path, nickname: &str) -> Invocation {
    Invocation::new(certutil)
        .arg("-D")
        .arg("-d")
        .arg(sql_dir(db))
        .arg("-n")
        .arg(nickname)
}

/// Looks `nickname` up; exits 0 when present.
#[must_use]
pub fn list(certutil: &Path, db: &Path, nickname: &str) -> Invocation {
    Invocation::new(certutil)
        .arg("-L")
        .arg("-d")
        .arg(sql_dir(db))
        .arg("-n")
        .arg(nickname)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn discovers_linux_databases() {
        let home = tempfile::tempdir().unwrap();
        let h = home.path();
        touch(&h.join(".pki/nssdb/cert9.db"));
        touch(&h.join(".local/share/pki/nssdb/cert9.db"));
        touch(&h.join(".mozilla/firefox/abc.default-release/cert9.db"));
        touch(&h.join(".mozilla/firefox/old.legacy/cert8.db"));
        touch(&h.join("snap/firefox/common/.mozilla/firefox/snp.default/cert9.db"));
        touch(&h.join(".var/app/org.mozilla.firefox/.mozilla/firefox/flt.default/cert9.db"));
        let dbs = discover(Platform::Linux, h, None);
        let paths: Vec<_> = dbs
            .iter()
            .map(|d| (d.app, d.path.strip_prefix(h).unwrap().to_path_buf()))
            .collect();
        assert_eq!(
            paths,
            [
                (NssApp::Chromium, PathBuf::from(".pki/nssdb")),
                (NssApp::Chromium, PathBuf::from(".local/share/pki/nssdb")),
                (
                    NssApp::Firefox,
                    PathBuf::from(".mozilla/firefox/abc.default-release")
                ),
                (
                    NssApp::Firefox,
                    PathBuf::from(".var/app/org.mozilla.firefox/.mozilla/firefox/flt.default")
                ),
                (
                    NssApp::Firefox,
                    PathBuf::from("snap/firefox/common/.mozilla/firefox/snp.default")
                ),
            ]
        );
    }

    #[test]
    fn discovers_macos_and_windows_profiles() {
        let home = tempfile::tempdir().unwrap();
        touch(
            &home
                .path()
                .join("Library/Application Support/Firefox/Profiles/x.default/cert9.db"),
        );
        assert_eq!(discover(Platform::Macos, home.path(), None).len(), 1);
        let appdata = tempfile::tempdir().unwrap();
        touch(
            &appdata
                .path()
                .join("Mozilla/Firefox/Profiles/y.default/cert9.db"),
        );
        assert_eq!(
            discover(Platform::Windows, home.path(), Some(appdata.path())).len(),
            1
        );
        assert!(discover(Platform::Windows, home.path(), None).is_empty());
    }

    #[test]
    fn argv() {
        let certutil = Path::new("/usr/bin/certutil");
        let db = Path::new("/home/a/.pki/nssdb");
        assert_eq!(
            add(
                certutil,
                db,
                "Teitunnel Local CA (a@b)",
                Path::new("/d/ca.pem")
            )
            .argv(),
            [
                "/usr/bin/certutil",
                "-A",
                "-d",
                "sql:/home/a/.pki/nssdb",
                "-t",
                "C,,",
                "-n",
                "Teitunnel Local CA (a@b)",
                "-i",
                "/d/ca.pem"
            ]
        );
        assert_eq!(
            delete(certutil, db, "N").argv(),
            [
                "/usr/bin/certutil",
                "-D",
                "-d",
                "sql:/home/a/.pki/nssdb",
                "-n",
                "N"
            ]
        );
        assert_eq!(
            list(certutil, db, "N").argv(),
            [
                "/usr/bin/certutil",
                "-L",
                "-d",
                "sql:/home/a/.pki/nssdb",
                "-n",
                "N"
            ]
        );
    }
}
