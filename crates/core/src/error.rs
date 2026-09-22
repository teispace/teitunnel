/// Errors produced by the Teitunnel core.
///
/// The desktop shell maps these into the IPC `AppError` (code, message, hint) using
/// [`Error::kind`], so it never has to match on the internals of other crates.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A Cloudflare API call failed.
    #[error(transparent)]
    CloudApi(#[from] cf_api::Error),
    /// Working with the cloudflared binary failed.
    #[error(transparent)]
    Cloudflared(#[from] cloudflared::Error),
    /// An account operation failed.
    #[error(transparent)]
    Accounts(#[from] crate::accounts::AccountError),
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

/// A coarse classification of [`Error`] for user-facing handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// cloudflared isn't installed (or can't be run).
    CloudflaredMissing,
    /// The thing acted on doesn't exist (any more).
    NotFound,
    /// A resource is exhausted or busy; retrying later may work.
    Unavailable,
    /// The user's input was rejected; the message says why.
    InvalidInput,
    /// Anything else: a bug or an environment problem, logged with details.
    Internal,
}

impl Error {
    /// Classifies the error.
    pub fn kind(&self) -> ErrorKind {
        use crate::{
            accounts::AccountError as A, quick_share::QuickShareError as Q,
            runtime::SupervisorError as S,
        };
        match self {
            Self::Cloudflared(cloudflared::Error::NotFound)
            | Self::QuickShare(Q::Binary(cloudflared::Error::NotFound)) => {
                ErrorKind::CloudflaredMissing
            }
            Self::QuickShare(Q::NotFound)
            | Self::Runtime(S::NotFound(_))
            | Self::Accounts(A::NotFound) => ErrorKind::NotFound,
            Self::Accounts(A::InvalidToken | A::NoAccess | A::InvalidCert) => {
                ErrorKind::InvalidInput
            }
            Self::QuickShare(Q::NoFreePort) | Self::Runtime(S::AlreadyRunning(_)) => {
                ErrorKind::Unavailable
            }
            _ => ErrorKind::Internal,
        }
    }
}

/// Result alias for this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quick_share::QuickShareError;

    #[test]
    fn classifies_errors() {
        let missing = Error::from(QuickShareError::Binary(cloudflared::Error::NotFound));
        assert_eq!(missing.kind(), ErrorKind::CloudflaredMissing);
        assert_eq!(
            Error::from(QuickShareError::NotFound).kind(),
            ErrorKind::NotFound
        );
        assert_eq!(
            Error::from(QuickShareError::NoFreePort).kind(),
            ErrorKind::Unavailable
        );
        assert_eq!(
            Error::from(cloudflared::Error::Exited(Some(1))).kind(),
            ErrorKind::Internal
        );
    }
}
