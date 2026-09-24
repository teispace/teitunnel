//! Local HTTPS domains: `https://app.localhost` with a real, trusted certificate.
//!
//! - [`ca`]: Teitunnel's own root CA, name-constrained to `.localhost`, `.test`, `.local`
//!   and loopback/private IPs, with its key in the OS keychain ([`keystore`]).
//! - [`leaf`] and [`sni`]: short-lived leaf certificates issued on demand for the SNI name
//!   of each TLS handshake, as a `rustls` certificate resolver and server config for Lens.
//! - [`trust`]: installing the CA into the OS and browser trust stores, per user where the
//!   OS allows it, with any privileged step returned as data ([`privileged`]).
//! - [`dns`], [`resolver_config`], [`resolve_check`], [`mdns`]: making the names resolve
//!   (`.localhost` needs nothing in browsers; `.test` uses a loopback DNS responder plus a
//!   per-OS resolver entry; `.local` is advertised over mDNS for phones).
//! - [`mobile`]: the CA as an Apple configuration profile and a plain certificate for
//!   phones on the LAN.
//! - [`ports`]: whether 443/80 can be bound, and the Linux fixes when they can't.
//! - [`registry`]: the model of local domains (plain serde types) that `core` persists and
//!   Lens routes by.
//!
//! No Tauri, no SQLite, no HTTP proxying: Lens (`crates/lens`) does the proxying and uses
//! [`sni::SniResolver`] / [`sni::server_config`] for TLS and [`registry::DomainRegistry`] for
//! host-based routing.

pub mod ca;
pub mod clock;
pub mod dns;
mod fsutil;
pub mod keystore;
pub mod leaf;
pub mod mdns;
pub mod mobile;
pub mod name;
pub mod platform;
pub mod ports;
pub mod privileged;
pub mod process;
pub mod registry;
pub mod resolve_check;
pub mod resolver_config;
pub mod sni;
pub mod trust;

pub use ca::{CaError, CaIdentity, LocalCa, crypto_provider};
pub use clock::{Clock, ManualClock, SystemClock};
pub use dns::{DnsError, DnsResponder, DnsZone};
pub use keystore::{
    CA_KEYCHAIN_ACCOUNT, CaKeyStore, CaSecret, FileKeyStore, KeyStoreError, MemoryKeyStore,
};
pub use leaf::{CertRequest, LeafCache, LeafError};
pub use name::{LocalName, NameError, Suffix};
pub use platform::Platform;
pub use privileged::PrivilegedAction;
pub use process::{Invocation, Runner, SystemRunner};
pub use registry::{DomainRegistry, DomainTarget, HostMatch, LocalDomain, RegistryError};
pub use sni::{CertPolicy, SniResolver, SuffixPolicy, server_config};
pub use trust::{
    CaCert, InstallOptions, StoreState, StoreStatus, TrustEnv, TrustManager, TrustReport,
    TrustStore,
};
