//! Everything about the `cloudflared` binary.
//!
//! Locating and installing it (with checksum and signature verification), building
//! commands as discrete arguments (never through a shell), parsing its JSON logs and
//! Prometheus metrics, and talking to its local endpoints (`/ready`, `/quicktunnel`,
//! `/metrics`).

pub mod command;
pub mod endpoints;
mod error;
pub mod install;
pub mod locate;
pub mod log_parse;
pub mod metrics;
mod version;

pub use command::{
    CommandSpec, LogLevel, Protocol, QuickTunnelCmd, RunCmd, TokenSource, TunnelToken,
};
pub use endpoints::{Endpoints, Ready};
pub use error::{Error, Result};
pub use locate::{BinarySource, BinaryStatus, Locator, MIN_SUPPORTED};
pub use log_parse::{EventKind, Level, LogEvent, parse_line};
pub use metrics::{MetricsSnapshot, Sample};
pub use version::Version;
