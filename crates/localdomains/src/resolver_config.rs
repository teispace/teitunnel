//! Per-OS resolver entries that send `.test` queries to Teitunnel's DNS responder. Each
//! needs administrator rights, so they are returned as [`PrivilegedAction`]s.
//!
//! - macOS: `/etc/resolver/test` (resolver(5)) with `nameserver 127.0.0.1` and `port`.
//! - Linux (systemd-resolved): a drop-in with `DNS=127.0.0.1:<port>` and the routing-only
//!   domain `~test`, then a restart of `systemd-resolved`.
//! - Windows: a Name Resolution Policy Table rule for `.test`. The Windows DNS client always
//!   uses port 53, so on Windows the responder must listen on `127.0.0.1:53`.

use std::path::PathBuf;

use crate::{platform::Platform, privileged::PrivilegedAction, process::Invocation};

/// macOS resolver file.
pub const MACOS_RESOLVER_FILE: &str = "/etc/resolver/test";
/// systemd-resolved drop-in.
pub const RESOLVED_DROP_IN: &str = "/etc/systemd/resolved.conf.d/teitunnel.conf";
/// Registry key of Teitunnel's NRPT rule (local rules, as `Add-DnsClientNrptRule` stores
/// them).
pub const WINDOWS_NRPT_KEY: &str =
    r"HKLM\SYSTEM\CurrentControlSet\Services\Dnscache\Parameters\DnsPolicyConfig\Teitunnel-test";

/// Why no configuration can be produced.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolverConfigError {
    /// Windows' DNS client can't use a port other than 53.
    #[error("Windows can only send .test queries to port 53; start the responder on 127.0.0.1:53")]
    WindowsNeedsPort53,
}

/// The steps that route `.test` to `127.0.0.1:<port>`.
///
/// # Errors
/// [`ResolverConfigError::WindowsNeedsPort53`] on Windows with another port.
pub fn setup(platform: Platform, port: u16) -> Result<Vec<PrivilegedAction>, ResolverConfigError> {
    Ok(match platform {
        Platform::Macos => vec![PrivilegedAction::WriteFile {
            path: PathBuf::from(MACOS_RESOLVER_FILE),
            contents: format!(
                "# Added by Teitunnel: .test names resolve through its local DNS responder.\nnameserver 127.0.0.1\nport {port}\n"
            ),
            mode: 0o644,
        }],
        Platform::Linux => vec![
            PrivilegedAction::WriteFile {
                path: PathBuf::from(RESOLVED_DROP_IN),
                contents: format!(
                    "# Added by Teitunnel: .test names resolve through its local DNS responder.\n[Resolve]\nDNS=127.0.0.1:{port}\nDomains=~test\n"
                ),
                mode: 0o644,
            },
            PrivilegedAction::Run(restart_resolved()),
        ],
        Platform::Windows => {
            if port != 53 {
                return Err(ResolverConfigError::WindowsNeedsPort53);
            }
            let mut steps: Vec<PrivilegedAction> = [
                ("Name", "REG_MULTI_SZ", ".test"),
                ("GenericDNSServers", "REG_SZ", "127.0.0.1"),
                ("ConfigOptions", "REG_DWORD", "8"),
                ("Version", "REG_DWORD", "2"),
                ("Comment", "REG_SZ", "Teitunnel local domains"),
            ]
            .into_iter()
            .map(|(name, ty, data)| PrivilegedAction::Run(reg_add(name, ty, data)))
            .collect();
            steps.push(PrivilegedAction::Run(dnscache_paramchange()));
            steps
        }
    })
}

/// The steps that undo [`setup`].
#[must_use]
pub fn teardown(platform: Platform) -> Vec<PrivilegedAction> {
    match platform {
        Platform::Macos => vec![PrivilegedAction::RemoveFile {
            path: PathBuf::from(MACOS_RESOLVER_FILE),
        }],
        Platform::Linux => vec![
            PrivilegedAction::RemoveFile {
                path: PathBuf::from(RESOLVED_DROP_IN),
            },
            PrivilegedAction::Run(restart_resolved()),
        ],
        Platform::Windows => vec![
            PrivilegedAction::Run(
                Invocation::new(r"C:\Windows\System32\reg.exe")
                    .arg("delete")
                    .arg(WINDOWS_NRPT_KEY)
                    .arg("/f"),
            ),
            PrivilegedAction::Run(dnscache_paramchange()),
        ],
    }
}

/// Whether systemd-resolved manages this Linux system's DNS (its stub resolv.conf exists).
#[must_use]
pub fn linux_uses_systemd_resolved(fs_root: &std::path::Path) -> bool {
    fs_root
        .join("run/systemd/resolve/stub-resolv.conf")
        .exists()
}

fn restart_resolved() -> Invocation {
    Invocation::new("/usr/bin/systemctl")
        .arg("restart")
        .arg("systemd-resolved")
}

fn reg_add(name: &str, ty: &str, data: &str) -> Invocation {
    Invocation::new(r"C:\Windows\System32\reg.exe")
        .arg("add")
        .arg(WINDOWS_NRPT_KEY)
        .arg("/v")
        .arg(name)
        .arg("/t")
        .arg(ty)
        .arg("/d")
        .arg(data)
        .arg("/f")
}

/// Tells the DNS Client service to re-read its parameters (the NRPT is cached in memory).
fn dnscache_paramchange() -> Invocation {
    Invocation::new(r"C:\Windows\System32\sc.exe")
        .arg("control")
        .arg("Dnscache")
        .arg("paramchange")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macos_file() {
        let steps = setup(Platform::Macos, 53535).unwrap();
        let [
            PrivilegedAction::WriteFile {
                path,
                contents,
                mode,
            },
        ] = steps.as_slice()
        else {
            panic!("expected one file");
        };
        assert_eq!(path, &PathBuf::from("/etc/resolver/test"));
        assert!(contents.ends_with("nameserver 127.0.0.1\nport 53535\n"));
        assert_eq!(*mode, 0o644);
        assert_eq!(
            teardown(Platform::Macos),
            [PrivilegedAction::RemoveFile {
                path: "/etc/resolver/test".into()
            }]
        );
    }

    #[test]
    fn linux_drop_in() {
        let steps = setup(Platform::Linux, 53535).unwrap();
        let PrivilegedAction::WriteFile { path, contents, .. } = &steps[0] else {
            panic!("expected a file");
        };
        assert_eq!(
            path,
            &PathBuf::from("/etc/systemd/resolved.conf.d/teitunnel.conf")
        );
        assert!(contents.contains("[Resolve]\nDNS=127.0.0.1:53535\nDomains=~test\n"));
        let PrivilegedAction::Run(inv) = &steps[1] else {
            panic!("expected a run")
        };
        assert_eq!(
            inv.argv(),
            ["/usr/bin/systemctl", "restart", "systemd-resolved"]
        );
    }

    #[test]
    fn windows_nrpt() {
        assert_eq!(
            setup(Platform::Windows, 53535),
            Err(ResolverConfigError::WindowsNeedsPort53)
        );
        let steps = setup(Platform::Windows, 53).unwrap();
        let PrivilegedAction::Run(first) = &steps[0] else {
            panic!("expected a run")
        };
        assert_eq!(
            first.argv(),
            [
                r"C:\Windows\System32\reg.exe",
                "add",
                WINDOWS_NRPT_KEY,
                "/v",
                "Name",
                "/t",
                "REG_MULTI_SZ",
                "/d",
                ".test",
                "/f"
            ]
        );
        let PrivilegedAction::Run(last) = steps.last().unwrap() else {
            panic!("expected a run")
        };
        assert_eq!(last.argv()[1..], ["control", "Dnscache", "paramchange"]);
        assert_eq!(teardown(Platform::Windows).len(), 2);
    }

    #[test]
    fn detects_resolved() {
        let root = tempfile::tempdir().unwrap();
        assert!(!linux_uses_systemd_resolved(root.path()));
        std::fs::create_dir_all(root.path().join("run/systemd/resolve")).unwrap();
        std::fs::write(root.path().join("run/systemd/resolve/stub-resolv.conf"), "").unwrap();
        assert!(linux_uses_systemd_resolved(root.path()));
    }
}
