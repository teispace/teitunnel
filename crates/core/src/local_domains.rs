//! Local HTTPS domains (M12-07, decision Q7): `https://shop.test`, `https://app.localhost`
//! and `https://phone.local` for services on this computer, with certificates browsers
//! trust.
//!
//! `crates/localdomains` does the certificates, trust installers and name resolution
//! pieces; this module is the product around them:
//!
//! - [`registry`]: the domains in the database (migration 17) and their setting.
//! - [`keys`]: the CA's key in the OS keychain (the `CaKeyStore` port over
//!   [`crate::secrets`]); never in the database, a backup or IPC.
//! - [`LocalDomains`]: one HTTPS listener (443, or 8443 when that can't be used) and one
//!   plain listener (80 → HTTPS redirects, and HTTP-only domains) in the process's Lens,
//!   through its [`crate::inspect::Inspector`], so a domain's requests can be inspected
//!   like a share's. Certificates are issued per SNI name and renewed; `.test` names get
//!   a loopback name server, `.local` names are advertised for phones.
//! - [`trust`]: installing the CA (user level; administrator steps returned as data).
//! - [`doctor`]: checks with fixes.
//!
//! Local domains never touch Cloudflare.

mod acceptor;
pub mod doctor;
pub mod keys;
pub mod model;
pub mod registry;
mod service;
pub mod trust;

#[cfg(test)]
mod tests;

use localdomains::{
    CaError, KeyStoreError, NameError, RegistryError, resolver_config::ResolverConfigError,
};

pub use doctor::{LocalDomainFix, LocalFacts, diagnose};
pub use localdomains;
pub use model::{
    CaView, LocalDomainInput, LocalDomainRow, LocalDomainSettings, LocalDomainView,
    LocalDomainsStatus, LocalTarget, NameResolution, PlatformKind, PortProblem, PortReason,
    PrivilegedStep, ResolverView, TrustOptions, TrustState, TrustStoreKind, TrustStoreView,
    TrustView, origin_of, parse_target,
};
pub use service::{AdminTask, LocalDomains, LocalDomainsConfig, LocalDomainsOptions};

use crate::{
    inspect::InspectError,
    store::StoreError,
    text::{Text, UserText, english_display, msg::local_domains as m},
};

/// Why a local domains operation failed.
#[derive(Debug, thiserror::Error)]
pub enum LocalDomainError {
    /// The name isn't a local name.
    Name(#[from] NameError),
    /// Another local domain has this name.
    Duplicate(String),
    /// No local domain has this name.
    NotFound(String),
    /// The target isn't a port, `host:port` or URL.
    BadTarget(String),
    /// The target was refused.
    InvalidTarget(&'static str),
    /// The certificate authority couldn't be made or read.
    Ca(String),
    /// The keychain refused.
    Keychain(String),
    /// Neither port could be used.
    NoPort {
        /// The port wanted.
        port: u16,
        /// The fallback.
        fallback: u16,
    },
    /// Only on Linux.
    NotSupported,
    /// `pkexec` isn't installed.
    NoPkexec,
    /// An administrator step failed.
    Privileged(String),
    /// The inspector (Lens) refused.
    Inspect(#[from] InspectError),
    /// The database failed.
    Store(#[from] StoreError),
    /// A file couldn't be written.
    Io(#[from] std::io::Error),
}

impl From<RegistryError> for LocalDomainError {
    fn from(err: RegistryError) -> Self {
        match err {
            RegistryError::Name(err) => Self::Name(err),
            RegistryError::Duplicate(name) => Self::Duplicate(name.to_string()),
            RegistryError::InvalidTarget(why) => Self::InvalidTarget(why),
            // Every suffix is enabled here.
            RegistryError::SuffixNotEnabled(suffix) => Self::Name(NameError::SuffixNotAllowed {
                allowed: format!(".{suffix}"),
            }),
        }
    }
}

impl From<CaError> for LocalDomainError {
    fn from(err: CaError) -> Self {
        match err {
            CaError::Store(KeyStoreError::Keychain(detail)) => Self::Keychain(detail),
            other => Self::Ca(other.to_string()),
        }
    }
}

impl From<KeyStoreError> for LocalDomainError {
    fn from(err: KeyStoreError) -> Self {
        match err {
            KeyStoreError::Keychain(detail) => Self::Keychain(detail),
            KeyStoreError::Io(err) => Self::Io(err),
        }
    }
}

impl From<ResolverConfigError> for LocalDomainError {
    fn from(err: ResolverConfigError) -> Self {
        Self::Privileged(err.to_string())
    }
}

impl UserText for LocalDomainError {
    fn text(&self) -> Text {
        use m::error as e;
        match self {
            Self::Name(err) => match err {
                NameError::Empty => e::name_empty(),
                NameError::TooLong => e::name_too_long(),
                NameError::InvalidLabel(label) => e::name_label(label),
                NameError::Wildcard => e::name_wildcard(),
                NameError::SuffixNotAllowed { .. } => e::name_suffix(),
                NameError::BareSuffix(suffix) => e::name_bare(suffix),
            },
            Self::Duplicate(name) => e::duplicate(name),
            Self::NotFound(name) => e::not_found(name),
            Self::BadTarget(value) => e::bad_target(value),
            Self::InvalidTarget(detail) => e::invalid_target(detail),
            Self::Ca(detail) => e::ca(detail),
            Self::Keychain(detail) => e::keychain(detail),
            Self::NoPort { port, fallback } => e::no_port(port, fallback),
            Self::NotSupported => e::not_supported(),
            Self::NoPkexec => e::no_pkexec(),
            Self::Privileged(detail) => e::privileged(detail),
            Self::Inspect(err) => err.text(),
            Self::Store(err) => err.text(),
            Self::Io(err) => e::io(err),
        }
    }
}

english_display!(LocalDomainError);
