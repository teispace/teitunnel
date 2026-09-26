use std::{io, net::SocketAddr, path::PathBuf};

use crate::{ExchangeId, ListenerId, TapId};

/// Errors returned by Lens's public API.
///
/// Failures while proxying a request are not errors of the API: they are recorded on the
/// captured exchange ([`crate::ExchangeError`]) and answered with an error page.
#[derive(Debug, thiserror::Error)]
pub enum LensError {
    /// The listening socket couldn't be bound.
    #[error("couldn't listen on {addr}: {source}")]
    Bind {
        /// The address that was requested.
        addr: SocketAddr,
        /// The operating system's reason.
        source: io::Error,
    },
    /// A listener was asked to bind a non-loopback address without opting in.
    #[error(
        "{0} isn't a loopback address; Lens listens only on this computer unless told otherwise"
    )]
    NotLoopback(SocketAddr),
    /// An upstream URL was malformed or unsupported.
    #[error("invalid upstream: {0}")]
    InvalidUpstream(String),
    /// A folder upstream doesn't exist or isn't a directory.
    #[error("folder {path} can't be served: {reason}")]
    InvalidFolder {
        /// The folder that was requested.
        path: PathBuf,
        /// Why it can't be served.
        reason: String,
    },
    /// A configuration value was rejected (pattern, header, id…).
    #[error("invalid configuration: {0}")]
    InvalidConfig(String),
    /// No tap has this id.
    #[error("no tap with id {0}")]
    UnknownTap(TapId),
    /// A tap with this id already exists.
    #[error("a tap with id {0} already exists")]
    DuplicateTap(TapId),
    /// No listener has this id.
    #[error("no listener with id {0}")]
    UnknownListener(ListenerId),
    /// The exchange isn't paused at a breakpoint (any more).
    #[error("exchange {0} isn't paused")]
    NotPaused(ExchangeId),
    /// No captured exchange has this id (it may have been evicted from the ring).
    #[error("no captured exchange {0}")]
    UnknownExchange(ExchangeId),
    /// The captured request body is incomplete, so the request can't be sent again as-is.
    #[error(
        "the captured request body is truncated ({captured} of {total} bytes), so it can't be replayed without replacing the body"
    )]
    BodyTruncated {
        /// Bytes captured.
        captured: u64,
        /// Bytes the client sent.
        total: u64,
    },
    /// TLS configuration failed (e.g. the platform's certificate verifier is unavailable).
    #[error("TLS setup failed: {0}")]
    Tls(String),
    /// The operating system's random number generator failed.
    #[error("secure random numbers are unavailable: {0}")]
    Random(String),
    /// Password hashing failed.
    #[error("couldn't hash the password: {0}")]
    PasswordHash(String),
    /// Waiting for an exchange timed out.
    #[error("no matching request arrived in time")]
    WaitTimeout,
    /// The Lens runtime has been shut down.
    #[error("Lens has shut down")]
    Closed,
}

/// Shorthand for results of Lens's API.
pub type Result<T, E = LensError> = std::result::Result<T, E>;

impl From<getrandom::Error> for LensError {
    fn from(err: getrandom::Error) -> Self {
        Self::Random(err.to_string())
    }
}
