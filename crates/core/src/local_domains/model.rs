//! What local domains look like to the app, the CLI and the control connection: plain
//! serde types (mirrored from `localdomains` where they cross IPC), and parsing what a
//! person types as a target.

use localdomains::{
    DomainTarget, LocalName, Platform, StoreState, StoreStatus, TrustStore,
    trust::{linux::LinuxFlavor, nss::NssApp},
};
use serde::{Deserialize, Serialize};

use crate::{inspect::lens::TapId, text::Text};

use super::LocalDomainError;

/// Where a local domain sends requests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LocalTarget {
    /// A port on this computer (`http://localhost:<port>`).
    Port {
        /// The port.
        port: u16,
    },
    /// An HTTP(S) service by URL.
    Url {
        /// E.g. `https://127.0.0.1:5173`.
        url: String,
    },
    /// A Quick Share, by id (not served yet; kept so files from newer versions load).
    Share {
        /// The share's id.
        id: String,
    },
    /// A route, by id (not served yet; kept so files from newer versions load).
    Route {
        /// The route's id.
        id: String,
    },
}

impl From<&DomainTarget> for LocalTarget {
    fn from(target: &DomainTarget) -> Self {
        match target {
            DomainTarget::Port { port } => Self::Port { port: *port },
            DomainTarget::Url { url } => Self::Url { url: url.clone() },
            DomainTarget::Share { id } => Self::Share { id: id.clone() },
            DomainTarget::Route { id } => Self::Route { id: id.clone() },
        }
    }
}

impl From<&LocalTarget> for DomainTarget {
    fn from(target: &LocalTarget) -> Self {
        match target {
            LocalTarget::Port { port } => Self::Port { port: *port },
            LocalTarget::Url { url } => Self::Url { url: url.clone() },
            LocalTarget::Share { id } => Self::Share { id: id.clone() },
            LocalTarget::Route { id } => Self::Route { id: id.clone() },
        }
    }
}

/// The local service a target reaches, as a URL (`None` for targets not served yet).
pub fn origin_of(target: &DomainTarget) -> Option<String> {
    match target {
        DomainTarget::Port { port } => Some(format!("http://localhost:{port}")),
        DomainTarget::Url { url } => Some(url.trim_end_matches('/').to_owned()),
        DomainTarget::Share { .. } | DomainTarget::Route { .. } => None,
    }
}

/// Reads what a person typed as a target: a port (`3000`), `host:port`, or a URL. Plain
/// HTTP on `localhost`/`127.0.0.1` becomes a port.
///
/// # Errors
/// [`LocalDomainError::BadTarget`].
pub fn parse_target(input: &str) -> Result<DomainTarget, LocalDomainError> {
    let input = input.trim().trim_end_matches('/');
    let bad = || LocalDomainError::BadTarget(input.to_owned());
    if input.is_empty() {
        return Err(bad());
    }
    if input.bytes().all(|b| b.is_ascii_digit()) {
        return match input.parse::<u16>() {
            Ok(port) if port > 0 => Ok(DomainTarget::Port { port }),
            _ => Err(bad()),
        };
    }
    let url = if input.contains("://") {
        input.to_owned()
    } else {
        format!("http://{input}")
    };
    let origin = crate::inspect::lens::OriginUrl::parse(&url).map_err(|_| bad())?;
    let loopback = matches!(origin.host(), "localhost" | "127.0.0.1");
    if !origin.is_https() && loopback {
        return Ok(DomainTarget::Port {
            port: origin.port(),
        });
    }
    Ok(DomainTarget::Url {
        url: origin.to_string(),
    })
}

/// A local domain to add (or the new settings of one).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LocalDomainInput {
    /// E.g. `shop.test`, `app.localhost`, `phone.local`.
    pub name: String,
    /// A port, `host:port` or URL.
    pub target: String,
    /// `*.name` goes to the same service.
    pub wildcard: bool,
    /// Serve it over HTTPS (plain HTTP otherwise).
    pub https: bool,
    /// Record its requests in the inspector.
    pub inspect: bool,
}

/// A local domain as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDomainRow {
    /// Its name.
    pub name: LocalName,
    /// Where it sends requests.
    pub target: DomainTarget,
    /// Subdomains too.
    pub wildcard: bool,
    /// Over HTTPS.
    pub https: bool,
    /// Requests are recorded.
    pub inspect: bool,
    /// The project file that declared it.
    pub project: Option<String>,
    /// When it was added, Unix seconds.
    pub created_at: i64,
}

/// Does the name reach this computer?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum NameResolution {
    /// The system resolves it to this computer.
    Ok,
    /// `.test` names need the resolver entry (browsers can't reach it yet).
    NeedsResolver,
    /// It resolves somewhere else.
    Elsewhere,
    /// Not checked (`.local` names resolve through multicast DNS on the network).
    Unchecked,
}

/// One local domain, with how it's doing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LocalDomainView {
    /// E.g. `shop.test`.
    pub name: String,
    /// Where to open it, e.g. `https://shop.test` (with the port when it isn't 443/80).
    pub url: String,
    /// The local service, e.g. `http://localhost:3000` (`None` for targets not served).
    pub origin: Option<String>,
    /// The target as stored.
    pub target: LocalTarget,
    /// Subdomains go to the same service.
    pub wildcard: bool,
    /// Served over HTTPS.
    pub https: bool,
    /// Requests are recorded in the inspector.
    pub inspect: bool,
    /// The project file that declared it.
    pub project: Option<String>,
    /// When it was added, Unix seconds.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: i64,
    /// Teitunnel is answering for it now.
    pub serving: bool,
    /// Whether the name reaches this computer.
    pub resolution: NameResolution,
    /// Its inspector tap while served.
    pub tap_id: Option<TapId>,
    /// Requests served since it started.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub requests: u64,
}

/// Why the preferred port couldn't be used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum PortReason {
    /// Another program listens on it.
    InUse,
    /// The system reserves it for administrators.
    PermissionDenied,
    /// Something else.
    Other,
}

/// A port Teitunnel wanted but didn't get.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PortProblem {
    /// The port wanted (443 or 80).
    pub port: u16,
    /// Why it wasn't available.
    pub reason: PortReason,
    /// The port used instead (`None`: none could be used).
    pub fallback: Option<u16>,
}

/// A step that needs administrator rights, for the person to run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PrivilegedStep {
    /// The command to paste into a terminal (an administrator one on Windows).
    pub command: String,
}

/// `.test` names: the responder and the system's resolver entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ResolverView {
    /// Some local domain ends in `.test`.
    pub needed: bool,
    /// Teitunnel's name server answers (on `127.0.0.1:<port>`).
    pub responding: bool,
    /// Its port.
    pub port: u16,
    /// The system sends `.test` names to it (checked by resolving one).
    pub configured: bool,
    /// Why the name server couldn't start.
    pub error: Option<Text>,
    /// The one-time steps that add the resolver entry.
    pub setup: Vec<PrivilegedStep>,
    /// The steps that remove it.
    pub teardown: Vec<PrivilegedStep>,
}

/// The local certificate authority (public facts only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CaView {
    /// `Teitunnel Local CA (user@host)`.
    pub common_name: String,
    /// SHA-256 of the certificate.
    pub sha256: String,
    /// When it expires, Unix seconds.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub not_after: i64,
}

/// Everything about local domains at a glance (no processes run, no prompts).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LocalDomainsStatus {
    /// The listeners are up.
    pub running: bool,
    /// The HTTPS port in use.
    pub https_port: Option<u16>,
    /// The plain HTTP port in use.
    pub http_port: Option<u16>,
    /// Why 443 or 80 isn't used.
    pub port_problems: Vec<PortProblem>,
    /// Phones and other computers on the network may connect (`.local` names only over
    /// HTTPS).
    pub lan: bool,
    /// This computer's addresses on the network, for phones.
    pub lan_addresses: Vec<String>,
    /// `.test` names.
    pub resolver: ResolverView,
    /// The certificate authority, once created.
    pub ca: Option<CaView>,
    /// The domains.
    pub domains: Vec<LocalDomainView>,
    /// Why local domains aren't served, if they should be.
    pub error: Option<Text>,
    /// Which platform's steps these are.
    pub platform: PlatformKind,
}

/// The platform, for the trust walkthrough.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum PlatformKind {
    /// macOS.
    Macos,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

impl From<Platform> for PlatformKind {
    fn from(platform: Platform) -> Self {
        match platform {
            Platform::Macos => Self::Macos,
            Platform::Windows => Self::Windows,
            Platform::Linux => Self::Linux,
        }
    }
}

/// A trust store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum TrustStoreKind {
    /// The macOS login keychain (Safari, Chrome, Edge, and Firefox by default).
    MacosKeychain,
    /// Windows' certificates for the current user.
    WindowsUser,
    /// The Linux system store (curl, Node, most tools).
    LinuxSystem,
    /// Chrome or Chromium's certificate database.
    Chrome,
    /// A Firefox profile's certificate database.
    Firefox,
    /// A Firefox profile's "trust the system's certificates" setting.
    FirefoxSystemRoots,
}

/// A trust store's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum TrustState {
    /// Trusted.
    Trusted,
    /// Present but not trusted.
    NotTrusted,
    /// Not there.
    Absent,
    /// Firefox follows the system's trust.
    FollowsSystem,
    /// Firefox doesn't follow the system's trust.
    Disabled,
    /// No supported store here.
    Unsupported,
    /// A tool is missing.
    ToolMissing {
        /// The program.
        tool: String,
        /// The package to install.
        package: Option<String>,
    },
    /// Checking or changing it failed.
    Error {
        /// Details (technical).
        message: String,
    },
}

/// One store's trust.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TrustStoreView {
    /// Which store.
    pub kind: TrustStoreKind,
    /// Its folder, for databases and profiles.
    pub path: Option<String>,
    /// The Linux distribution family, for the system store.
    pub flavor: Option<String>,
    /// Its state.
    pub state: TrustState,
}

impl From<&StoreStatus> for TrustStoreView {
    fn from(status: &StoreStatus) -> Self {
        let (kind, path, flavor) = match &status.store {
            TrustStore::MacosLoginKeychain => (TrustStoreKind::MacosKeychain, None, None),
            TrustStore::WindowsCurrentUserRoot => (TrustStoreKind::WindowsUser, None, None),
            TrustStore::LinuxSystem { flavor } => (
                TrustStoreKind::LinuxSystem,
                None,
                flavor.map(|f| flavor_name(f).to_owned()),
            ),
            TrustStore::Nss { app, path } => (
                match app {
                    NssApp::Chromium => TrustStoreKind::Chrome,
                    NssApp::Firefox => TrustStoreKind::Firefox,
                },
                Some(path.display().to_string()),
                None,
            ),
            TrustStore::FirefoxEnterpriseRoots { profile } => (
                TrustStoreKind::FirefoxSystemRoots,
                Some(profile.display().to_string()),
                None,
            ),
        };
        let state = match &status.state {
            StoreState::Trusted => TrustState::Trusted,
            StoreState::PresentNotTrusted => TrustState::NotTrusted,
            StoreState::Absent => TrustState::Absent,
            StoreState::FollowsSystem => TrustState::FollowsSystem,
            StoreState::Disabled => TrustState::Disabled,
            StoreState::Unsupported => TrustState::Unsupported,
            StoreState::ToolMissing { tool, package } => TrustState::ToolMissing {
                tool: tool.clone(),
                package: package.clone(),
            },
            StoreState::Error { message } => TrustState::Error {
                message: message.clone(),
            },
        };
        Self {
            kind,
            path,
            flavor,
            state,
        }
    }
}

fn flavor_name(flavor: LinuxFlavor) -> &'static str {
    match flavor {
        LinuxFlavor::Debian => "debian",
        LinuxFlavor::Fedora => "fedora",
        LinuxFlavor::Suse => "suse",
        LinuxFlavor::P11Kit => "p11-kit",
    }
}

/// Whether this computer trusts the local certificate authority, store by store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TrustView {
    /// The system's own store trusts it (what most browsers use).
    pub trusted: bool,
    /// Every store found.
    pub stores: Vec<TrustStoreView>,
    /// Steps left that need administrator rights (Linux's system store).
    pub steps: Vec<PrivilegedStep>,
    /// The certificate authority (created by the first trust).
    pub ca: Option<CaView>,
    /// The platform.
    pub platform: PlatformKind,
}

/// What to trust beyond the system store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct TrustOptions {
    /// Also add it to Chrome's and Firefox's own certificate databases (always on Linux).
    pub browsers: bool,
    /// Turn on "trust the system's certificates" in Firefox profiles (macOS, Windows).
    pub firefox_system_roots: bool,
}

/// Preferences for local domains (the `localDomains` setting).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", default)]
pub struct LocalDomainSettings {
    /// Phones and computers on the network may open `.local` names.
    pub lan: bool,
}

#[allow(clippy::derivable_impls)]
impl Default for LocalDomainSettings {
    fn default() -> Self {
        Self { lan: false }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_what_people_type_as_a_target() {
        assert_eq!(
            parse_target("3000").unwrap(),
            DomainTarget::Port { port: 3000 }
        );
        assert_eq!(
            parse_target(" localhost:5173/ ").unwrap(),
            DomainTarget::Port { port: 5173 }
        );
        assert_eq!(
            parse_target("http://127.0.0.1:8080").unwrap(),
            DomainTarget::Port { port: 8080 }
        );
        assert_eq!(
            parse_target("https://localhost:5173").unwrap(),
            DomainTarget::Url {
                url: "https://localhost:5173".into()
            }
        );
        assert_eq!(
            parse_target("192.168.1.20:3000").unwrap(),
            DomainTarget::Url {
                url: "http://192.168.1.20:3000".into()
            }
        );
        for bad in ["", "0", "70000", "ftp://x", "http://a/path", "a b"] {
            assert!(parse_target(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn targets_round_trip_and_name_their_origin() {
        let target = DomainTarget::Url {
            url: "https://localhost:5173/".into(),
        };
        assert_eq!(DomainTarget::from(&LocalTarget::from(&target)), target);
        assert_eq!(
            origin_of(&target).as_deref(),
            Some("https://localhost:5173")
        );
        assert_eq!(
            origin_of(&DomainTarget::Port { port: 3000 }).as_deref(),
            Some("http://localhost:3000")
        );
        assert_eq!(origin_of(&DomainTarget::Share { id: "s".into() }), None);
    }
}
