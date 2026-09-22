//! Typed client for the Cloudflare API v4.
//!
//! This crate knows HTTP and JSON, and the shapes of the endpoints Teitunnel uses
//! (accounts, zones, DNS records, tunnels and their configurations). It knows nothing
//! about Teitunnel's product model; that lives in `teitunnel-core`.

mod client;
mod envelope;
mod error;
mod probe;
mod resources;
mod token;

pub use client::{API_BASE, Client};
pub use envelope::{ApiMessage, Envelope, ResultInfo};
pub use error::{Error, Result};
pub use probe::{Access, NIL_ID, NIL_UUID};
pub use resources::{Account, AccountRef, Plan, TokenStatus, Zone, ZoneStatus};
pub use token::ApiToken;
