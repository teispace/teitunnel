//! Private networks as observed for a change: the account's CIDR routes, its default
//! virtual network and its private hostname routes. Pure.
//!
//! Teitunnel routes ranges to this Mac's tunnel in the default virtual network, and
//! hostnames to this Mac's tunnel. A route belongs to this Mac when it points at this
//! Mac's tunnel, so removing a network, a hostname or the tunnel removes exactly those;
//! routes to other tunnels are never touched.

use serde::Serialize;

use crate::domain::{PrivateHostname, PrivateNetwork};

/// The comment on routes Teitunnel creates (IP and hostname routes).
pub const NETWORK_COMMENT: &str = "Added by Teitunnel";

/// A private network route in the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedNetworkRoute {
    /// Route id.
    pub id: String,
    /// The range as Cloudflare stores it.
    pub network: String,
    /// Its tunnel.
    pub tunnel_id: String,
    /// That tunnel's name, when known.
    pub tunnel_name: Option<String>,
    /// Its virtual network (`None`: unknown, treated as the default one).
    pub virtual_network_id: Option<String>,
    /// Its remark.
    pub comment: String,
}

impl ObservedNetworkRoute {
    /// The range, if it parses (Cloudflare only stores valid CIDRs; anything else is
    /// ignored rather than trusted).
    pub fn range(&self) -> Option<PrivateNetwork> {
        PrivateNetwork::parse(&self.network).ok()
    }
}

/// A private hostname route in the account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ObservedHostnameRoute {
    /// Route id.
    pub id: String,
    /// The hostname as Cloudflare stores it.
    pub hostname: String,
    /// Its tunnel.
    pub tunnel_id: String,
    /// That tunnel's name, when known.
    pub tunnel_name: Option<String>,
    /// Its remark.
    pub comment: String,
}

impl ObservedHostnameRoute {
    /// The hostname, if it's one Teitunnel could have added (a wildcard route added
    /// elsewhere is stored without its `*.` and still parses; anything else is ignored).
    pub fn name(&self) -> Option<PrivateHostname> {
        PrivateHostname::parse(&self.hostname).ok()
    }
}

/// Private networks as observed for a change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NetworkState {
    /// The default virtual network's id, if the account has one.
    pub default_vnet: Option<String>,
    /// Every route in the account, sorted by network.
    pub routes: Vec<ObservedNetworkRoute>,
    /// Every private hostname route in the account, sorted by hostname; `None` when they
    /// couldn't be read (and the change didn't need them).
    pub hostnames: Option<Vec<ObservedHostnameRoute>>,
}

impl NetworkState {
    /// Routes in the default virtual network (the only one Teitunnel uses).
    pub fn in_default_vnet(&self) -> impl Iterator<Item = &ObservedNetworkRoute> {
        self.routes.iter().filter(|r| {
            match (&r.virtual_network_id, &self.default_vnet) {
                (Some(route), Some(default)) => route == default,
                // Unknown on either side: assume the default, the conservative choice.
                _ => true,
            }
        })
    }

    /// Routes to `tunnel_id`.
    pub fn of_tunnel<'a>(
        &'a self,
        tunnel_id: &'a str,
    ) -> impl Iterator<Item = &'a ObservedNetworkRoute> + 'a {
        self.routes.iter().filter(move |r| r.tunnel_id == tunnel_id)
    }

    /// Hostname routes to `tunnel_id`.
    pub fn hostnames_of_tunnel<'a>(
        &'a self,
        tunnel_id: &'a str,
    ) -> impl Iterator<Item = &'a ObservedHostnameRoute> + 'a {
        self.hostnames
            .iter()
            .flatten()
            .filter(move |r| r.tunnel_id == tunnel_id)
    }

    /// Hostname routes for `hostname`, to any tunnel.
    pub fn hostname_routes<'a>(
        &'a self,
        hostname: &'a PrivateHostname,
    ) -> impl Iterator<Item = &'a ObservedHostnameRoute> + 'a {
        self.hostnames
            .iter()
            .flatten()
            .filter(move |r| hostname.matches(&r.hostname))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn route(id: &str, network: &str, tunnel: &str, vnet: Option<&str>) -> ObservedNetworkRoute {
        ObservedNetworkRoute {
            id: id.into(),
            network: network.into(),
            tunnel_id: tunnel.into(),
            tunnel_name: None,
            virtual_network_id: vnet.map(str::to_owned),
            comment: String::new(),
        }
    }

    #[test]
    fn filters_by_virtual_network_and_tunnel() {
        let state = NetworkState {
            default_vnet: Some("v-default".into()),
            routes: vec![
                route("a", "10.0.0.0/24", "t1", Some("v-default")),
                route("b", "10.0.0.0/24", "t2", Some("v-lab")),
                route("c", "10.1.0.0/24", "t1", None),
            ],
            hostnames: Some(vec![
                ObservedHostnameRoute {
                    id: "h1".into(),
                    hostname: "wiki.internal".into(),
                    tunnel_id: "t1".into(),
                    tunnel_name: None,
                    comment: NETWORK_COMMENT.into(),
                },
                ObservedHostnameRoute {
                    id: "h2".into(),
                    hostname: "Nas.Internal".into(),
                    tunnel_id: "t2".into(),
                    tunnel_name: None,
                    comment: String::new(),
                },
            ]),
        };
        let default: Vec<&str> = state.in_default_vnet().map(|r| r.id.as_str()).collect();
        assert_eq!(default, ["a", "c"]);
        let ours: Vec<&str> = state.of_tunnel("t1").map(|r| r.id.as_str()).collect();
        assert_eq!(ours, ["a", "c"]);
        assert_eq!(
            state.routes[0].range().map(|n| n.to_string()).as_deref(),
            Some("10.0.0.0/24")
        );
        let ours: Vec<&str> = state
            .hostnames_of_tunnel("t1")
            .map(|r| r.id.as_str())
            .collect();
        assert_eq!(ours, ["h1"]);
        let nas = PrivateHostname::parse("nas.internal").unwrap();
        let named: Vec<&str> = state.hostname_routes(&nas).map(|r| r.id.as_str()).collect();
        assert_eq!(named, ["h2"]);
    }
}
