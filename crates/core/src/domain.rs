//! Domain model: accounts, zones, tunnels, routes, origins and Quick Shares.
//!
//! Types here are validated on construction (parse, don't validate), so the rest of the
//! core can rely on them being well-formed.

mod hostname;
mod origin;
mod path_rule;
mod route_origin;

pub use hostname::{Hostname, HostnameError};
pub use origin::{OriginError, OriginUrl};
pub use path_rule::{PathError, PathRule};
pub use route_origin::{RouteOrigin, RouteOriginError};
