//! Everything about the `cloudflared` binary.
//!
//! Locating and installing it (with checksum and signature verification), building
//! commands as discrete arguments (never through a shell), parsing its JSON logs and
//! Prometheus metrics, and talking to its local endpoints (`/ready`, `/quicktunnel`,
//! `/metrics`).

mod error;
mod version;

pub use error::{Error, Result};
pub use version::Version;
