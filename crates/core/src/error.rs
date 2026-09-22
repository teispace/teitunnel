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
    /// A routes change failed before anything was applied.
    #[error(transparent)]
    Engine(#[from] crate::engine::EngineError),
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
    /// The state changed since the user looked, or they must confirm first; the UI
    /// refreshes the preview.
    Conflict,
    /// Cloudflare refused: the credential lacks a permission.
    PermissionDenied,
    /// Anything else: a bug or an environment problem, logged with details.
    Internal,
}

impl Error {
    /// Classifies the error.
    pub fn kind(&self) -> ErrorKind {
        use crate::{
            accounts::AccountError as A,
            engine::{EngineError as E, ObserveError as O, PlanError as P},
            quick_share::QuickShareError as Q,
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
            Self::Engine(e) => match e {
                E::Plan(P::NoZone(_) | P::RouteExists(_)) | E::Input(_) => ErrorKind::InvalidInput,
                E::Plan(P::NoSuchRoute(_) | P::NoTunnel) => ErrorKind::NotFound,
                E::Stale(_) | E::NeedsConfirmation | E::NothingToRestore => ErrorKind::Conflict,
                E::Observe(O::Api(api)) if api.is_auth() => ErrorKind::PermissionDenied,
                E::Observe(O::Api(api)) if api.status().is_none() => ErrorKind::Unavailable,
                E::Observe(_) => ErrorKind::Internal,
            },
            Self::CloudApi(api) if api.is_auth() => ErrorKind::PermissionDenied,
            _ => ErrorKind::Internal,
        }
    }
}

impl Error {
    /// The input field an [`ErrorKind::InvalidInput`] error is about.
    pub fn field(&self) -> Option<&'static str> {
        use crate::engine::{EngineError as E, PlanError as P};
        match self {
            Self::Engine(E::Input(input)) => Some(input.field),
            Self::Engine(E::Plan(P::NoZone(_) | P::RouteExists(_))) => Some("hostname"),
            Self::Accounts(_) => Some("credential"),
            _ => None,
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
