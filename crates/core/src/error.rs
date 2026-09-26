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
    /// Analytics couldn't be read.
    #[error(transparent)]
    Analytics(#[from] crate::analytics::AnalyticsError),
    /// A Snapshot couldn't be prepared or changed.
    #[error(transparent)]
    Snapshot(crate::snapshot::SnapshotError),
    /// The inspector refused or failed.
    #[error(transparent)]
    Inspect(crate::inspect::InspectError),
    /// Comments couldn't be read or written.
    #[error(transparent)]
    Comments(crate::comments::CommentsError),
}

impl From<crate::comments::CommentsError> for Error {
    fn from(err: crate::comments::CommentsError) -> Self {
        match err {
            crate::comments::CommentsError::Store(err) => Self::Store(err),
            other => Self::Comments(other),
        }
    }
}

impl From<crate::inspect::InspectError> for Error {
    fn from(err: crate::inspect::InspectError) -> Self {
        match err {
            crate::inspect::InspectError::Engine(err) => Self::Engine(err),
            crate::inspect::InspectError::Store(err) => Self::Store(err),
            other => Self::Inspect(other),
        }
    }
}

impl From<crate::snapshot::SnapshotError> for Error {
    fn from(err: crate::snapshot::SnapshotError) -> Self {
        match err {
            crate::snapshot::SnapshotError::Engine(err) => Self::Engine(err),
            other => Self::Snapshot(other),
        }
    }
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
            Self::Accounts(A::InvalidToken | A::NoAccess | A::InvalidCert)
            | Self::QuickShare(Q::InvalidHostHeader) => ErrorKind::InvalidInput,
            Self::QuickShare(Q::NoFreePort) | Self::Runtime(S::AlreadyRunning(_)) => {
                ErrorKind::Unavailable
            }
            Self::Engine(e) => match e {
                E::Plan(
                    P::NoZone(_)
                    | P::RouteExists(_)
                    | P::AccessDomain(_)
                    | P::InvalidTunnelName
                    | P::TunnelNameTaken(_)
                    | P::SnapshotLoginNeedsDomain
                    | P::EdgeRateLimitPeriod { .. }
                    | P::InvalidTokenLabel
                    | P::ServiceTokenExists(_)
                    | P::FrontNeedsRoute(_)
                    | P::Front(_)
                    | P::InboxNeedsSecret,
                )
                | E::Input(_) => ErrorKind::InvalidInput,
                E::Plan(
                    P::ZeroTrustNotSetUp
                    | P::NoWorkersSubdomain
                    | P::EdgeRateLimitNeedsPro(_)
                    | P::EdgeQuotaFull { .. }
                    | P::ServiceTokenLimit(_),
                ) => ErrorKind::Unavailable,
                E::Plan(
                    P::NoSuchSnapshot(_)
                    | P::NoSuchRoute(_)
                    | P::NoTunnel
                    | P::NoSuchRecord(_)
                    | P::NoSuchLogin(_)
                    | P::NoSuchNetwork(_)
                    | P::NotBalanced(_)
                    | P::NotReserved(_)
                    | P::NoSuchServiceToken(_)
                    | P::NoFront(_),
                )
                | E::Observe(O::UnknownTunnel) => ErrorKind::NotFound,
                E::Stale(_)
                | E::Adopt(_)
                | E::NeedsConfirmation
                | E::NothingToRestore
                | E::Plan(
                    P::AccessAppExists(_)
                    | P::NetworkRouted { .. }
                    | P::RoutedElsewhere { .. }
                    | P::BalancerExists(_)
                    | P::SnapshotExists(_)
                    | P::HostnameRouted(_)
                    | P::HostnameServed { .. }
                    | P::HostnameInUse(_)
                    | P::EdgeRateLimitConflict { .. }
                    | P::ServiceTokenNotOwned(_)
                    | P::WorkerRouteTaken { .. },
                ) => ErrorKind::Conflict,
                E::Observe(O::Api(api)) if api.is_auth() => ErrorKind::PermissionDenied,
                E::Observe(
                    O::AccessPermission
                    | O::EdgePermission
                    | O::CacheRulesPermission
                    | O::ServiceTokenPermission
                    | O::WorkersPermission,
                ) => ErrorKind::PermissionDenied,
                E::Observe(O::Api(api)) if api.status().is_none() => ErrorKind::Unavailable,
                E::Observe(_) => ErrorKind::Internal,
            },
            Self::Snapshot(e) => {
                use crate::snapshot::SnapshotError as N;
                match e {
                    N::NotFound | N::NoSuchVersion(_) | N::NotPrepared => ErrorKind::NotFound,
                    N::Unchanged | N::NameTaken(_) => ErrorKind::Conflict,
                    N::Io { .. } | N::Random | N::Crawl(_) | N::Engine(_) => ErrorKind::Internal,
                    _ => ErrorKind::InvalidInput,
                }
            }
            Self::Inspect(err) => {
                use crate::inspect::InspectError as I;
                match err {
                    I::UnknownTap | I::UnknownExchange | I::NotRoute(_) | I::NotInspected(_) => {
                        ErrorKind::NotFound
                    }
                    I::NotWeb
                    | I::Invalid(_)
                    | I::Lens(
                        lens::LensError::InvalidConfig(_) | lens::LensError::BodyTruncated { .. },
                    ) => ErrorKind::InvalidInput,
                    I::TapGone => ErrorKind::Conflict,
                    _ => ErrorKind::Internal,
                }
            }
            Self::CloudApi(api) if api.is_auth() => ErrorKind::PermissionDenied,
            Self::Comments(err) => {
                use crate::comments::CommentsError as C;
                match err {
                    C::InvalidBody | C::InvalidName | C::InvalidPath | C::InvalidAnchor => {
                        ErrorKind::InvalidInput
                    }
                    C::NotFound => ErrorKind::NotFound,
                    C::TooMany | C::RateLimited => ErrorKind::Unavailable,
                    C::NoDatabase | C::Disabled | C::NoAccount => ErrorKind::Conflict,
                    C::Api(api) if api.is_auth() => ErrorKind::PermissionDenied,
                    C::Api(api) if api.status().is_none() => ErrorKind::Unavailable,
                    C::Api(_) | C::Store(_) => ErrorKind::Internal,
                }
            }
            Self::Analytics(err) => {
                use crate::analytics::AnalyticsError as An;
                match err {
                    An::Permission => ErrorKind::PermissionDenied,
                    An::RateLimited | An::NotOnPlan => ErrorKind::Unavailable,
                    An::NoZone(_) | An::Account(A::NotFound) => ErrorKind::NotFound,
                    An::Api(api) if api.is_auth() => ErrorKind::PermissionDenied,
                    An::Api(api) if api.status().is_none() => ErrorKind::Unavailable,
                    _ => ErrorKind::Internal,
                }
            }
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
            Self::QuickShare(crate::quick_share::QuickShareError::InvalidHostHeader) => {
                Some("hostHeader")
            }
            Self::Engine(E::Plan(
                P::NoZone(_)
                | P::RouteExists(_)
                | P::RoutedElsewhere { .. }
                | P::HostnameRouted(_)
                | P::HostnameServed { .. },
            ))
            | Self::Snapshot(crate::snapshot::SnapshotError::InvalidHostname(_)) => {
                Some("hostname")
            }
            Self::Engine(E::Plan(P::InvalidTunnelName | P::TunnelNameTaken(_))) => {
                Some("tunnelName")
            }
            Self::Engine(E::Plan(P::AccessDomain(_))) => Some("path"),
            Self::Engine(E::Plan(P::NetworkRouted { .. })) => Some("network"),
            Self::Accounts(_) => Some("credential"),
            Self::Snapshot(
                crate::snapshot::SnapshotError::InvalidName
                | crate::snapshot::SnapshotError::NameTaken(_),
            ) => Some("name"),
            Self::Snapshot(crate::snapshot::SnapshotError::PasswordTooShort(_)) => Some("password"),
            _ => None,
        }
    }
}

impl crate::text::UserText for Error {
    fn text(&self) -> crate::text::Text {
        match self {
            Self::CloudApi(err) => err.text(),
            Self::Cloudflared(err) => err.text(),
            Self::Accounts(err) => err.text(),
            Self::QuickShare(err) => err.text(),
            Self::Runtime(err) => err.text(),
            Self::Store(err) => err.text(),
            Self::Engine(err) => err.text(),
            Self::Analytics(err) => err.text(),
            Self::Snapshot(err) => err.text(),
            Self::Inspect(err) => err.text(),
            Self::Comments(err) => err.text(),
        }
    }
}

impl crate::text::UserText for cf_api::Error {
    fn text(&self) -> crate::text::Text {
        use crate::text::msg::error::cloudflare as m;
        match self {
            Self::Api { .. } if self.is_auth() => m::permission(self.detail()),
            Self::Api { .. } => m::api(self.detail()),
            Self::Decode(_) => m::decode(),
            Self::Network(detail) => m::network(detail),
        }
    }
}

impl crate::text::UserText for cloudflared::Error {
    fn text(&self) -> crate::text::Text {
        use crate::text::msg::error::cloudflared as m;
        match self {
            Self::InvalidVersion(version) => m::invalid_version(version),
            Self::NotFound => m::not_found(),
            Self::Io(err) => m::io(err),
            Self::Exited(Some(code)) => m::exited(code),
            Self::Exited(None) => m::exited_signal(),
            Self::Http(detail) => m::http(detail),
            Self::Verification(detail) => m::verification(detail),
            Self::UnsupportedPlatform => m::unsupported_platform(),
            Self::Timeout(operation) => m::timeout(operation),
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
