//! A private hostname that WARP clients reach through a tunnel, and telling one apart
//! from a private range in a field that takes either.
//!
//! Cloudflare's hostname routes accept a name under 255 characters with at most one
//! wildcard label, and store `*.internal.local` as `internal.local`
//! (<https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/private-net/cloudflared/connect-private-hostname/>,
//! checked 2026-09-26). Teitunnel takes exact names only: how a stored `internal.local`
//! matches its subdomains isn't documented well enough to show it truthfully.

use std::fmt;

use serde::Serialize;

use super::{Hostname, PrivateNetwork, PrivateNetworkError};
use crate::text::{Text, UserText, english_display, msg};

/// Why a private hostname was rejected. Messages are shown next to the field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrivateHostnameError {
    /// Not a valid DNS name (or a single label).
    Invalid,
    /// Has a wildcard.
    Wildcard,
    /// `localhost` or a name under it: always the WARP device itself.
    Localhost(String),
}

impl UserText for PrivateHostnameError {
    fn text(&self) -> Text {
        match self {
            Self::Invalid => msg::error::network::invalid(),
            Self::Wildcard => msg::error::network::hostname_wildcard(),
            Self::Localhost(hostname) => msg::error::network::hostname_localhost(hostname),
        }
    }
}

english_display!(PrivateHostnameError);

/// A validated, lowercase, ASCII (punycode) hostname without a wildcard, such as
/// `wiki.internal`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PrivateHostname(Hostname);

impl PrivateHostname {
    /// Parses and checks a hostname typed by the user.
    ///
    /// # Errors
    /// See [`PrivateHostnameError`].
    pub fn parse(input: &str) -> Result<Self, PrivateHostnameError> {
        if input.contains('*') {
            return Err(PrivateHostnameError::Wildcard);
        }
        let hostname = Hostname::parse(input).map_err(|_| PrivateHostnameError::Invalid)?;
        if hostname.is_in_zone("localhost") {
            return Err(PrivateHostnameError::Localhost(hostname.to_string()));
        }
        Ok(Self(hostname))
    }

    /// The hostname as ASCII.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Whether it lies within `zone` (a zone apex like `teispace.com`).
    pub fn is_in_zone(&self, zone: &str) -> bool {
        self.0.is_in_zone(zone)
    }

    /// Whether `stored` (a route's hostname as Cloudflare returns it) is this name.
    pub fn matches(&self, stored: &str) -> bool {
        stored
            .trim_end_matches('.')
            .eq_ignore_ascii_case(self.as_str())
    }
}

impl fmt::Display for PrivateHostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl Serialize for PrivateHostname {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

/// What a WARP client reaches through a tunnel: an address range or a hostname.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrivateTarget {
    /// An address or a CIDR range.
    Network(PrivateNetwork),
    /// A hostname.
    Hostname(PrivateHostname),
}

/// Why a range or hostname was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PrivateTargetError {
    /// It looks like an address, and isn't a usable one.
    Network(PrivateNetworkError),
    /// It looks like a hostname, and isn't a usable one.
    Hostname(PrivateHostnameError),
}

impl UserText for PrivateTargetError {
    fn text(&self) -> Text {
        match self {
            Self::Network(e) => e.text(),
            Self::Hostname(e) => e.text(),
        }
    }
}

english_display!(PrivateTargetError);

impl PrivateTarget {
    /// Parses a field that takes either. Input made only of digits, dots, colons and
    /// slashes, or with a colon or a slash anywhere, is an address (no hostname has
    /// them), so a mistyped range gets the range's error rather than the hostname's.
    ///
    /// # Errors
    /// See [`PrivateTargetError`].
    pub fn parse(input: &str) -> Result<Self, PrivateTargetError> {
        let trimmed = input.trim();
        let address = trimmed.is_empty()
            || trimmed.contains([':', '/'])
            || trimmed.bytes().all(|b| b.is_ascii_digit() || b == b'.');
        if address {
            PrivateNetwork::parse(trimmed)
                .map(Self::Network)
                .map_err(PrivateTargetError::Network)
        } else {
            PrivateHostname::parse(trimmed)
                .map(Self::Hostname)
                .map_err(PrivateTargetError::Hostname)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_hostnames() {
        let h = PrivateHostname::parse(" Wiki.Internal. ").unwrap();
        assert_eq!(h.to_string(), "wiki.internal");
        assert!(h.matches("WIKI.internal"));
        assert!(!h.matches("internal"));
        assert!(h.is_in_zone("internal"));
        assert_eq!(
            PrivateHostname::parse("bücher.home.arpa").unwrap().as_str(),
            "xn--bcher-kva.home.arpa"
        );
    }

    #[test]
    fn rejects_what_a_hostname_route_cant_carry() {
        use PrivateHostnameError as E;
        assert_eq!(PrivateHostname::parse("wiki"), Err(E::Invalid));
        assert_eq!(PrivateHostname::parse("wi_ki.internal"), Err(E::Invalid));
        assert_eq!(PrivateHostname::parse("*.internal.local"), Err(E::Wildcard));
        assert_eq!(PrivateHostname::parse("dev-*.internal"), Err(E::Wildcard));
        assert_eq!(
            PrivateHostname::parse("app.localhost"),
            Err(E::Localhost("app.localhost".into()))
        );
    }

    #[test]
    fn tells_ranges_from_hostnames() {
        let parse = PrivateTarget::parse;
        assert!(matches!(
            parse("192.168.1.0/24"),
            Ok(PrivateTarget::Network(_))
        ));
        assert!(matches!(parse("fd00::1"), Ok(PrivateTarget::Network(_))));
        assert!(matches!(
            parse("wiki.internal"),
            Ok(PrivateTarget::Hostname(_))
        ));
        // A mistyped range gets the range's error.
        assert_eq!(
            parse("10.0.0.0/33"),
            Err(PrivateTargetError::Network(PrivateNetworkError::Invalid))
        );
        assert_eq!(
            parse("10.0.0"),
            Err(PrivateTargetError::Network(PrivateNetworkError::Invalid))
        );
        assert_eq!(
            parse(""),
            Err(PrivateTargetError::Network(PrivateNetworkError::Invalid))
        );
        assert_eq!(
            parse("*.internal"),
            Err(PrivateTargetError::Hostname(PrivateHostnameError::Wildcard))
        );
    }

    proptest::proptest! {
        #[test]
        fn never_panics(input in ".{0,40}") {
            let _ = PrivateTarget::parse(&input);
        }
    }
}
