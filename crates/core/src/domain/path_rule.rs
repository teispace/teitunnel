use serde::{Deserialize, Serialize};

use crate::text::{Text, UserText, english_display, msg};

/// Why a path rule was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// Not a valid regular expression.
    Invalid(String),
    /// Uses syntax Go's RE2 (cloudflared) doesn't support.
    Unsupported,
}

impl UserText for PathError {
    fn text(&self) -> Text {
        match self {
            Self::Invalid(detail) => msg::error::path::invalid(detail),
            Self::Unsupported => msg::error::path::unsupported(),
        }
    }
}

english_display!(PathError);

/// A path regex for an ingress rule, e.g. `^/api/`. Validated with Rust's `regex`
/// (the same RE2-style syntax family as cloudflared's Go regexp).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(try_from = "String", into = "String")]
pub struct PathRule(String);

impl PathRule {
    /// Validates a path pattern.
    ///
    /// # Errors
    /// See [`PathError`].
    pub fn parse(input: &str) -> Result<Self, PathError> {
        let pattern = input.trim();
        if ["(?=", "(?!", "(?<=", "(?<!"]
            .iter()
            .any(|s| pattern.contains(s))
            || pattern.contains("\\1")
        {
            return Err(PathError::Unsupported);
        }
        regex::Regex::new(pattern).map_err(|err| PathError::Invalid(err.to_string()))?;
        Ok(Self(pattern.to_owned()))
    }

    /// The pattern.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for PathRule {
    type Error = PathError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::parse(&value)
    }
}

impl From<PathRule> for String {
    fn from(path: PathRule) -> Self {
        path.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_patterns() {
        assert!(PathRule::parse("^/api/").is_ok());
        assert!(PathRule::parse("\\.(png|jpg)$").is_ok());
        assert!(matches!(
            PathRule::parse("(unclosed"),
            Err(PathError::Invalid(_))
        ));
        assert_eq!(PathRule::parse("^/(?!admin)"), Err(PathError::Unsupported));
    }

    proptest::proptest! {
        /// Whatever is typed, parsing never panics, and a valid rule parses back to itself.
        #[test]
        fn never_panics_and_reparses(input in ".{0,40}") {
            if let Ok(rule) = PathRule::parse(&input) {
                let again = PathRule::parse(rule.as_str()).expect("a valid rule stays valid");
                proptest::prop_assert_eq!(again, rule);
            }
        }
    }
}
