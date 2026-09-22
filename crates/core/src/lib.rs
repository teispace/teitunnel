//! Teitunnel's product core.
//!
//! A plain async Rust API with no dependency on Tauri, so it can be unit-tested in
//! isolation and reused by a future CLI. The desktop shell is a thin adapter over it.
//!
//! See `docs/ARCHITECTURE.md` for the design of each module.

pub mod discovery;
pub mod doctor;
pub mod domain;
pub mod engine;
mod error;
pub mod platform;
pub mod redact;
pub mod runtime;
mod secret;
pub mod secrets;
pub mod store;

pub use error::{Error, Result};
pub use secret::Secret;
