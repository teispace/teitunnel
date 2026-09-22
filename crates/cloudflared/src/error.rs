/// Errors produced while working with the `cloudflared` binary.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A version string did not match `YYYY.M.P`.
    #[error("unrecognised cloudflared version: {0:?}")]
    InvalidVersion(String),
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
