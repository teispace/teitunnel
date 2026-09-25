//! Teitunnel's local control connection: how the CLI, editor extensions and launchers
//! talk to the running app.
//!
//! - [`protocol`]: newline-delimited JSON-RPC 2.0, versioned by [`protocol::PROTOCOL_VERSION`].
//! - [`endpoint`]: a Unix domain socket (macOS, Linux) or a named pipe (Windows) that
//!   only the current user can open, plus a per-install token file.
//! - [`Server`]: authenticates clients (`hello` with the token), bounds messages,
//!   rate-limits, times out, and asks the person before any change unless they allowed
//!   the client in Settings ▸ Integrations. It answers from a [`Host`].
//! - [`ControlClient`]: the client the CLI uses.
//! - [`deeplink`]: `teitunnel://` links, handled through the same [`Host`].
//!
//! Nothing here listens on TCP, and nothing here knows Teitunnel's core: the core
//! implements [`Host`].

pub mod client;
pub mod deeplink;
pub mod endpoint;
mod framing;
pub mod host;
pub mod protocol;
pub mod server;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(test)]
mod tests;

pub use client::{ClientError, ControlClient};
pub use endpoint::{Endpoint, Token};
pub use host::{Action, BoxFuture, ConfirmRequest, Decision, Host, HostResult, Requester};
pub use server::{Limits, Server};
