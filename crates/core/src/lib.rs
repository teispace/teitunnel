//! Teitunnel's product core.
//!
//! A plain async Rust API with no dependency on Tauri, so it can be unit-tested in
//! isolation and reused by a future CLI. The desktop shell is a thin adapter over it.
//!
//! See `docs/ARCHITECTURE.md` for the design of each module.

pub mod accounts;
pub mod alerts;
pub mod analytics;
pub mod binary;
pub mod cli_install;
pub mod cli_shares;
pub mod connector_logs;
pub mod diagnostics;
pub mod discovery;
pub mod doctor;
pub mod doctor_monitor;
pub mod domain;
pub mod domain_shares;
pub mod engine;
mod error;
pub mod export;
pub mod health;
pub mod import;
pub mod machine;
pub mod platform;
pub mod quick_share;
pub mod redact;
pub mod remote_logs;
pub mod runtime;
mod secret;
pub mod secrets;
pub mod service;
pub mod settings;
pub mod store;
pub mod text;
pub mod traffic;
pub mod updates;
pub mod uptime;
pub mod web_auth;

pub use cloudflared::Error as CloudflaredError;
pub use error::{Error, ErrorKind, Result};
pub use secret::Secret;
