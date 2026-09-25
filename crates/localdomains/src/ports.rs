//! Can Lens listen on 443/80?
//!
//! - **macOS** (since 10.14) lets any user bind ports below 1024, but **only on the wildcard
//!   address** (`0.0.0.0`/`::`): binding `127.0.0.1:443` as a user fails with EACCES
//!   (checked on macOS 27). So on macOS Lens listens on the wildcard address and must drop
//!   connections whose peer isn't loopback (unless LAN access is on) — see
//!   [`choose_listener`] and [`ListenerChoice::WildcardLoopbackOnly`].
//! - **Windows** lets any user bind low ports on any address.
//! - **Linux** reserves ports below `net.ipv4.ip_unprivileged_port_start` (1024 by default)
//!   for root; the fixes are a sysctl drop-in or a file capability on the binary, both
//!   one-time privileged steps ([`linux_fixes`]).
//!
//! Otherwise Lens falls back to 8443/8080 with the port in the URL.

use std::{
    io,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{privileged::PrivilegedAction, process::Invocation};

/// The Linux sysctl file.
pub const UNPRIVILEGED_PORT_START: &str = "/proc/sys/net/ipv4/ip_unprivileged_port_start";
/// Where the sysctl fix is written.
pub const SYSCTL_DROP_IN: &str = "/etc/sysctl.d/50-teitunnel-unprivileged-ports.conf";

/// Result of trying to bind a port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum BindCheck {
    /// Bindable (the test listener was closed again).
    Available,
    /// The OS refused: the port is privileged.
    PermissionDenied,
    /// Another process is listening.
    InUse,
    /// Something else.
    Other {
        /// The error.
        message: String,
    },
}

/// Tries to bind `addr` and releases it immediately.
#[must_use]
pub fn check_bind(addr: SocketAddr) -> BindCheck {
    match TcpListener::bind(addr) {
        Ok(listener) => {
            drop(listener);
            BindCheck::Available
        }
        Err(err) => match err.kind() {
            io::ErrorKind::PermissionDenied => BindCheck::PermissionDenied,
            io::ErrorKind::AddrInUse => BindCheck::InUse,
            _ => BindCheck::Other {
                message: err.to_string(),
            },
        },
    }
}

/// Tries to bind `127.0.0.1:<port>` and releases it immediately.
#[must_use]
pub fn check_loopback_bind(port: u16) -> BindCheck {
    check_bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))
}

/// How Lens can listen on a port.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ListenerChoice {
    /// Bind `127.0.0.1` (and `::1`) directly.
    Loopback,
    /// Bind the wildcard address and accept only loopback peers (macOS, low ports), or LAN
    /// peers too when LAN access is on. Check every accepted connection with
    /// [`is_loopback_peer`].
    WildcardLoopbackOnly,
    /// Neither works.
    Unavailable {
        /// Why loopback failed.
        loopback: BindCheck,
    },
}

/// Picks how to listen on `port`: loopback if the OS allows it, else the wildcard address
/// with peer filtering when only the loopback bind was refused for permissions.
#[must_use]
pub fn choose_listener(port: u16) -> ListenerChoice {
    match check_loopback_bind(port) {
        BindCheck::Available => ListenerChoice::Loopback,
        BindCheck::PermissionDenied
            if check_bind(SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)))
                == BindCheck::Available
                || check_bind(SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)))
                    == BindCheck::Available =>
        {
            ListenerChoice::WildcardLoopbackOnly
        }
        loopback => ListenerChoice::Unavailable { loopback },
    }
}

/// Whether a connection's peer is on this machine (127.0.0.0/8, `::1`, or IPv4-mapped
/// loopback).
#[must_use]
pub fn is_loopback_peer(peer: IpAddr) -> bool {
    match peer {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.to_ipv4_mapped().is_some_and(|v4| v4.is_loopback())
        }
    }
}

/// Reads Linux's lowest unprivileged port (from [`UNPRIVILEGED_PORT_START`]).
#[must_use]
pub fn unprivileged_port_start(path: &Path) -> Option<u16> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

/// A way to let Teitunnel bind low ports on Linux.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortFix {
    /// Lower `ip_unprivileged_port_start` for every user (drop-in + apply now).
    Sysctl(Vec<PrivilegedAction>),
    /// Give only this binary `cap_net_bind_service` (must be redone after each update).
    Setcap(Vec<PrivilegedAction>),
}

/// The fixes for binding `lowest_port` (e.g. 80), for the binary at `binary`.
#[must_use]
pub fn linux_fixes(binary: &Path, lowest_port: u16) -> Vec<PortFix> {
    vec![
        PortFix::Sysctl(vec![
            PrivilegedAction::WriteFile {
                path: PathBuf::from(SYSCTL_DROP_IN),
                contents: format!(
                    "# Added by Teitunnel: lets it serve local HTTPS domains on port {lowest_port} and up.\nnet.ipv4.ip_unprivileged_port_start={lowest_port}\n"
                ),
                mode: 0o644,
            },
            PrivilegedAction::Run(
                Invocation::new("/usr/sbin/sysctl")
                    .arg("-w")
                    .arg(format!("net.ipv4.ip_unprivileged_port_start={lowest_port}")),
            ),
        ]),
        PortFix::Setcap(vec![PrivilegedAction::Run(
            Invocation::new("/usr/sbin/setcap")
                .arg("cap_net_bind_service=+ep")
                .arg(binary.as_os_str()),
        )]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_check_detects_in_use() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(check_loopback_bind(port), BindCheck::InUse);
        drop(listener);
        assert_eq!(check_loopback_bind(port), BindCheck::Available);
    }

    #[test]
    fn loopback_peers() {
        assert!(is_loopback_peer("127.0.0.1".parse().unwrap()));
        assert!(is_loopback_peer("127.8.9.10".parse().unwrap()));
        assert!(is_loopback_peer("::1".parse().unwrap()));
        assert!(is_loopback_peer("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_loopback_peer("192.168.1.5".parse().unwrap()));
        assert!(!is_loopback_peer("::ffff:10.0.0.1".parse().unwrap()));
    }

    #[test]
    fn chooses_loopback_for_a_free_high_port() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        assert_eq!(choose_listener(port), ListenerChoice::Loopback);
    }

    #[test]
    fn reads_sysctl() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("start");
        std::fs::write(&file, "1024\n").unwrap();
        assert_eq!(unprivileged_port_start(&file), Some(1024));
        assert_eq!(unprivileged_port_start(&dir.path().join("missing")), None);
    }

    #[test]
    fn fixes() {
        let fixes = linux_fixes(Path::new("/opt/teitunnel/teitunnel"), 80);
        let PortFix::Sysctl(steps) = &fixes[0] else {
            panic!("sysctl first")
        };
        let PrivilegedAction::WriteFile { contents, .. } = &steps[0] else {
            panic!("file")
        };
        assert!(contents.ends_with("net.ipv4.ip_unprivileged_port_start=80\n"));
        let PortFix::Setcap(steps) = &fixes[1] else {
            panic!("setcap second")
        };
        let PrivilegedAction::Run(inv) = &steps[0] else {
            panic!("run")
        };
        assert_eq!(
            inv.argv(),
            [
                "/usr/sbin/setcap",
                "cap_net_bind_service=+ep",
                "/opt/teitunnel/teitunnel"
            ]
        );
    }
}
