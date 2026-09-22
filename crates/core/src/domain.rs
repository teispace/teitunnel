//! Domain model: accounts, zones, tunnels, routes, origins and Quick Shares.
//!
//! Types here are validated on construction (parse, don't validate), so the rest of the
//! core can rely on them being well-formed.

mod origin;

pub use origin::{OriginError, OriginUrl};
