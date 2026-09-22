use std::{fmt, net::IpAddr};

use serde::Serialize;

/// Why an origin was rejected. Messages are shown to the user as-is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OriginError {
    /// Nothing was entered.
    #[error("Enter a port or a local address, like 3000 or localhost:3000.")]
    Empty,
    /// The port isn't a number between 1 and 65535.
    #[error("Port must be a number between 1 and 65535.")]
    InvalidPort,
    /// The scheme isn't supported for this kind of share.
    #[error("Only http:// and https:// services can be shared.")]
    UnsupportedScheme,
    /// The host part is malformed.
    #[error("That doesn't look like a valid address.")]
    InvalidHost,
}

/// An HTTP(S) origin cloudflared can proxy to, e.g. `http://localhost:3000`.
///
/// Parsing is forgiving about what people type: `3000`, `:3000`, `localhost:3000`,
/// `127.0.0.1:8080` and full URLs all work. Paths and query strings are dropped;
/// cloudflared proxies the whole host.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct OriginUrl(String);

impl OriginUrl {
    /// Parses user input into an origin.
    ///
    /// # Errors
    /// See [`OriginError`] for the cases.
    pub fn parse(input: &str) -> Result<Self, OriginError> {
        let input = input.trim();
        if input.is_empty() {
            return Err(OriginError::Empty);
        }
        let (scheme, rest) = match input.split_once("://") {
            Some((scheme, rest)) => (scheme.to_ascii_lowercase(), rest),
            None => ("http".to_owned(), input),
        };
        if scheme != "http" && scheme != "https" {
            return Err(OriginError::UnsupportedScheme);
        }
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let authority = authority.rsplit('@').next().unwrap_or_default(); // drop userinfo
        let (host, port) = split_host_port(authority)?;
        let host = if host.is_empty() {
            "localhost".to_owned()
        } else {
            host
        };
        validate_host(&host)?;
        let port = match port {
            Some(port) => port,
            None if input.chars().all(|c| c.is_ascii_digit()) => {
                return Err(OriginError::InvalidPort);
            }
            None => {
                if scheme == "https" {
                    443
                } else {
                    80
                }
            }
        };
        Ok(Self(format!("{scheme}://{host}:{port}")))
    }

    /// The origin as a URL string.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The port.
    pub fn port(&self) -> u16 {
        self.0
            .rsplit(':')
            .next()
            .and_then(|p| p.parse().ok())
            .unwrap_or_default()
    }
}

impl fmt::Display for OriginUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Splits `host:port`, `[v6]:port`, `:port` or a bare `port`.
fn split_host_port(authority: &str) -> Result<(String, Option<u16>), OriginError> {
    let parse_port = |p: &str| {
        p.parse::<u16>()
            .ok()
            .filter(|p| *p > 0)
            .ok_or(OriginError::InvalidPort)
    };
    if authority.chars().all(|c| c.is_ascii_digit()) && !authority.is_empty() {
        return Ok((String::new(), Some(parse_port(authority)?)));
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let (host, after) = rest.split_once(']').ok_or(OriginError::InvalidHost)?;
        let port = match after.strip_prefix(':') {
            Some(port) => Some(parse_port(port)?),
            None if after.is_empty() => None,
            None => return Err(OriginError::InvalidHost),
        };
        return Ok((format!("[{host}]"), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => Ok((host.to_ascii_lowercase(), Some(parse_port(port)?))),
        None => Ok((authority.to_ascii_lowercase(), None)),
    }
}

fn validate_host(host: &str) -> Result<(), OriginError> {
    if let Some(v6) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']')) {
        return v6
            .parse::<IpAddr>()
            .map(|_| ())
            .map_err(|_| OriginError::InvalidHost);
    }
    let valid = host.len() <= 253
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && !label.starts_with('-')
                && !label.ends_with('-')
        });
    if valid {
        Ok(())
    } else {
        Err(OriginError::InvalidHost)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_what_people_type() {
        for (input, expected) in [
            ("3000", "http://localhost:3000"),
            (":3000", "http://localhost:3000"),
            ("localhost:5173", "http://localhost:5173"),
            ("  LOCALHOST:8080/app?x=1 ", "http://localhost:8080"),
            ("127.0.0.1:4000", "http://127.0.0.1:4000"),
            ("http://[::1]:3000", "http://[::1]:3000"),
            ("https://localhost:8443", "https://localhost:8443"),
            ("https://my-mac.local", "https://my-mac.local:443"),
            ("http://user:pass@localhost:9000/", "http://localhost:9000"),
        ] {
            assert_eq!(
                OriginUrl::parse(input).unwrap().as_str(),
                expected,
                "{input}"
            );
        }
        assert_eq!(OriginUrl::parse("3000").unwrap().port(), 3000);
    }

    #[test]
    fn rejects_invalid_input_with_a_reason() {
        assert_eq!(OriginUrl::parse(""), Err(OriginError::Empty));
        assert_eq!(OriginUrl::parse("70000"), Err(OriginError::InvalidPort));
        assert_eq!(OriginUrl::parse("0"), Err(OriginError::InvalidPort));
        assert_eq!(
            OriginUrl::parse("localhost:http"),
            Err(OriginError::InvalidPort)
        );
        assert_eq!(
            OriginUrl::parse("ssh://localhost:22"),
            Err(OriginError::UnsupportedScheme)
        );
        assert_eq!(
            OriginUrl::parse("local host:3000"),
            Err(OriginError::InvalidHost)
        );
        assert_eq!(
            OriginUrl::parse("http://[zz]:1"),
            Err(OriginError::InvalidHost)
        );
        assert_eq!(
            OriginUrl::parse("-bad-.com:1"),
            Err(OriginError::InvalidHost)
        );
    }

    proptest::proptest! {
        #[test]
        fn never_panics_and_output_reparses(input in ".{0,60}") {
            if let Ok(origin) = OriginUrl::parse(&input) {
                proptest::prop_assert_eq!(OriginUrl::parse(origin.as_str()), Ok(origin));
            }
        }
    }
}
