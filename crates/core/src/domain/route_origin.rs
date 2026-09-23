use std::fmt;

use serde::{Deserialize, Serialize};

/// Why an origin was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RouteOriginError {
    /// Nothing entered.
    #[error("Enter a port or an address, like 3000 or localhost:3000.")]
    Empty,
    /// Unknown scheme.
    #[error("Use http, https, tcp, ssh, rdp, smb or unix.")]
    Scheme,
    /// Malformed address or port.
    #[error("That address isn't valid. Use host:port, like localhost:3000.")]
    Address,
    /// Bad HTTP status for `http_status:`.
    #[error("Use a status code between 100 and 599.")]
    Status,
}

/// Where a route sends traffic: the `service` of an ingress rule.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(try_from = "String", into = "String")]
pub struct RouteOrigin(String);

const SCHEMES: &[&str] = &["http", "https", "tcp", "ssh", "rdp", "smb"];

impl RouteOrigin {
    /// Parses user input: `3000`, `:3000`, `localhost:3000`, `http://…`, `https://…`,
    /// `tcp://…`, `ssh://…`, `rdp://…`, `smb://…`, `unix:/path`, `unix+tls:/path`,
    /// `hello_world`, `http_status:404`.
    ///
    /// # Errors
    /// See [`RouteOriginError`].
    pub fn parse(input: &str) -> Result<Self, RouteOriginError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(RouteOriginError::Empty);
        }
        if input == "hello_world" {
            return Ok(Self(input.to_owned()));
        }
        if let Some(code) = input.strip_prefix("http_status:") {
            let code: u16 = code.parse().map_err(|_| RouteOriginError::Status)?;
            return if (100..600).contains(&code) {
                Ok(Self(format!("http_status:{code}")))
            } else {
                Err(RouteOriginError::Status)
            };
        }
        for prefix in ["unix+tls:", "unix:"] {
            if let Some(path) = input.strip_prefix(prefix) {
                return if path.starts_with('/') && !path.contains(char::is_whitespace) {
                    Ok(Self(input.to_owned()))
                } else {
                    Err(RouteOriginError::Address)
                };
            }
        }
        let (scheme, rest) = match input.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
            None => ("http".to_owned(), input),
        };
        if !SCHEMES.contains(&scheme.as_str()) {
            return Err(RouteOriginError::Scheme);
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        // An IPv6 host must be bracketed (`[::1]:3000`); it's validated below.
        let mut ipv6 = false;
        let (host, port) = if authority.chars().all(|c| c.is_ascii_digit()) {
            ("localhost", Some(authority))
        } else if let Some(port) = authority.strip_prefix(':') {
            ("localhost", Some(port))
        } else if let Some(bracketed) = authority.strip_prefix('[') {
            let (host, after) = bracketed.split_once(']').ok_or(RouteOriginError::Address)?;
            if host.parse::<std::net::Ipv6Addr>().is_err() {
                return Err(RouteOriginError::Address);
            }
            ipv6 = true;
            (host, after.strip_prefix(':'))
        } else {
            match authority.rsplit_once(':') {
                Some((host, port)) => (host, Some(port)),
                None => (authority, None),
            }
        };
        let host_ok = ipv6
            || (!host.is_empty()
                && host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-.".contains(&b)));
        let port = match port {
            Some(p) => Some(
                p.parse::<u16>()
                    .ok()
                    .filter(|p| *p > 0)
                    .ok_or(RouteOriginError::Address)?,
            ),
            None => None,
        };
        if !host_ok || (port.is_none() && !matches!(scheme.as_str(), "http" | "https")) {
            return Err(RouteOriginError::Address);
        }
        let host = if ipv6 {
            format!("[{host}]")
        } else {
            host.to_ascii_lowercase()
        };
        let service = match port {
            Some(port) => format!("{scheme}://{host}:{port}"),
            None => format!("{scheme}://{host}"),
        };
        Ok(Self(service))
    }

    /// The ingress `service` string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The local TCP port, when the origin has one (used for "is it listening?").
    pub fn port(&self) -> Option<u16> {
        let (_, rest) = self.0.split_once("://")?;
        rest.rsplit_once(':')?.1.parse().ok()
    }

    /// Whether the origin points at this machine.
    pub fn is_local(&self) -> bool {
        let Some((_, rest)) = self.0.split_once("://") else {
            return self.0.starts_with("unix") || self.0 == "hello_world";
        };
        let host = rest.rsplit_once(':').map_or(rest, |(h, _)| h);
        matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "0.0.0.0")
    }
}

impl fmt::Display for RouteOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for RouteOrigin {
    type Error = RouteOriginError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<RouteOrigin> for String {
    fn from(origin: RouteOrigin) -> Self {
        origin.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_every_supported_form() {
        for (input, expected) in [
            ("3000", "http://localhost:3000"),
            (":5000", "http://localhost:5000"),
            ("localhost:8080", "http://localhost:8080"),
            ("https://localhost:8443/app", "https://localhost:8443"),
            ("tcp://127.0.0.1:5432", "tcp://127.0.0.1:5432"),
            ("ssh://localhost:22", "ssh://localhost:22"),
            ("rdp://10.0.0.5:3389", "rdp://10.0.0.5:3389"),
            ("http://[::1]:3000", "http://[::1]:3000"),
            ("http://my-nas.local", "http://my-nas.local"),
            ("unix:/tmp/app.sock", "unix:/tmp/app.sock"),
            ("hello_world", "hello_world"),
            ("http_status:404", "http_status:404"),
        ] {
            assert_eq!(
                RouteOrigin::parse(input).unwrap().as_str(),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(RouteOrigin::parse(""), Err(RouteOriginError::Empty));
        assert_eq!(
            RouteOrigin::parse("ftp://x:21"),
            Err(RouteOriginError::Scheme)
        );
        assert_eq!(
            RouteOrigin::parse("tcp://localhost"),
            Err(RouteOriginError::Address)
        );
        assert_eq!(
            RouteOrigin::parse("localhost:99999"),
            Err(RouteOriginError::Address)
        );
        assert_eq!(
            RouteOrigin::parse("http_status:99"),
            Err(RouteOriginError::Status)
        );
        assert_eq!(
            RouteOrigin::parse("unix:relative.sock"),
            Err(RouteOriginError::Address)
        );
        assert_eq!(
            RouteOrigin::parse("bad host:80"),
            Err(RouteOriginError::Address)
        );
        // IPv6 hosts must be bracketed; a bare one is ambiguous with the port
        // (found by the round-trip property test).
        for input in ["-::1", "::1:3000", "http://fe80::1:80", "[nope]:80"] {
            assert_eq!(
                RouteOrigin::parse(input),
                Err(RouteOriginError::Address),
                "{input}"
            );
        }
    }

    #[test]
    fn knows_ports_and_locality() {
        let origin = RouteOrigin::parse("3000").unwrap();
        assert_eq!(origin.port(), Some(3000));
        assert!(origin.is_local());
        assert!(!RouteOrigin::parse("http://10.0.0.2:80").unwrap().is_local());
        assert_eq!(RouteOrigin::parse("http_status:404").unwrap().port(), None);
    }

    proptest::proptest! {
        #[test]
        fn output_reparses_to_itself(input in "[a-z0-9:/._\\[\\]-]{0,30}") {
            if let Ok(origin) = RouteOrigin::parse(&input) {
                proptest::prop_assert_eq!(RouteOrigin::parse(origin.as_str()), Ok(origin));
            }
        }
    }
}
