use std::fmt;

use serde::{Deserialize, Serialize};

/// Why a hostname was rejected. Messages are shown under the field.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostnameError {
    /// Nothing entered.
    #[error("Enter a hostname, like app.example.com.")]
    Empty,
    /// Not a valid DNS name.
    #[error("That isn't a valid hostname. Use letters, digits and hyphens, separated by dots.")]
    Invalid,
    /// A single label (no dot).
    #[error("Include the domain, like app.example.com.")]
    NoDomain,
    /// `*` used somewhere other than the whole first label.
    #[error("A wildcard can only be the first part, like *.example.com.")]
    BadWildcard,
    /// Longer than DNS allows.
    #[error("That hostname is too long.")]
    TooLong,
}

/// A validated, lowercase, ASCII (punycode) hostname, optionally a leading wildcard.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(try_from = "String", into = "String")]
pub struct Hostname(String);

impl Hostname {
    /// Parses user input (Unicode names become punycode).
    ///
    /// # Errors
    /// See [`HostnameError`].
    pub fn parse(input: &str) -> Result<Self, HostnameError> {
        let trimmed = input.trim().trim_end_matches('.');
        if trimmed.is_empty() {
            return Err(HostnameError::Empty);
        }
        let (wildcard, rest) = match trimmed.strip_prefix("*.") {
            Some(rest) => (true, rest),
            None => (false, trimmed),
        };
        if rest.contains('*') {
            return Err(HostnameError::BadWildcard);
        }
        let ascii = idna::domain_to_ascii(rest).map_err(|_| HostnameError::Invalid)?;
        let labels: Vec<&str> = ascii.split('.').collect();
        if labels.len() < 2 {
            return Err(HostnameError::NoDomain);
        }
        let valid_label = |l: &&str| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        };
        if !labels.iter().all(valid_label) {
            return Err(HostnameError::Invalid);
        }
        let full = if wildcard {
            format!("*.{ascii}")
        } else {
            ascii
        };
        if full.len() > 253 {
            return Err(HostnameError::TooLong);
        }
        Ok(Self(full))
    }

    /// The hostname as ASCII.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether it's a wildcard (`*.example.com`).
    pub fn is_wildcard(&self) -> bool {
        self.0.starts_with("*.")
    }

    /// Whether this hostname lies within `zone` (a zone apex like `example.com`).
    pub fn is_in_zone(&self, zone: &str) -> bool {
        let name = self.0.trim_start_matches("*.");
        name == zone || name.ends_with(&format!(".{zone}"))
    }

    /// Picks the zone this hostname belongs to: the longest matching apex wins, so
    /// `a.dev.example.com` goes to `dev.example.com` when both zones exist.
    pub fn zone_in<'a, Z: AsRef<str>>(&self, zones: &'a [Z]) -> Option<&'a Z> {
        zones
            .iter()
            .filter(|zone| self.is_in_zone(zone.as_ref()))
            .max_by_key(|zone| zone.as_ref().len())
    }
}

impl fmt::Display for Hostname {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for Hostname {
    type Error = HostnameError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<Hostname> for String {
    fn from(host: Hostname) -> Self {
        host.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_valid_names() {
        for (input, expected) in [
            ("App.Example.com", "app.example.com"),
            ("  api.xyz.com. ", "api.xyz.com"),
            ("*.xyz.com", "*.xyz.com"),
            ("münchen.de", "xn--mnchen-3ya.de"),
            ("a-b.c-d.io", "a-b.c-d.io"),
        ] {
            assert_eq!(
                Hostname::parse(input).unwrap().as_str(),
                expected,
                "{input}"
            );
        }
    }

    #[test]
    fn rejects_invalid_names() {
        assert_eq!(Hostname::parse(""), Err(HostnameError::Empty));
        assert_eq!(Hostname::parse("localhost"), Err(HostnameError::NoDomain));
        assert_eq!(Hostname::parse("a.*.com"), Err(HostnameError::BadWildcard));
        assert_eq!(Hostname::parse("*x.com"), Err(HostnameError::BadWildcard));
        assert_eq!(Hostname::parse("-a.com"), Err(HostnameError::Invalid));
        assert_eq!(Hostname::parse("a..com"), Err(HostnameError::Invalid));
        assert_eq!(Hostname::parse("a_b.com"), Err(HostnameError::Invalid));
        assert_eq!(
            Hostname::parse(&format!("{}.com", "a".repeat(64))),
            Err(HostnameError::Invalid)
        );
        let long = format!("{}com", "abcdefghi.".repeat(26));
        assert_eq!(Hostname::parse(&long), Err(HostnameError::TooLong));
    }

    #[test]
    fn longest_zone_wins() {
        let zones = ["example.com", "dev.example.com", "other.com"];
        let host = Hostname::parse("a.dev.example.com").unwrap();
        assert_eq!(host.zone_in(&zones), Some(&"dev.example.com"));
        assert_eq!(
            Hostname::parse("example.com").unwrap().zone_in(&zones),
            Some(&"example.com")
        );
        assert_eq!(
            Hostname::parse("*.other.com").unwrap().zone_in(&zones),
            Some(&"other.com")
        );
        assert_eq!(
            Hostname::parse("notexample.com").unwrap().zone_in(&zones),
            None
        );
    }

    proptest::proptest! {
        #[test]
        fn parsing_is_idempotent(input in "[a-zA-Z0-9.*-]{0,40}") {
            if let Ok(host) = Hostname::parse(&input) {
                proptest::prop_assert_eq!(Hostname::parse(host.as_str()), Ok(host));
            }
        }
    }
}
