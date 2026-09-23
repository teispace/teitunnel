//! A private network range that WARP clients reach through a tunnel.

use std::{
    fmt,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    str::FromStr,
};

use ipnet::{IpNet, Ipv4Net, Ipv6Net};
use serde::Serialize;

use crate::text::{Text, UserText, english_display, msg};

/// Why a range was rejected. Messages are shown next to the field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrivateNetworkError {
    /// Not an IP address or CIDR range.
    Invalid,
    /// Covers far too much (it would take over a large part of the internet or the LAN).
    TooBroad(String),
    /// Loopback, link-local, multicast or unspecified: never reachable through a tunnel.
    Unroutable(String),
}

impl UserText for PrivateNetworkError {
    fn text(&self) -> Text {
        match self {
            Self::Invalid => msg::error::network::invalid(),
            Self::TooBroad(network) => msg::error::network::too_broad(network),
            Self::Unroutable(network) => msg::error::network::unroutable(network),
        }
    }
}

english_display!(PrivateNetworkError);

/// A validated range, host bits cleared (`192.168.1.7/24` → `192.168.1.0/24`); a bare
/// address is a single host (`/32` or `/128`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrivateNetwork(IpNet);

impl PrivateNetwork {
    /// Parses and checks a range typed by the user.
    ///
    /// # Errors
    /// See [`PrivateNetworkError`].
    pub fn parse(input: &str) -> Result<Self, PrivateNetworkError> {
        let input = input.trim();
        let net = IpNet::from_str(input)
            .or_else(|_| IpAddr::from_str(input).map(IpNet::from))
            .map_err(|_| PrivateNetworkError::Invalid)?
            .trunc();
        let too_broad = match net {
            IpNet::V4(n) => n.prefix_len() < 8,
            IpNet::V6(n) => n.prefix_len() < 16,
        };
        if too_broad {
            return Err(PrivateNetworkError::TooBroad(net.to_string()));
        }
        let unroutable = match net.addr() {
            IpAddr::V4(a) => {
                a.is_loopback()
                    || a.is_link_local()
                    || a.is_multicast()
                    || a.is_unspecified()
                    || a.is_broadcast()
            }
            IpAddr::V6(a) => {
                a.is_loopback()
                    || a.is_multicast()
                    || a.is_unspecified()
                    || (a.segments()[0] & 0xffc0) == 0xfe80
            }
        };
        if unroutable {
            return Err(PrivateNetworkError::Unroutable(net.to_string()));
        }
        Ok(Self(net))
    }

    /// The range.
    pub fn net(&self) -> IpNet {
        self.0
    }

    /// Whether it's in space reserved for private use (RFC 1918, shared CGNAT space,
    /// unique local IPv6). Routing a public range works, but takes those addresses over
    /// for WARP clients.
    pub fn is_private(&self) -> bool {
        match self.0 {
            IpNet::V4(n) => {
                let within = |base: Ipv4Addr, len: u8| {
                    Ipv4Net::new(base, len).is_ok_and(|space| space.contains(&n))
                };
                within(Ipv4Addr::new(10, 0, 0, 0), 8)
                    || within(Ipv4Addr::new(172, 16, 0, 0), 12)
                    || within(Ipv4Addr::new(192, 168, 0, 0), 16)
                    || within(Ipv4Addr::new(100, 64, 0, 0), 10)
            }
            IpNet::V6(n) => Ipv6Net::new(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 0), 7)
                .is_ok_and(|space| space.contains(&n)),
        }
    }

    /// Whether the two ranges share any address.
    pub fn overlaps(&self, other: &Self) -> bool {
        self.0.contains(&other.0) || other.0.contains(&self.0)
    }
}

impl fmt::Display for PrivateNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for PrivateNetwork {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net(s: &str) -> PrivateNetwork {
        PrivateNetwork::parse(s).unwrap()
    }

    #[test]
    fn normalizes_ranges_and_single_hosts() {
        assert_eq!(net("192.168.1.7/24").to_string(), "192.168.1.0/24");
        assert_eq!(net(" 10.0.0.5 ").to_string(), "10.0.0.5/32");
        assert_eq!(net("fd12:3456::1/48").to_string(), "fd12:3456::/48");
        assert_eq!(net("fd00::1").to_string(), "fd00::1/128");
    }

    #[test]
    fn rejects_what_a_tunnel_cant_or_shouldnt_carry() {
        use PrivateNetworkError as E;
        assert_eq!(PrivateNetwork::parse("nope"), Err(E::Invalid));
        assert_eq!(PrivateNetwork::parse("10.0.0.0/33"), Err(E::Invalid));
        assert_eq!(
            PrivateNetwork::parse("0.0.0.0/0"),
            Err(E::TooBroad("0.0.0.0/0".into()))
        );
        assert_eq!(
            PrivateNetwork::parse("10.0.0.0/7"),
            Err(E::TooBroad("10.0.0.0/7".into()))
        );
        for bad in [
            "127.0.0.1",
            "169.254.1.0/24",
            "224.0.0.0/8",
            "::1",
            "fe80::/64",
        ] {
            assert!(
                matches!(PrivateNetwork::parse(bad), Err(E::Unroutable(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn knows_private_space_and_overlaps() {
        assert!(net("192.168.1.0/24").is_private());
        assert!(net("100.64.0.0/10").is_private());
        assert!(net("fd12:3456::/48").is_private());
        assert!(!net("2001:db8::/32").is_private());
        assert!(!net("8.8.8.0/24").is_private());
        assert!(!net("172.32.0.0/16").is_private());
        assert!(net("10.0.0.0/8").overlaps(&net("10.1.0.0/16")));
        assert!(net("10.1.0.0/16").overlaps(&net("10.0.0.0/8")));
        assert!(!net("10.1.0.0/16").overlaps(&net("10.2.0.0/16")));
    }

    proptest::proptest! {
        /// Any IPv4 address and prefix gives the network it's in, which reads back as
        /// itself; parsing never panics on noise.
        #[test]
        fn networks_are_canonical(a: u8, b: u8, c: u8, d: u8, prefix in 0u8..=32) {
            if let Ok(network) = PrivateNetwork::parse(&format!("{a}.{b}.{c}.{d}/{prefix}")) {
                let text = network.to_string();
                proptest::prop_assert_eq!(PrivateNetwork::parse(&text).unwrap().to_string(), text);
            }
        }

        #[test]
        fn never_panics(input in ".{0,30}") {
            let _ = PrivateNetwork::parse(&input);
        }
    }
}
