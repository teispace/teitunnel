//! Linux: the system trust store (one privileged step, returned as data) for curl, Node's
//! system-CA mode, Go and other tools using the OS bundle. Browsers use NSS instead (see
//! `nss`).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{privileged::PrivilegedAction, process::Invocation};

/// File name used in the anchor directories.
const ANCHOR_NAME: &str = "teitunnel-local-ca";

/// How this distribution manages its trust store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinuxFlavor {
    /// Debian, Ubuntu and derivatives: `/usr/local/share/ca-certificates/*.crt` +
    /// `update-ca-certificates`.
    Debian,
    /// Fedora, RHEL, CentOS: `/etc/pki/ca-trust/source/anchors/` + `update-ca-trust`.
    Fedora,
    /// openSUSE: `/etc/pki/trust/anchors/` + `update-ca-certificates`.
    Suse,
    /// Arch and other p11-kit systems: `trust anchor --store`.
    P11Kit,
}

impl LinuxFlavor {
    /// The package that provides NSS's `certutil` on this flavor.
    #[must_use]
    pub const fn certutil_package(self) -> &'static str {
        match self {
            Self::Debian => "libnss3-tools",
            Self::Fedora => "nss-tools",
            Self::Suse => "mozilla-nss-tools",
            Self::P11Kit => "nss",
        }
    }

    fn anchor_path(self) -> Option<PathBuf> {
        match self {
            Self::Debian => Some(PathBuf::from(format!(
                "/usr/local/share/ca-certificates/{ANCHOR_NAME}.crt"
            ))),
            Self::Fedora => Some(PathBuf::from(format!(
                "/etc/pki/ca-trust/source/anchors/{ANCHOR_NAME}.pem"
            ))),
            Self::Suse => Some(PathBuf::from(format!(
                "/etc/pki/trust/anchors/{ANCHOR_NAME}.pem"
            ))),
            Self::P11Kit => None,
        }
    }
}

/// The detected system store: the flavor and the tool that refreshes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemStore {
    /// The flavor.
    pub flavor: LinuxFlavor,
    /// `update-ca-certificates`, `update-ca-trust` or `trust`.
    pub tool: PathBuf,
}

/// Detects the system store. `root` is `/` except in tests; `find` looks a program up.
#[must_use]
pub fn detect(root: &Path, find: &dyn Fn(&str) -> Option<PathBuf>) -> Option<SystemStore> {
    let dir = |p: &str| root.join(p.trim_start_matches('/')).is_dir();
    let candidates = [
        (
            LinuxFlavor::Debian,
            "/usr/local/share/ca-certificates",
            "update-ca-certificates",
        ),
        (
            LinuxFlavor::Fedora,
            "/etc/pki/ca-trust/source/anchors",
            "update-ca-trust",
        ),
        (
            LinuxFlavor::Suse,
            "/etc/pki/trust/anchors",
            "update-ca-certificates",
        ),
    ];
    candidates
        .into_iter()
        .find_map(|(flavor, anchors, tool)| {
            dir(anchors)
                .then(|| find(tool))
                .flatten()
                .map(|tool| SystemStore { flavor, tool })
        })
        .or_else(|| {
            find("trust").map(|tool| SystemStore {
                flavor: LinuxFlavor::P11Kit,
                tool,
            })
        })
}

impl SystemStore {
    /// The privileged steps that add `cert` (PEM) to the system store.
    #[must_use]
    pub fn install_actions(&self, cert: &Path) -> Vec<PrivilegedAction> {
        match self.flavor.anchor_path() {
            Some(anchor) => vec![
                PrivilegedAction::CopyFile {
                    from: cert.to_path_buf(),
                    to: anchor,
                    mode: 0o644,
                },
                PrivilegedAction::Run(self.refresh()),
            ],
            None => vec![PrivilegedAction::Run(
                Invocation::new(&self.tool)
                    .arg("anchor")
                    .arg("--store")
                    .arg(cert.as_os_str()),
            )],
        }
    }

    /// The privileged steps that remove it again.
    #[must_use]
    pub fn uninstall_actions(&self, cert: &Path) -> Vec<PrivilegedAction> {
        match self.flavor.anchor_path() {
            Some(anchor) => vec![
                PrivilegedAction::RemoveFile { path: anchor },
                PrivilegedAction::Run(self.refresh_fresh()),
            ],
            None => vec![PrivilegedAction::Run(
                Invocation::new(&self.tool)
                    .arg("anchor")
                    .arg("--remove")
                    .arg(cert.as_os_str()),
            )],
        }
    }

    fn refresh(&self) -> Invocation {
        match self.flavor {
            LinuxFlavor::Fedora => Invocation::new(&self.tool).arg("extract"),
            LinuxFlavor::Debian | LinuxFlavor::Suse | LinuxFlavor::P11Kit => {
                Invocation::new(&self.tool)
            }
        }
    }

    /// After a removal, Debian's tool needs `--fresh` to drop the old link.
    fn refresh_fresh(&self) -> Invocation {
        match self.flavor {
            LinuxFlavor::Debian => Invocation::new(&self.tool).arg("--fresh"),
            LinuxFlavor::Fedora | LinuxFlavor::Suse | LinuxFlavor::P11Kit => self.refresh(),
        }
    }

    /// Whether the anchor file is in place with exactly this PEM (file-based flavors).
    /// `None` for p11-kit, where [`SystemStore::list_anchors`] is used instead.
    #[must_use]
    pub fn anchor_matches(&self, root: &Path, pem: &str) -> Option<bool> {
        let anchor = self.flavor.anchor_path()?;
        let path = root.join(anchor.strip_prefix("/").unwrap_or(&anchor));
        Some(std::fs::read_to_string(path).is_ok_and(|on_disk| on_disk.trim() == pem.trim()))
    }

    /// `trust list --filter=ca-anchors` (p11-kit), to look for the CA's label.
    #[must_use]
    pub fn list_anchors(&self) -> Invocation {
        Invocation::new(&self.tool)
            .arg("list")
            .arg("--filter=ca-anchors")
    }
}

/// Whether `trust list` output has an anchor labelled `common_name`.
#[must_use]
pub fn anchors_list_has(stdout: &str, common_name: &str) -> bool {
    stdout.lines().any(|l| {
        l.trim()
            .strip_prefix("label:")
            .is_some_and(|v| v.trim() == common_name)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("/usr/sbin/{name}")))
    }

    #[test]
    fn detects_flavors() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(detect(root.path(), &|_| None), None);
        assert_eq!(
            detect(root.path(), &found).unwrap().flavor,
            LinuxFlavor::P11Kit
        );
        std::fs::create_dir_all(root.path().join("etc/pki/ca-trust/source/anchors")).unwrap();
        let fedora = detect(root.path(), &found).unwrap();
        assert_eq!(fedora.flavor, LinuxFlavor::Fedora);
        assert_eq!(fedora.tool, PathBuf::from("/usr/sbin/update-ca-trust"));
        std::fs::create_dir_all(root.path().join("usr/local/share/ca-certificates")).unwrap();
        assert_eq!(
            detect(root.path(), &found).unwrap().flavor,
            LinuxFlavor::Debian
        );
    }

    #[test]
    fn debian_actions() {
        let store = SystemStore {
            flavor: LinuxFlavor::Debian,
            tool: "/usr/sbin/update-ca-certificates".into(),
        };
        let install = store.install_actions(Path::new("/home/a/.local/share/teitunnel/ca.pem"));
        assert_eq!(
            install,
            [
                PrivilegedAction::CopyFile {
                    from: "/home/a/.local/share/teitunnel/ca.pem".into(),
                    to: "/usr/local/share/ca-certificates/teitunnel-local-ca.crt".into(),
                    mode: 0o644
                },
                PrivilegedAction::Run(Invocation::new("/usr/sbin/update-ca-certificates"))
            ]
        );
        let uninstall = store.uninstall_actions(Path::new("/x.pem"));
        assert_eq!(
            uninstall[1],
            PrivilegedAction::Run(
                Invocation::new("/usr/sbin/update-ca-certificates").arg("--fresh")
            )
        );
    }

    #[test]
    fn fedora_and_p11kit_actions() {
        let fedora = SystemStore {
            flavor: LinuxFlavor::Fedora,
            tool: "/usr/bin/update-ca-trust".into(),
        };
        assert_eq!(
            fedora.install_actions(Path::new("/c.pem"))[1],
            PrivilegedAction::Run(Invocation::new("/usr/bin/update-ca-trust").arg("extract"))
        );
        let arch = SystemStore {
            flavor: LinuxFlavor::P11Kit,
            tool: "/usr/bin/trust".into(),
        };
        let PrivilegedAction::Run(inv) = &arch.install_actions(Path::new("/c.pem"))[0] else {
            panic!("expected a run");
        };
        assert_eq!(
            inv.argv(),
            ["/usr/bin/trust", "anchor", "--store", "/c.pem"]
        );
        let PrivilegedAction::Run(inv) = &arch.uninstall_actions(Path::new("/c.pem"))[0] else {
            panic!("expected a run");
        };
        assert_eq!(
            inv.argv(),
            ["/usr/bin/trust", "anchor", "--remove", "/c.pem"]
        );
        assert!(anchors_list_has(
            "pkcs11:id=...\n    type: certificate\n    label: Teitunnel Local CA (a@b)\n",
            "Teitunnel Local CA (a@b)"
        ));
    }

    #[test]
    fn anchor_file_check() {
        let root = tempfile::tempdir().unwrap();
        let store = SystemStore {
            flavor: LinuxFlavor::Debian,
            tool: "/usr/sbin/update-ca-certificates".into(),
        };
        assert_eq!(store.anchor_matches(root.path(), "PEM"), Some(false));
        let dir = root.path().join("usr/local/share/ca-certificates");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("teitunnel-local-ca.crt"), "PEM\n").unwrap();
        assert_eq!(store.anchor_matches(root.path(), "PEM"), Some(true));
        assert_eq!(store.anchor_matches(root.path(), "OTHER"), Some(false));
    }
}
