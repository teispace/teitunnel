use std::fmt;

/// A value that must never be logged, displayed or serialized by accident.
///
/// `Debug` and `Display` print `[redacted]`. There is deliberately no `Serialize`
/// implementation, so a secret can't cross IPC or land in a JSON file. Read the
/// value only where it is actually used, via [`Secret::expose`].
#[derive(Clone, PartialEq, Eq)]
pub struct Secret<T>(T);

impl<T> Secret<T> {
    /// Wraps a sensitive value.
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrows the sensitive value. Keep the borrow as short as possible.
    pub const fn expose(&self) -> &T {
        &self.0
    }
}

impl<T> From<T> for Secret<T> {
    fn from(value: T) -> Self {
        Self(value)
    }
}

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret([redacted])")
    }
}

impl<T> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "cf-token-abcdef0123456789";

    #[test]
    fn debug_and_display_redact() {
        let secret = Secret::new(TOKEN.to_owned());
        assert_eq!(format!("{secret:?}"), "Secret([redacted])");
        assert_eq!(secret.to_string(), "[redacted]");
        assert!(!format!("{secret:#?}").contains(TOKEN));
    }

    #[test]
    fn redacts_inside_containing_structs() {
        #[derive(Debug)]
        #[allow(dead_code)]
        struct Credential {
            account: &'static str,
            token: Secret<String>,
        }
        let c = Credential {
            account: "acc",
            token: Secret::from(TOKEN.to_owned()),
        };
        let printed = format!("{c:?}");
        assert!(printed.contains("acc"));
        assert!(!printed.contains(TOKEN));
    }

    #[test]
    fn expose_returns_inner_value() {
        assert_eq!(Secret::new(TOKEN).expose(), &TOKEN);
    }
}
