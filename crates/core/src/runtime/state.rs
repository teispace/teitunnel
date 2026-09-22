use std::{fmt, sync::Arc};

use cloudflared::LogEvent;
use serde::Serialize;

/// Identifies a connector within the supervisor, e.g. `qs-<uuid>` or a tunnel id.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ConnectorId(pub String);

impl fmt::Display for ConnectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Lifecycle of one connector (ARCHITECTURE §5.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ConnectorState {
    /// Not running.
    Stopped,
    /// The process is being spawned.
    Starting,
    /// Running, but no edge connection yet.
    Connecting,
    /// Serving traffic.
    Healthy {
        /// Registered edge connections.
        connections: u32,
    },
    /// Running with no edge connections after having had some (or after the start
    /// timeout).
    Degraded,
    /// Exited unexpectedly; restarts after a backoff.
    Crashed {
        /// Restart attempt number (1-based).
        attempt: u32,
        /// Milliseconds until the restart.
        retry_in_ms: u64,
        /// Exit code, when there was one.
        exit_code: Option<i32>,
    },
    /// Crashed too often in a short time; not restarted automatically.
    CrashLoop {
        /// Exit code of the last crash.
        exit_code: Option<i32>,
    },
    /// Shutting down.
    Stopping,
}

/// Something that happened in the runtime.
#[derive(Debug, Clone)]
pub enum RuntimeEvent {
    /// A connector changed state.
    State {
        /// Which connector.
        id: ConnectorId,
        /// Its new state.
        state: ConnectorState,
    },
    /// A connector logged a line.
    Log {
        /// Which connector.
        id: ConnectorId,
        /// The parsed line.
        event: Arc<LogEvent>,
    },
}
