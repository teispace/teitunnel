//! App updates (M6-02, D-075): when to check, and the status the app shows. Checking,
//! downloading and installing are Tauri's updater plugin, in the shell; every update is
//! verified against the public key built into the app before it's installed.
//!
//! A check fetches one static file (`latest.json` of the latest GitHub release) and
//! sends nothing about the user (D-019).

use std::time::Duration;

use serde::Serialize;

use crate::text::{Text, msg::updates as m};

/// Wait after launch before the first check, so it never competes with startup.
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(15);

/// Time between automatic checks while the app runs.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Where an update stands.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum UpdateState {
    /// Nothing checked yet.
    Idle,
    /// Asking whether there's a newer version.
    Checking,
    /// This is the latest version.
    UpToDate,
    /// Downloading a newer version.
    #[serde(rename_all = "camelCase")]
    Downloading {
        /// The version being downloaded.
        version: String,
        /// Share downloaded, 0–1, when the size is known.
        progress: Option<f64>,
    },
    /// Downloaded and verified; installs on restart (or on quit, see `install_on_quit`).
    #[serde(rename_all = "camelCase")]
    Ready {
        /// The new version.
        version: String,
        /// Its release notes (Markdown).
        notes: Option<String>,
    },
    /// Checking or downloading failed; the next check tries again.
    Failed {
        /// Why.
        message: Text,
    },
}

/// What the app shows about updates.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    /// The running version.
    pub current_version: String,
    /// Why this copy can't update itself, if it can't (checks are off then).
    pub unsupported: Option<Text>,
    /// Automatic checks are on.
    pub automatic: bool,
    /// When the last check finished (Unix ms).
    pub last_checked: Option<f64>,
    /// Where an update stands.
    pub state: UpdateState,
    /// A ready update installs when the app quits.
    pub install_on_quit: bool,
}

/// How this copy of the app was installed, as far as updating goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// An app bundle, AppImage or per-user installer: replaced without asking.
    InPlace,
    /// A `.deb` or `.rpm` package: installing asks for an administrator password.
    SystemPackage,
    /// A development or test build: never updates.
    Development,
    /// Anything else (e.g. a binary run from a build directory).
    Unknown,
}

impl Install {
    /// Why it can't update itself, if it can't.
    pub fn unsupported(self) -> Option<Text> {
        match self {
            Self::InPlace | Self::SystemPackage => None,
            Self::Development => Some(m::development()),
            Self::Unknown => Some(m::unknown_install()),
        }
    }

    /// Whether a ready update installs by itself when the app quits. A system package
    /// would ask for a password at quit, so it waits for "Restart to Update" instead.
    pub fn installs_on_quit(self) -> bool {
        self == Self::InPlace
    }
}

/// Whether an automatic check is due, `since_last` after the previous one (`None`: none
/// yet).
pub fn check_due(automatic: bool, since_last: Option<Duration>) -> bool {
    automatic && since_last.is_none_or(|elapsed| elapsed >= CHECK_EVERY)
}

/// The share of `total` that `downloaded` is, when the total is known.
#[allow(clippy::cast_precision_loss)] // a progress bar, not accounting
pub fn progress(downloaded: u64, total: Option<u64>) -> Option<f64> {
    total
        .filter(|total| *total > 0)
        .map(|total| (downloaded as f64 / total as f64).clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checks_daily_and_only_when_on() {
        assert!(check_due(true, None));
        assert!(!check_due(false, None));
        assert!(!check_due(true, Some(Duration::from_secs(3600))));
        assert!(check_due(true, Some(CHECK_EVERY)));
    }

    #[test]
    fn packages_wait_for_the_user_and_dev_builds_never_update() {
        assert!(Install::InPlace.installs_on_quit());
        assert!(!Install::SystemPackage.installs_on_quit());
        assert!(Install::SystemPackage.unsupported().is_none());
        assert!(Install::Development.unsupported().is_some());
        assert!(Install::Unknown.unsupported().is_some());
    }

    #[test]
    fn progress_is_a_clamped_share() {
        assert_eq!(progress(5, Some(10)), Some(0.5));
        assert_eq!(progress(20, Some(10)), Some(1.0));
        assert_eq!(progress(5, None), None);
        assert_eq!(progress(5, Some(0)), None);
    }
}
