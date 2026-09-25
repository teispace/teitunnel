//! Does a name resolve for tools that use the system resolver?
//!
//! Browsers resolve `*.localhost` to loopback themselves (RFC 6761 §6.3), but curl builds,
//! Node, Java and some system resolvers may not. This asks the OS resolver (getaddrinfo)
//! so the app can tell the user when a tool won't reach `app.localhost` and suggest a
//! `.test` name instead.

use std::net::IpAddr;

use serde::{Deserialize, Serialize};

/// What the system resolver returned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Resolution {
    /// Only loopback addresses.
    Loopback,
    /// Some non-loopback addresses (e.g. a DNS server answered with a real IP).
    Elsewhere {
        /// The addresses.
        addrs: Vec<IpAddr>,
    },
    /// The name didn't resolve.
    NotFound,
}

/// Resolves `name` with the OS resolver.
pub async fn resolves_locally(name: &str) -> Resolution {
    match tokio::net::lookup_host((name, 0)).await {
        Ok(addrs) => classify(addrs.map(|a| a.ip()).collect()),
        Err(_) => Resolution::NotFound,
    }
}

fn classify(mut addrs: Vec<IpAddr>) -> Resolution {
    addrs.sort_unstable();
    addrs.dedup();
    if addrs.is_empty() {
        Resolution::NotFound
    } else if addrs.iter().all(IpAddr::is_loopback) {
        Resolution::Loopback
    } else {
        Resolution::Elsewhere { addrs }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        let lo4: IpAddr = "127.0.0.1".parse().unwrap();
        let lo6: IpAddr = "::1".parse().unwrap();
        let real: IpAddr = "93.184.216.34".parse().unwrap();
        assert_eq!(classify(vec![]), Resolution::NotFound);
        assert_eq!(classify(vec![lo4, lo6, lo4]), Resolution::Loopback);
        assert_eq!(
            classify(vec![lo4, real]),
            Resolution::Elsewhere {
                addrs: vec![real, lo4]
            }
        );
    }

    #[tokio::test]
    async fn plain_localhost_is_loopback() {
        assert_eq!(resolves_locally("localhost").await, Resolution::Loopback);
    }
}
