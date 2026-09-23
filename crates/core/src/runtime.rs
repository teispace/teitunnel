//! Connector runtime: supervising cloudflared processes (Session mode), their health,
//! logs and metrics (ARCHITECTURE §5).
//!
//! Each connector is an actor task that owns one child process and drives the state
//! machine `Stopped → Starting → Connecting → Healthy/Degraded → Crashed/Stopping`.
//! The [`Supervisor`] owns the actors, fans their events out to subscribers, and makes
//! sure no process outlives the app (graceful stop on exit, pidfile reaping on launch).

mod connector;
mod logbuf;
mod policy;
mod ports;
mod registry;
mod signal;
mod state;
mod supervisor;

pub use cloudflared::LogEvent;
pub use logbuf::LogBuffer;
pub use policy::{CrashTracker, RestartPolicy};
pub use ports::{PortAllocator, QUICK_SHARE_PORTS, TUNNEL_PORTS};
pub(crate) use registry::stop_pid as stop_foreign;
pub use registry::{PidRegistry, is_running, this_process};

/// Asks a process to end (SIGTERM; on Windows it's terminated).
pub fn interrupt(pid: u32) {
    signal::signal_process(pid, false);
}
pub use state::{ConnectorId, ConnectorState, RuntimeEvent};
pub use supervisor::{ConnectorSpec, Supervisor, SupervisorError};
