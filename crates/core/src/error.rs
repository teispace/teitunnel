/// Errors produced by the Teitunnel core.
///
/// The desktop shell maps these into the IPC `AppError` (code, message, hint).
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Cloudflare API call failed.
    #[error(transparent)]
    CloudApi(#[from] cf_api::Error),
    /// Working with the cloudflared binary failed.
    #[error(transparent)]
    Cloudflared(#[from] cloudflared::Error),
    /// A Quick Share operation failed.
    #[error(transparent)]
    QuickShare(#[from] crate::quick_share::QuickShareError),
    /// A connector operation failed.
    #[error(transparent)]
    Runtime(#[from] crate::runtime::SupervisorError),
    /// The local database failed.
    #[error(transparent)]
    Store(#[from] crate::store::StoreError),
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;
