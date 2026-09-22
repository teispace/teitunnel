//! Typed client for the Cloudflare API v4.
//!
//! This crate knows HTTP and JSON, and the shapes of the endpoints Teitunnel uses
//! (accounts, zones, DNS records, tunnels and their configurations). It knows nothing
//! about Teitunnel's product model; that lives in `teitunnel-core`.

mod envelope;
mod error;

pub use envelope::{ApiMessage, Envelope};
pub use error::{Error, Result};
