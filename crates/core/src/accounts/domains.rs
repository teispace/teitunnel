//! Domains (Cloudflare zones) as the UI shows them.

use serde::Serialize;

/// Where a domain is in its setup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum DomainStatus {
    /// Active on Cloudflare: routes can use it.
    Active,
    /// Waiting for the registrar to switch nameservers.
    Pending,
    /// Nameservers moved away from Cloudflare.
    Moved,
    /// Being set up, or a state we don't know.
    Other,
}

/// A domain in a connected account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Domain {
    /// Zone id.
    pub id: String,
    /// Domain name.
    pub name: String,
    /// Setup state.
    pub status: DomainStatus,
    /// Cloudflare nameservers to set at the registrar.
    pub name_servers: Vec<String>,
    /// Nameservers currently at the registrar (before the switch).
    pub original_name_servers: Vec<String>,
    /// Plan name.
    pub plan: Option<String>,
    /// Whether the proxy is paused.
    pub paused: bool,
}

impl From<cf_api::Zone> for Domain {
    fn from(zone: cf_api::Zone) -> Self {
        use cf_api::ZoneStatus as Z;
        Self {
            status: match zone.status {
                Z::Active => DomainStatus::Active,
                Z::Pending => DomainStatus::Pending,
                Z::Moved => DomainStatus::Moved,
                Z::Initializing | Z::Unknown => DomainStatus::Other,
            },
            id: zone.id,
            name: zone.name,
            name_servers: zone.name_servers,
            original_name_servers: zone.original_name_servers.unwrap_or_default(),
            plan: zone.plan.map(|p| p.name).filter(|n| !n.is_empty()),
            paused: zone.paused,
        }
    }
}
