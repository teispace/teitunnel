use std::{fmt, str::FromStr};

use crate::error::Error;

/// A cloudflared release version (`YYYY.M.P`, e.g. `2026.9.0`).
///
/// Ordering follows release order, so minimum-version checks are plain comparisons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    /// Release year.
    pub year: u16,
    /// Release month (1–12).
    pub month: u8,
    /// Patch number within the month.
    pub patch: u16,
}

impl Version {
    /// Extracts the version from `cloudflared --version` output, e.g.
    /// `cloudflared version 2026.9.0 (built 2026-09-02-1200 UTC)`.
    ///
    /// # Errors
    /// Returns [`Error::InvalidVersion`] when no version token is present.
    pub fn from_version_output(output: &str) -> Result<Self, Error> {
        output
            .split_whitespace()
            .find_map(|token| token.parse().ok())
            .ok_or_else(|| Error::InvalidVersion(output.trim().to_owned()))
    }
}

impl FromStr for Version {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || Error::InvalidVersion(s.to_owned());
        let mut parts = s.split('.');
        let (Some(year), Some(month), Some(patch), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(invalid());
        };
        let year: u16 = year.parse().map_err(|_| invalid())?;
        let month: u8 = month.parse().map_err(|_| invalid())?;
        let patch: u16 = patch.parse().map_err(|_| invalid())?;
        if !(2020..=2100).contains(&year) || !(1..=12).contains(&month) {
            return Err(invalid());
        }
        Ok(Self { year, month, patch })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.year, self.month, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_version_output() {
        let v = Version::from_version_output(
            "cloudflared version 2026.9.0 (built 2026-09-02-1200 UTC)",
        )
        .unwrap();
        assert_eq!(
            v,
            Version {
                year: 2026,
                month: 9,
                patch: 0
            }
        );
        assert_eq!(v.to_string(), "2026.9.0");
    }

    #[test]
    fn orders_by_release() {
        let a: Version = "2025.11.1".parse().unwrap();
        let b: Version = "2026.2.0".parse().unwrap();
        assert!(a < b);
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            "1.2",
            "2026.13.0",
            "2026.9.0.1",
            "v2026.9.0",
            "1999.1.0",
        ] {
            assert!(bad.parse::<Version>().is_err(), "{bad} should be rejected");
        }
        assert!(Version::from_version_output("command not found").is_err());
    }
}
