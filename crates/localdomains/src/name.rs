//! Validated local host names (`app.localhost`, `api.test`, `phone.local`).
//!
//! Construction is the validation: a [`LocalName`] is lowercase ASCII, has at least one
//! label in front of an allowed [`Suffix`], and every label follows the LDH rules. Names
//! outside the three special-use suffixes can't be represented, so nothing downstream
//! (certificates, DNS answers, mDNS) can be asked to serve a real internet name.

use std::fmt;

use serde::{Deserialize, Serialize};

/// Longest DNS name in presentation form, without the trailing dot (RFC 1035 §2.3.4).
const MAX_NAME_LEN: usize = 253;
/// Longest DNS label (RFC 1035 §2.3.4).
const MAX_LABEL_LEN: usize = 63;

/// A special-use top-level suffix Teitunnel may serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Suffix {
    /// `.localhost` (RFC 6761 §6.3): browsers send it to loopback with no setup.
    Localhost,
    /// `.test` (RFC 6761 §6.2): needs Teitunnel's DNS responder and a resolver entry.
    Test,
    /// `.local` (RFC 6762): multicast DNS on the LAN.
    Local,
}

impl Suffix {
    /// Every suffix, in preference order.
    pub const ALL: [Self; 3] = [Self::Localhost, Self::Test, Self::Local];

    /// The suffix label, without dots.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Localhost => "localhost",
            Self::Test => "test",
            Self::Local => "local",
        }
    }

    fn of(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == label)
    }
}

impl fmt::Display for Suffix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a name was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NameError {
    /// The name is empty.
    #[error("the name is empty")]
    Empty,
    /// The name is longer than 253 characters.
    #[error("the name is longer than 253 characters")]
    TooLong,
    /// A label is empty, too long, or has characters other than letters, digits and inner
    /// hyphens.
    #[error("invalid label {0:?}: use letters, digits and inner hyphens, at most 63 characters")]
    InvalidLabel(String),
    /// Wildcards are a property of a domain (see `LocalDomain::wildcard`), not part of its
    /// name.
    #[error("wildcards aren't allowed in a name")]
    Wildcard,
    /// The name doesn't end in one of the allowed suffixes.
    #[error("the name must end in one of: {allowed}")]
    SuffixNotAllowed {
        /// The allowed suffixes, comma-separated, for the message.
        allowed: String,
    },
    /// The name is only a suffix (`localhost`), with nothing in front of it.
    #[error("add a name in front of .{0}")]
    BareSuffix(Suffix),
}

/// A validated, lowercase local host name such as `app.localhost`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LocalName {
    name: String,
    suffix: Suffix,
}

impl LocalName {
    /// Parses a name, accepting only the `allowed` suffixes. Case is folded and one trailing
    /// dot (DNS form) is dropped.
    ///
    /// # Errors
    /// See [`NameError`].
    pub fn parse(input: &str, allowed: &[Suffix]) -> Result<Self, NameError> {
        let trimmed = input.strip_suffix('.').unwrap_or(input);
        if trimmed.is_empty() {
            return Err(NameError::Empty);
        }
        if trimmed.len() > MAX_NAME_LEN {
            return Err(NameError::TooLong);
        }
        let name = trimmed.to_ascii_lowercase();
        for label in name.split('.') {
            if label == "*" {
                return Err(NameError::Wildcard);
            }
            if !valid_label(label) {
                return Err(NameError::InvalidLabel(label.to_owned()));
            }
        }
        let last = name.rsplit('.').next().unwrap_or_default();
        let not_allowed = || NameError::SuffixNotAllowed {
            allowed: allowed
                .iter()
                .map(|s| format!(".{s}"))
                .collect::<Vec<_>>()
                .join(", "),
        };
        let suffix = Suffix::of(last).ok_or_else(not_allowed)?;
        if !allowed.contains(&suffix) {
            return Err(not_allowed());
        }
        if name.len() == suffix.as_str().len() {
            return Err(NameError::BareSuffix(suffix));
        }
        Ok(Self { name, suffix })
    }

    /// Parses a name with any of the three suffixes.
    ///
    /// # Errors
    /// See [`NameError`].
    pub fn parse_any(input: &str) -> Result<Self, NameError> {
        Self::parse(input, &Suffix::ALL)
    }

    /// The name, lowercase, without a trailing dot.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.name
    }

    /// The special-use suffix the name ends in.
    #[must_use]
    pub fn suffix(&self) -> Suffix {
        self.suffix
    }

    /// The first (leftmost) label.
    #[must_use]
    pub fn first_label(&self) -> &str {
        self.name.split('.').next().unwrap_or_default()
    }

    /// The name one level up (`app.localhost` for `api.app.localhost`), unless that would be
    /// the bare suffix.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        let (_, rest) = self.name.split_once('.')?;
        (rest != self.suffix.as_str()).then(|| Self {
            name: rest.to_owned(),
            suffix: self.suffix,
        })
    }

    /// The wildcard sibling that covers one level below this name (`*.app.localhost`).
    #[must_use]
    pub fn wildcard(&self) -> String {
        format!("*.{}", self.name)
    }

    /// Whether `self` is exactly one label below `other` (what `*.other` matches).
    #[must_use]
    pub fn is_child_of(&self, other: &Self) -> bool {
        self.parent().as_ref() == Some(other)
    }
}

impl fmt::Display for LocalName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.name)
    }
}

impl TryFrom<String> for LocalName {
    type Error = NameError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse_any(&value)
    }
}

impl From<LocalName> for String {
    fn from(value: LocalName) -> Self {
        value.name
    }
}

fn valid_label(label: &str) -> bool {
    let bytes = label.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= MAX_LABEL_LEN
        && bytes
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && bytes.first() != Some(&b'-')
        && bytes.last() != Some(&b'-')
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn accepts_and_normalises() {
        let name = LocalName::parse_any("App.LocalHost.").unwrap();
        assert_eq!(name.as_str(), "app.localhost");
        assert_eq!(name.suffix(), Suffix::Localhost);
        assert_eq!(name.first_label(), "app");
        assert_eq!(name.wildcard(), "*.app.localhost");
    }

    #[test]
    fn refuses_bad_names() {
        let only_localhost = [Suffix::Localhost];
        assert_eq!(LocalName::parse_any(""), Err(NameError::Empty));
        assert_eq!(LocalName::parse_any("."), Err(NameError::Empty));
        assert_eq!(
            LocalName::parse_any("localhost"),
            Err(NameError::BareSuffix(Suffix::Localhost))
        );
        assert!(matches!(
            LocalName::parse_any("example.com"),
            Err(NameError::SuffixNotAllowed { .. })
        ));
        assert!(matches!(
            LocalName::parse("app.test", &only_localhost),
            Err(NameError::SuffixNotAllowed { .. })
        ));
        assert_eq!(
            LocalName::parse_any("*.app.localhost"),
            Err(NameError::Wildcard)
        );
        for bad in [
            "-a.localhost",
            "a-.localhost",
            "a..localhost",
            "a_b.localhost",
            "é.localhost",
        ] {
            assert!(
                matches!(LocalName::parse_any(bad), Err(NameError::InvalidLabel(_))),
                "{bad}"
            );
        }
        let long_label = format!("{}.localhost", "a".repeat(64));
        assert!(matches!(
            LocalName::parse_any(&long_label),
            Err(NameError::InvalidLabel(_))
        ));
        let long_name = format!("{}localhost", "abcdefghi.".repeat(25));
        assert_eq!(LocalName::parse_any(&long_name), Err(NameError::TooLong));
    }

    #[test]
    fn parents_and_children() {
        let api = LocalName::parse_any("api.app.localhost").unwrap();
        let app = LocalName::parse_any("app.localhost").unwrap();
        assert_eq!(api.parent(), Some(app.clone()));
        assert_eq!(app.parent(), None);
        assert!(api.is_child_of(&app));
        let deep = LocalName::parse_any("a.api.app.localhost").unwrap();
        assert!(!deep.is_child_of(&app));
    }

    #[test]
    fn serde_round_trip_validates() {
        let name = LocalName::parse_any("app.test").unwrap();
        let json = serde_json::to_string(&name).unwrap();
        assert_eq!(json, "\"app.test\"");
        assert_eq!(serde_json::from_str::<LocalName>(&json).unwrap(), name);
        assert!(serde_json::from_str::<LocalName>("\"evil.com\"").is_err());
    }

    proptest! {
        #[test]
        fn parse_never_panics_and_output_is_valid(input in ".{0,300}") {
            if let Ok(name) = LocalName::parse_any(&input) {
                prop_assert!(name.as_str().len() <= MAX_NAME_LEN);
                prop_assert!(name.as_str().split('.').all(valid_label));
                prop_assert_eq!(LocalName::parse_any(name.as_str()), Ok(name.clone()));
            }
        }
    }
}
