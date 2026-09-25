//! Which OS a plan is built for. Builders take it as a value so every platform's argv can
//! be unit-tested on any machine.

use serde::{Deserialize, Serialize};

/// A desktop OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Platform {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux (and other Unix desktops treated like it).
    Linux,
}

impl Platform {
    /// The platform this build runs on.
    #[must_use]
    pub const fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::Macos
        } else if cfg!(windows) {
            Self::Windows
        } else {
            Self::Linux
        }
    }
}
