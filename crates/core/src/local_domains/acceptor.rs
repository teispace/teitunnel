//! Lens acceptors for local domains: who may connect, and the TLS handshake.
//!
//! macOS lets a normal user bind 443 only on the wildcard address, and `.local`
//! names resolve to this computer's network address, so the listeners may be reachable
//! from the network. Every connection is checked here, before any byte is read:
//! loopback and this computer's own addresses are always allowed; other machines on a
//! private network only when LAN access is on, and over HTTPS only for `.local` names.

use std::{
    collections::HashSet,
    io,
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, PoisonError, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use localdomains::ports::is_loopback_peer;
use tokio::net::TcpStream;

use crate::inspect::lens::{AcceptFuture, Accepted, Acceptor};

/// Who a peer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Peer {
    /// This computer.
    Local,
    /// Another device on a private network (LAN access is on).
    Lan,
}

/// Which peers may connect. Cheap to clone; clones share the state.
#[derive(Debug, Clone, Default)]
pub(crate) struct PeerPolicy {
    lan: Arc<AtomicBool>,
    own: Arc<RwLock<HashSet<IpAddr>>>,
}

impl PeerPolicy {
    /// Allows network peers too (`.local` names for phones).
    pub(crate) fn set_lan(&self, lan: bool) {
        self.lan.store(lan, Ordering::Relaxed);
    }

    /// Re-reads this computer's addresses (after a network change).
    pub(crate) fn refresh(&self) {
        let addrs = own_addresses();
        *self.own.write().unwrap_or_else(PoisonError::into_inner) = addrs.into_iter().collect();
    }

    #[cfg(test)]
    pub(crate) fn set_own(&self, addrs: &[IpAddr]) {
        *self.own.write().unwrap_or_else(PoisonError::into_inner) = addrs.iter().copied().collect();
    }

    /// Whether `peer` may connect, and as whom.
    pub(crate) fn classify(&self, peer: IpAddr) -> Option<Peer> {
        let peer = canonical(peer);
        if is_loopback_peer(peer)
            || self
                .own
                .read()
                .unwrap_or_else(PoisonError::into_inner)
                .contains(&peer)
        {
            return Some(Peer::Local);
        }
        (self.lan.load(Ordering::Relaxed) && is_private(peer)).then_some(Peer::Lan)
    }
}

/// IPv4-mapped IPv6 addresses (a dual-stack socket's view of IPv4 peers) as IPv4.
fn canonical(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(ip, IpAddr::V4),
        IpAddr::V4(_) => ip,
    }
}

/// Private and link-local ranges (RFC 1918, RFC 3927, RFC 4193, RFC 4291 link-local).
fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => {
            let first = v6.segments()[0];
            (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
        }
    }
}

/// This computer's interface addresses.
pub(crate) fn own_addresses() -> Vec<IpAddr> {
    if_addrs::get_if_addrs()
        .map(|interfaces| interfaces.into_iter().map(|i| i.ip()).collect())
        .unwrap_or_default()
}

/// Addresses a phone on the network could use (private IPv4 first).
pub(crate) fn lan_addresses() -> Vec<String> {
    let mut addrs: Vec<IpAddr> = own_addresses()
        .into_iter()
        .filter(|ip| !ip.is_loopback() && is_private(*ip))
        .collect();
    addrs.sort_by_key(|ip| (ip.is_ipv6(), *ip));
    addrs.dedup();
    addrs.into_iter().map(|ip| ip.to_string()).collect()
}

fn refused(peer: SocketAddr) -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        format!("connection from {peer} refused: local domains answer this computer only"),
    )
}

/// Plain HTTP (redirects to HTTPS, and HTTP-only domains) with the peer check.
#[derive(Debug, Clone)]
pub(crate) struct CheckedPlain {
    pub(crate) policy: PeerPolicy,
}

impl Acceptor for CheckedPlain {
    fn accept(&self, stream: TcpStream, peer: SocketAddr) -> AcceptFuture {
        let allowed = self.policy.classify(peer.ip()).is_some();
        Box::pin(async move {
            if !allowed {
                return Err(refused(peer));
            }
            Ok(Accepted {
                io: Box::new(stream),
                server_name: None,
            })
        })
    }

    fn is_secure(&self) -> bool {
        false
    }
}

/// TLS with certificates issued on demand for the SNI name, with the peer check.
#[derive(Clone)]
pub(crate) struct CheckedTls {
    pub(crate) policy: PeerPolicy,
    pub(crate) tls: tokio_rustls::TlsAcceptor,
}

impl std::fmt::Debug for CheckedTls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CheckedTls").finish_non_exhaustive()
    }
}

impl Acceptor for CheckedTls {
    fn accept(&self, stream: TcpStream, peer: SocketAddr) -> AcceptFuture {
        let who = self.policy.classify(peer.ip());
        let tls = self.tls.clone();
        Box::pin(async move {
            let who = who.ok_or_else(|| refused(peer))?;
            let stream = tls.accept(stream).await?;
            let server_name = stream.get_ref().1.server_name().map(str::to_owned);
            if who == Peer::Lan
                && !server_name
                    .as_deref()
                    .is_some_and(|name| name.to_ascii_lowercase().ends_with(".local"))
            {
                return Err(refused(peer));
            }
            Ok(Accepted {
                io: Box::new(stream),
                server_name,
            })
        })
    }

    fn is_secure(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn loopback_and_own_addresses_only_until_lan_is_on() {
        let policy = PeerPolicy::default();
        policy.set_own(&[ip("192.168.1.5")]);
        assert_eq!(policy.classify(ip("127.0.0.1")), Some(Peer::Local));
        assert_eq!(policy.classify(ip("::1")), Some(Peer::Local));
        assert_eq!(policy.classify(ip("::ffff:127.0.0.1")), Some(Peer::Local));
        assert_eq!(policy.classify(ip("192.168.1.5")), Some(Peer::Local));
        assert_eq!(policy.classify(ip("::ffff:192.168.1.5")), Some(Peer::Local));
        assert_eq!(policy.classify(ip("192.168.1.9")), None);
        assert_eq!(policy.classify(ip("203.0.113.9")), None);

        policy.set_lan(true);
        assert_eq!(policy.classify(ip("192.168.1.9")), Some(Peer::Lan));
        assert_eq!(policy.classify(ip("10.1.2.3")), Some(Peer::Lan));
        assert_eq!(policy.classify(ip("fe80::1")), Some(Peer::Lan));
        assert_eq!(policy.classify(ip("fd00::1")), Some(Peer::Lan));
        assert_eq!(
            policy.classify(ip("203.0.113.9")),
            None,
            "never public addresses"
        );
        assert_eq!(policy.classify(ip("2001:db8::1")), None);
    }
}
