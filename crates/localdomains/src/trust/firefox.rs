//! Firefox on macOS and Windows: `security.enterprise_roots.enabled` makes Firefox trust
//! roots the user added to the OS store. It has been on by default since Firefox 120, so
//! Teitunnel only writes it (in each profile's `user.js`, with the user's explicit consent)
//! when the user asks, e.g. because it was switched off.

use std::{fs, io, path::Path};

/// The exact line Teitunnel adds to `user.js`.
pub const ENTERPRISE_ROOTS_LINE: &str = r#"user_pref("security.enterprise_roots.enabled", true); // added by Teitunnel for local HTTPS domains"#;

/// How a profile treats OS-trusted roots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnterpriseRoots {
    /// Teitunnel's line is in `user.js`.
    SetByTeitunnel,
    /// The user (or policy) turned it off in `prefs.js`.
    Disabled,
    /// Not set: Firefox's default applies (on since Firefox 120).
    Default,
}

/// Reads a profile's setting.
#[must_use]
pub fn state(profile: &Path) -> EnterpriseRoots {
    let has_line = fs::read_to_string(profile.join("user.js"))
        .is_ok_and(|text| text.lines().any(|l| l.trim() == ENTERPRISE_ROOTS_LINE));
    if has_line {
        return EnterpriseRoots::SetByTeitunnel;
    }
    let disabled = fs::read_to_string(profile.join("prefs.js")).is_ok_and(|text| {
        text.lines().any(|l| {
            let l: String = l.chars().filter(|c| !c.is_whitespace()).collect();
            l.starts_with(r#"user_pref("security.enterprise_roots.enabled",false)"#)
        })
    });
    if disabled {
        EnterpriseRoots::Disabled
    } else {
        EnterpriseRoots::Default
    }
}

/// Adds Teitunnel's line to `user.js` (idempotent).
///
/// # Errors
/// Reading or writing `user.js` failed.
pub fn enable(profile: &Path) -> io::Result<()> {
    let path = profile.join("user.js");
    let mut text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => String::new(),
        Err(err) => return Err(err),
    };
    if text.lines().any(|l| l.trim() == ENTERPRISE_ROOTS_LINE) {
        return Ok(());
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(ENTERPRISE_ROOTS_LINE);
    text.push('\n');
    fs::write(path, text)
}

/// Removes Teitunnel's line from `user.js`, leaving anything else untouched.
///
/// # Errors
/// Reading or writing `user.js` failed.
pub fn disable(profile: &Path) -> io::Result<()> {
    let path = profile.join("user.js");
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(err),
    };
    let kept: Vec<&str> = text
        .lines()
        .filter(|l| l.trim() != ENTERPRISE_ROOTS_LINE)
        .collect();
    if kept.is_empty() {
        return fs::remove_file(path);
    }
    fs::write(path, kept.join("\n") + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_disable_round_trip_keeps_user_prefs() {
        let profile = tempfile::tempdir().unwrap();
        let p = profile.path();
        assert_eq!(state(p), EnterpriseRoots::Default);
        fs::write(p.join("user.js"), "user_pref(\"a\", 1);").unwrap();
        enable(p).unwrap();
        enable(p).unwrap();
        let text = fs::read_to_string(p.join("user.js")).unwrap();
        assert_eq!(text.matches("enterprise_roots").count(), 1);
        assert!(text.starts_with("user_pref(\"a\", 1);\n"));
        assert_eq!(state(p), EnterpriseRoots::SetByTeitunnel);
        disable(p).unwrap();
        assert_eq!(
            fs::read_to_string(p.join("user.js")).unwrap(),
            "user_pref(\"a\", 1);\n"
        );
        assert_eq!(state(p), EnterpriseRoots::Default);
    }

    #[test]
    fn disable_removes_a_file_we_created() {
        let profile = tempfile::tempdir().unwrap();
        enable(profile.path()).unwrap();
        disable(profile.path()).unwrap();
        assert!(!profile.path().join("user.js").exists());
        disable(profile.path()).unwrap();
    }

    #[test]
    fn detects_user_disabled() {
        let profile = tempfile::tempdir().unwrap();
        fs::write(
            profile.path().join("prefs.js"),
            "user_pref(\"security.enterprise_roots.enabled\", false);\n",
        )
        .unwrap();
        assert_eq!(state(profile.path()), EnterpriseRoots::Disabled);
    }
}
