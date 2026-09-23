//! Domain model: accounts, zones, tunnels, routes, origins and Quick Shares.
//!
//! Types here are validated on construction (parse, don't validate), so the rest of the
//! core can rely on them being well-formed.

mod client_access;
mod hostname;
mod origin;
mod path_rule;
mod private_network;
mod route_origin;

pub use client_access::{ClientAccess, ClientProtocol};
pub use hostname::{Hostname, HostnameError};
pub use origin::{OriginError, OriginUrl};
pub use path_rule::{PathError, PathRule};
pub use private_network::{PrivateNetwork, PrivateNetworkError};
pub use route_origin::{RouteOrigin, RouteOriginError};
