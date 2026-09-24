use std::fmt;

/// A value that must never be logged or displayed by accident.
///
/// `Debug` and `Display` print `[redacted]`, and there is no `Serialize`. Lens keeps its
/// own copy of this type (rather than using `teitunnel-core`'s) because it must not
/// depend on core; the two behave identically.
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

    #[test]
    fn never_prints_the_value() {
        let secret = Secret::new("hunter2".to_owned());
        assert_eq!(format!("{secret:?}"), "Secret([redacted])");
        assert_eq!(secret.to_string(), "[redacted]");
        assert_eq!(secret.expose(), "hunter2");
    }
}
