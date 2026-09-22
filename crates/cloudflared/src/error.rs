/// Errors produced while working with the `cloudflared` binary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A version string did not match `YYYY.M.P`.
    #[error("unrecognised cloudflared version: {0:?}")]
    InvalidVersion(String),
    /// No usable cloudflared binary was found.
    #[error("cloudflared isn't installed")]
    NotFound,
    /// Running the binary failed.
    #[error("couldn't run cloudflared: {0}")]
    Io(#[from] std::io::Error),
    /// The binary exited unsuccessfully.
    #[error("cloudflared exited with status {0:?}")]
    Exited(Option<i32>),
    /// An HTTP request failed.
    #[error("request failed: {0}")]
    Http(String),
    /// An operation took too long.
    #[error("{0} timed out")]
    Timeout(&'static str),
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
