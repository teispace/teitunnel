//! The model of local domains: plain serde types plus validation.
//!
//! `core` persists [`LocalDomain`]s (this crate has no database) and hands a
//! [`DomainRegistry`] to Lens, which routes requests by `Host` with [`DomainRegistry::lookup`]
//! and issues certificates through the registry's [`crate::sni::CertPolicy`] implementation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
    leaf::CertRequest,
    name::{LocalName, NameError, Suffix},
};

/// Longest accepted share/route id.
const MAX_ID_LEN: usize = 128;

/// What a local domain points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DomainTarget {
    /// A quick share, by id.
    Share {
        /// The share's id.
        id: String,
    },
    /// A route, by id.
    Route {
        /// The route's id.
        id: String,
    },
    /// A local port on loopback.
    Port {
        /// The port (1–65535).
        port: u16,
    },
    /// An HTTP(S) service by URL (`http://127.0.0.1:8080`, `https://localhost:5173`), for
    /// services that aren't on loopback or speak TLS themselves. No path or query.
    Url {
        /// The URL.
        url: String,
    },
}

/// Longest accepted target URL.
const MAX_URL_LEN: usize = 2048;

fn valid_target_url(url: &str) -> Result<(), &'static str> {
    let rest = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
        .ok_or("the URL must start with http:// or https://")?;
    if url.len() > MAX_URL_LEN {
        return Err("URL too long");
    }
    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("the URL has spaces or control characters");
    }
    let authority = rest.strip_suffix('/').unwrap_or(rest);
    if authority.is_empty() {
        return Err("the URL has no host");
    }
    if authority.contains(['/', '?', '#', '@']) {
        return Err("the URL can't have a path, query or credentials");
    }
    Ok(())
}

/// One local domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDomain {
    /// The name, e.g. `myapp.localhost`.
    pub name: LocalName,
    /// Where requests for it go.
    pub target: DomainTarget,
    /// Whether one level of subdomains (`*.myapp.localhost`) goes to the same target.
    pub wildcard: bool,
    /// When it was created, Unix seconds.
    pub created_at: i64,
}

/// Why a domain was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RegistryError {
    /// The name is invalid.
    #[error(transparent)]
    Name(#[from] NameError),
    /// The suffix isn't enabled (e.g. `.test` without the DNS responder).
    #[error(".{0} names aren't enabled")]
    SuffixNotEnabled(Suffix),
    /// Another domain already has this name.
    #[error("{0} is already in use")]
    Duplicate(LocalName),
    /// The target is invalid (empty or overlong id, port 0).
    #[error("invalid target: {0}")]
    InvalidTarget(&'static str),
}

/// How a host matched a domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostMatch<'a> {
    /// The matching domain.
    pub domain: &'a LocalDomain,
    /// True if the host is a subdomain served through the domain's wildcard.
    pub via_wildcard: bool,
}

/// The set of local domains, validated as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainRegistry {
    enabled: Vec<Suffix>,
    domains: BTreeMap<LocalName, LocalDomain>,
}

impl DomainRegistry {
    /// An empty registry accepting the `enabled` suffixes.
    #[must_use]
    pub fn new(enabled: &[Suffix]) -> Self {
        Self {
            enabled: enabled.to_vec(),
            domains: BTreeMap::new(),
        }
    }

    /// A registry from persisted domains.
    ///
    /// # Errors
    /// The first domain that fails validation.
    pub fn from_domains(
        enabled: &[Suffix],
        domains: impl IntoIterator<Item = LocalDomain>,
    ) -> Result<Self, RegistryError> {
        let mut registry = Self::new(enabled);
        for domain in domains {
            registry.add(domain)?;
        }
        Ok(registry)
    }

    /// The enabled suffixes.
    #[must_use]
    pub fn enabled(&self) -> &[Suffix] {
        &self.enabled
    }

    /// Checks a domain against the rules and the existing domains without adding it.
    ///
    /// A more specific name under another domain's wildcard is allowed and takes
    /// precedence (`api.app.localhost` next to `app.localhost` with wildcard on).
    ///
    /// # Errors
    /// See [`RegistryError`].
    pub fn validate(&self, domain: &LocalDomain) -> Result<(), RegistryError> {
        let suffix = domain.name.suffix();
        if !self.enabled.contains(&suffix) {
            return Err(RegistryError::SuffixNotEnabled(suffix));
        }
        match &domain.target {
            DomainTarget::Share { id } | DomainTarget::Route { id } => {
                if id.trim().is_empty() {
                    return Err(RegistryError::InvalidTarget("empty id"));
                }
                if id.len() > MAX_ID_LEN {
                    return Err(RegistryError::InvalidTarget("id too long"));
                }
            }
            DomainTarget::Port { port } => {
                if *port == 0 {
                    return Err(RegistryError::InvalidTarget("port 0"));
                }
            }
            DomainTarget::Url { url } => {
                valid_target_url(url).map_err(RegistryError::InvalidTarget)?;
            }
        }
        if self.domains.contains_key(&domain.name) {
            return Err(RegistryError::Duplicate(domain.name.clone()));
        }
        Ok(())
    }

    /// Adds a domain.
    ///
    /// # Errors
    /// See [`DomainRegistry::validate`].
    pub fn add(&mut self, domain: LocalDomain) -> Result<(), RegistryError> {
        self.validate(&domain)?;
        self.domains.insert(domain.name.clone(), domain);
        Ok(())
    }

    /// Replaces an existing domain's target and wildcard setting (same name).
    ///
    /// # Errors
    /// The new target is invalid.
    pub fn update(&mut self, domain: LocalDomain) -> Result<Option<LocalDomain>, RegistryError> {
        let previous = self.domains.remove(&domain.name);
        match self.add(domain) {
            Ok(()) => Ok(previous),
            Err(err) => {
                if let Some(previous) = previous {
                    self.domains.insert(previous.name.clone(), previous);
                }
                Err(err)
            }
        }
    }

    /// Removes a domain.
    pub fn remove(&mut self, name: &LocalName) -> Option<LocalDomain> {
        self.domains.remove(name)
    }

    /// The domain with exactly this name.
    #[must_use]
    pub fn get(&self, name: &LocalName) -> Option<&LocalDomain> {
        self.domains.get(name)
    }

    /// All domains, by name.
    pub fn iter(&self) -> impl Iterator<Item = &LocalDomain> {
        self.domains.values()
    }

    /// Number of domains.
    #[must_use]
    pub fn len(&self) -> usize {
        self.domains.len()
    }

    /// Whether there are no domains.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.domains.is_empty()
    }

    /// Finds the domain serving `host` (an SNI name or a `Host` header, with or without
    /// `:port`). An exact name wins over a parent's wildcard; wildcards cover one level.
    #[must_use]
    pub fn lookup(&self, host: &str) -> Option<HostMatch<'_>> {
        let host = strip_port(host);
        let name = LocalName::parse(host, &self.enabled).ok()?;
        if let Some(domain) = self.domains.get(&name) {
            return Some(HostMatch {
                domain,
                via_wildcard: false,
            });
        }
        let parent = name.parent()?;
        self.domains
            .get(&parent)
            .filter(|d| d.wildcard)
            .map(|domain| HostMatch {
                domain,
                via_wildcard: true,
            })
    }

    /// The certificate to serve for an SNI name, if a domain covers it.
    #[must_use]
    pub fn cert_request(&self, sni: &str) -> Option<CertRequest> {
        self.lookup(sni).map(|m| CertRequest {
            name: m.domain.name.clone(),
            wildcard: m.domain.wildcard,
        })
    }

    /// Domains with the given suffix (for the DNS responder or mDNS).
    pub fn with_suffix(&self, suffix: Suffix) -> impl Iterator<Item = &LocalDomain> {
        self.domains
            .values()
            .filter(move |d| d.name.suffix() == suffix)
    }
}

fn strip_port(host: &str) -> &str {
    match host.rsplit_once(':') {
        Some((name, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => host,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(name: &str, wildcard: bool) -> LocalDomain {
        LocalDomain {
            name: LocalName::parse_any(name).unwrap(),
            target: DomainTarget::Port { port: 3000 },
            wildcard,
            created_at: 1_790_000_000,
        }
    }

    #[test]
    fn validates_suffix_target_and_duplicates() {
        let mut reg = DomainRegistry::new(&[Suffix::Localhost]);
        reg.add(domain("app.localhost", false)).unwrap();
        assert_eq!(
            reg.add(domain("app.localhost", true)),
            Err(RegistryError::Duplicate(
                LocalName::parse_any("app.localhost").unwrap()
            ))
        );
        assert_eq!(
            reg.add(domain("app.test", false)),
            Err(RegistryError::SuffixNotEnabled(Suffix::Test))
        );
        let mut bad = domain("x.localhost", false);
        bad.target = DomainTarget::Port { port: 0 };
        assert!(matches!(reg.add(bad), Err(RegistryError::InvalidTarget(_))));
        let mut bad = domain("y.localhost", false);
        bad.target = DomainTarget::Share { id: " ".into() };
        assert!(matches!(reg.add(bad), Err(RegistryError::InvalidTarget(_))));
        for url in [
            "localhost:3000",
            "ftp://x",
            "http://",
            "http://a/path",
            "http://a?q",
            "http://u:p@a",
            "http://a b",
        ] {
            let mut bad = domain("z.localhost", false);
            bad.target = DomainTarget::Url { url: url.into() };
            assert!(
                matches!(reg.add(bad), Err(RegistryError::InvalidTarget(_))),
                "{url}"
            );
        }
        let mut good = domain("u.localhost", false);
        good.target = DomainTarget::Url {
            url: "https://127.0.0.1:5173/".into(),
        };
        reg.add(good).unwrap();
        assert_eq!(reg.len(), 2);
    }

    #[test]
    fn lookup_prefers_exact_and_wildcards_cover_one_level() {
        let reg = DomainRegistry::from_domains(
            &Suffix::ALL,
            [
                domain("app.localhost", true),
                domain("api.app.localhost", false),
            ],
        )
        .unwrap();
        let exact = reg.lookup("API.app.localhost:8443").unwrap();
        assert_eq!(exact.domain.name.as_str(), "api.app.localhost");
        assert!(!exact.via_wildcard);
        let wild = reg.lookup("web.app.localhost").unwrap();
        assert_eq!(wild.domain.name.as_str(), "app.localhost");
        assert!(wild.via_wildcard);
        assert!(reg.lookup("a.web.app.localhost").is_none());
        assert!(reg.lookup("other.localhost").is_none());
        assert!(reg.lookup("evil.com").is_none());
        assert_eq!(
            reg.cert_request("web.app.localhost").unwrap(),
            CertRequest {
                name: LocalName::parse_any("app.localhost").unwrap(),
                wildcard: true
            }
        );
    }

    #[test]
    fn no_wildcard_no_subdomains() {
        let reg =
            DomainRegistry::from_domains(&Suffix::ALL, [domain("app.localhost", false)]).unwrap();
        assert!(reg.lookup("x.app.localhost").is_none());
    }

    #[test]
    fn update_keeps_previous_on_error() {
        let mut reg =
            DomainRegistry::from_domains(&Suffix::ALL, [domain("app.localhost", false)]).unwrap();
        let mut bad = domain("app.localhost", true);
        bad.target = DomainTarget::Port { port: 0 };
        assert!(reg.update(bad).is_err());
        assert!(
            !reg.get(&LocalName::parse_any("app.localhost").unwrap())
                .unwrap()
                .wildcard
        );
        assert!(reg.update(domain("app.localhost", true)).unwrap().is_some());
    }

    #[test]
    fn serde_shape() {
        let json = serde_json::to_value(domain("app.localhost", true)).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "name": "app.localhost",
                "target": { "kind": "port", "port": 3000 },
                "wildcard": true,
                "createdAt": 1_790_000_000
            })
        );
        let back: LocalDomain = serde_json::from_value(json).unwrap();
        assert_eq!(back, domain("app.localhost", true));
    }
}
