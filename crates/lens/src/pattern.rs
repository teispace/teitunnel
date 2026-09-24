//! Path patterns for stubs and gate bypass lists.

use std::fmt;

use regex::Regex;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::LensError;

/// Matches a request path (without the query string).
///
/// Written as text: `/exact`, a glob with `*` (any characters, including `/`) and `?`
/// (one character), e.g. `/webhooks/*`, or a regular expression prefixed with `re:`,
/// e.g. `re:^/api/v[12]/`.
#[derive(Clone)]
pub enum PathPattern {
    /// The path must equal this.
    Exact(String),
    /// A glob with `*` and `?`.
    Glob(String),
    /// A regular expression, searched anywhere in the path unless anchored.
    Regex(Regex),
}

impl PathPattern {
    /// Parses the textual form (see the type docs).
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`] for an invalid regular expression or a pattern that
    /// doesn't start with `/` (globs and exact paths).
    pub fn parse(text: &str) -> Result<Self, LensError> {
        if let Some(re) = text.strip_prefix("re:") {
            return Regex::new(re)
                .map(Self::Regex)
                .map_err(|err| LensError::InvalidConfig(format!("path regex {re:?}: {err}")));
        }
        if !text.starts_with('/') && text != "*" {
            return Err(LensError::InvalidConfig(format!(
                "path pattern {text:?} must start with '/'"
            )));
        }
        if text.contains(['*', '?']) {
            Ok(Self::Glob(text.to_owned()))
        } else {
            Ok(Self::Exact(text.to_owned()))
        }
    }

    /// Whether `path` matches.
    pub fn matches(&self, path: &str) -> bool {
        match self {
            Self::Exact(exact) => exact == path,
            Self::Glob(glob) => glob_match(glob.as_bytes(), path.as_bytes()),
            Self::Regex(re) => re.is_match(path),
        }
    }

    /// The textual form, as accepted by [`PathPattern::parse`].
    pub fn as_text(&self) -> String {
        match self {
            Self::Exact(text) | Self::Glob(text) => text.clone(),
            Self::Regex(re) => format!("re:{}", re.as_str()),
        }
    }
}

impl fmt::Debug for PathPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "PathPattern({})", self.as_text())
    }
}

impl PartialEq for PathPattern {
    fn eq(&self, other: &Self) -> bool {
        self.as_text() == other.as_text()
    }
}

impl Eq for PathPattern {}

impl Serialize for PathPattern {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_text())
    }
}

impl<'de> Deserialize<'de> for PathPattern {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// Iterative glob matching with backtracking on the last `*` (linear in practice, no
/// recursion, so hostile patterns or paths can't blow the stack).
fn glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        match pattern.get(p) {
            Some(b'*') => {
                star = Some((p, t));
                p += 1;
            }
            Some(&c) if c == b'?' || c == text[t] => {
                p += 1;
                t += 1;
            }
            _ => match star {
                Some((sp, st)) => {
                    p = sp + 1;
                    t = st + 1;
                    star = Some((sp, st + 1));
                }
                None => return false,
            },
        }
    }
    pattern[p.min(pattern.len())..].iter().all(|&c| c == b'*')
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn pattern(text: &str) -> PathPattern {
        PathPattern::parse(text).unwrap_or_else(|_| PathPattern::Exact(String::new()))
    }

    #[test]
    fn exact_glob_and_regex() {
        assert!(pattern("/health").matches("/health"));
        assert!(!pattern("/health").matches("/healthz"));
        assert!(pattern("/webhooks/*").matches("/webhooks/stripe"));
        assert!(pattern("/webhooks/*").matches("/webhooks/a/b"));
        assert!(!pattern("/webhooks/*").matches("/webhook"));
        assert!(pattern("/v?/users").matches("/v1/users"));
        assert!(pattern("*").matches("/anything"));
        assert!(pattern("/*.json").matches("/a/b.json"));
        assert!(pattern("re:^/api/v[12]/").matches("/api/v2/x"));
        assert!(!pattern("re:^/api/v[12]/").matches("/api/v3/x"));
    }

    #[test]
    fn rejects_bad_patterns() {
        assert!(PathPattern::parse("webhooks").is_err());
        assert!(PathPattern::parse("re:(").is_err());
    }

    #[test]
    fn serde_round_trip() {
        let original = pattern("re:^/a");
        let json = serde_json::to_string(&original).unwrap_or_default();
        assert_eq!(json, "\"re:^/a\"");
        let back: PathPattern = serde_json::from_str(&json).unwrap_or_else(|_| pattern("/"));
        assert_eq!(back, original);
    }

    proptest! {
        #[test]
        fn glob_never_panics(p in "[/a-z*?]{0,24}", t in "[/a-z]{0,48}") {
            let _ = glob_match(p.as_bytes(), t.as_bytes());
        }

        #[test]
        fn star_matches_everything_with_prefix(prefix in "/[a-z]{0,8}", rest in "[/a-z.]{0,32}") {
            let glob = format!("{prefix}*");
            let text = format!("{prefix}{rest}");
            prop_assert!(glob_match(glob.as_bytes(), text.as_bytes()));
        }

        #[test]
        fn literal_glob_is_equality(a in "/[a-z]{0,12}", b in "/[a-z]{0,12}") {
            prop_assert_eq!(glob_match(a.as_bytes(), b.as_bytes()), a == b);
        }
    }
}
