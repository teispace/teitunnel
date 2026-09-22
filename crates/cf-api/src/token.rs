use std::fmt;

/// A Cloudflare API token or OAuth access token. Redacted in `Debug`; there is no
/// `Display`, `Serialize` or conversion that could leak it by accident.
#[derive(Clone, PartialEq, Eq)]
pub struct ApiToken(String);

impl ApiToken {
    /// Wraps a token read from the keychain or pasted by the user.
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into().trim().to_owned())
    }

    pub(crate) fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for ApiToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiToken([redacted])")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_is_redacted_and_input_trimmed() {
        let token = ApiToken::new("  abc123\n");
        assert_eq!(format!("{token:?}"), "ApiToken([redacted])");
        assert_eq!(token.bearer(), "Bearer abc123");
    }
}
