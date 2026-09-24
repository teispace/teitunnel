//! The observer: one consistent read of everything a plan depends on.

use std::collections::HashSet;

use futures_util::{StreamExt, TryStreamExt, stream};

use super::types::Intent;
use super::{
    access::{AccessNeed, AccessRule, AccessState, ObservedAccessApp, definition_of, domain_host},
    cloud::CloudApi,
    local::Local,
    networks::{NetworkState, ObservedNetworkRoute},
    types::{ObservedRecord, ObservedTunnel, RouteElsewhere, Snapshot},
};
use crate::{domain::Hostname, store::StoreError};

use crate::text::{Text, UserText, english_display, msg};

/// Requests in flight at once while reading DNS records.
const CONCURRENCY: usize = 4;

/// The DNS comment prefix Teitunnel writes on records it creates.
const OWNERSHIP_PREFIX: &str = "teitunnel:";

/// Why observing failed.
#[derive(Debug, thiserror::Error)]
pub enum ObserveError {
    /// Cloudflare returned an error.
    #[error(transparent)]
    Api(#[from] cf_api::Error),
    /// The local database failed.
    #[error(transparent)]
    Store(#[from] StoreError),
    /// Requiring a login needs Access permissions the credential doesn't have.
    AccessPermission,
    /// The chosen tunnel isn't one of this Mac's (any more).
    UnknownTunnel,
}

impl UserText for ObserveError {
    fn text(&self) -> Text {
        match self {
            Self::Api(err) => err.text(),
            Self::Store(err) => err.text(),
            Self::AccessPermission => msg::error::observe::access_permission(),
            Self::UnknownTunnel => msg::error::observe::unknown_tunnel(),
        }
    }
}

english_display!(ObserveError);

/// Whether a change reads something optional.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Want {
    /// Not needed.
    #[default]
    No,
    /// Needed: failing to read it fails the observation.
    Yes,
    /// Useful but incidental: a credential that can't read it doesn't block the change.
    IfAllowed,
}

/// What an observation reads beyond zones, the tunnel and DNS.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ObserveNeed {
    /// Access (logins).
    pub access: AccessNeed,
    /// Private network routes.
    pub networks: Want,
    /// The names of the account's tunnels, even when the target tunnel exists (to name
    /// a new one).
    pub tunnel_names: bool,
    /// Load balancing for a hostname.
    pub balance: super::balance::BalanceNeed,
    /// A Snapshot's Worker.
    pub site: super::sites::SiteNeed,
}

impl ObserveNeed {
    /// Nothing optional.
    pub fn none() -> Self {
        Self::default()
    }

    /// What planning `intent` needs.
    pub fn of(intent: &Intent) -> Self {
        Self {
            access: AccessNeed::of(intent),
            networks: match intent {
                Intent::AddNetwork { .. } | Intent::RemoveNetwork { .. } => Want::Yes,
                // Removing the tunnel removes its routes, when they can be read.
                Intent::RemoveTunnel => Want::IfAllowed,
                _ => Want::No,
            },
            tunnel_names: matches!(intent, Intent::CreateTunnel { .. }),
            balance: match intent {
                Intent::BalanceRoute { hostname } | Intent::UnbalanceRoute { hostname } => {
                    super::balance::BalanceNeed {
                        hostname: Some(hostname.to_string()),
                        want: Want::Yes,
                    }
                }
                // A route joining or leaving a balanced hostname updates its pool, when
                // load balancing can be read at all.
                Intent::AddRoute { route } => super::balance::BalanceNeed {
                    hostname: Some(route.hostname.to_string()),
                    want: Want::IfAllowed,
                },
                Intent::RemoveRoute { hostname, .. } => super::balance::BalanceNeed {
                    hostname: Some(hostname.to_string()),
                    want: Want::IfAllowed,
                },
                _ => super::balance::BalanceNeed::default(),
            },
            site: intent
                .site()
                .map(|site| super::sites::SiteNeed {
                    script: Some(site.script.clone()),
                    hostname: match intent {
                        Intent::PublishSnapshot { .. } | Intent::UpdateSnapshot { .. } => {
                            site.address.hostname().map(ToString::to_string)
                        }
                        _ => None,
                    },
                })
                .unwrap_or_default(),
        }
    }
}

/// Reads the zones, one of this Mac's tunnels (`target`, or the default one) and the
/// DNS records for `hostnames` (every routed hostname when `None`), plus what `need`
/// asks for. The routes of this Mac's other tunnels come from the local store.
///
/// # Errors
/// API or database errors, or [`ObserveError::UnknownTunnel`] for a `target` that isn't
/// this Mac's. A tunnel deleted elsewhere isn't an error: it's observed as missing, so
/// the planner creates a new one.
pub async fn observe<C: CloudApi>(
    api: &C,
    local: &Local,
    account: &str,
    target: Option<&str>,
    machine_name: &str,
    hostnames: Option<&[&Hostname]>,
    need: &ObserveNeed,
) -> Result<Snapshot, ObserveError> {
    let machine = local.tunnel(account, target).await?;
    if target.is_some() && machine.is_none() {
        return Err(ObserveError::UnknownTunnel);
    }
    let elsewhere = routes_elsewhere(local, account, machine.as_ref()).await?;
    let (zones, tunnel, owned) = tokio::join!(
        api.zones(account),
        observe_tunnel(api, account, machine.as_ref().map(|m| m.tunnel_id.as_str())),
        local.owned_records(account),
    );
    let (mut zones, tunnel, owned) = (zones?, tunnel?, owned?);
    let tunnel_names = if tunnel.is_some() && !need.tunnel_names {
        Vec::new()
    } else {
        let mut names = api.tunnel_names(account).await?;
        names.sort_unstable();
        names
    };
    zones.sort_by(|a, b| a.name.cmp(&b.name));

    let mut names: Vec<String> = match hostnames {
        Some(list) => list.iter().map(ToString::to_string).collect(),
        None => tunnel
            .iter()
            .flat_map(|t| t.ingress.iter().filter_map(|r| r.hostname.clone()))
            .collect(),
    };
    names.sort_unstable();
    names.dedup();

    let lookups: Vec<(String, String)> = names
        .iter()
        .filter_map(|name| {
            let zone = Hostname::parse(name).ok()?.zone_in(&zones)?.id.clone();
            Some((zone, name.clone()))
        })
        .collect();
    let mut records: Vec<ObservedRecord> = stream::iter(lookups)
        .map(|(zone, name)| async move {
            let found = api.records_named(&zone, &name).await?;
            Ok::<_, cf_api::Error>((zone, found))
        })
        .buffered(CONCURRENCY)
        .map_ok(|(zone, found)| {
            found
                .into_iter()
                .map(|record| ObservedRecord {
                    owned: is_owned(&record, &owned),
                    zone_id: zone.clone(),
                    record,
                })
                .collect::<Vec<_>>()
        })
        .try_concat()
        .await?;
    records.sort_by(|a, b| (&a.record.name, &a.record.id).cmp(&(&b.record.name, &b.record.id)));
    let (access, networks, balance, site) = tokio::try_join!(
        observe_access(api, local, account, &need.access, &names),
        observe_networks(api, account, need.networks),
        async {
            super::balance::observe(api, account, &zones, &need.balance)
                .await
                .map_err(ObserveError::from)
        },
        async {
            super::sites::observe(api, account, &need.site)
                .await
                .map_err(ObserveError::from)
        },
    )?;

    Ok(Snapshot {
        account_id: account.to_owned(),
        machine_name: machine.map_or_else(|| machine_name.to_owned(), |m| m.name),
        zones,
        tunnel,
        tunnel_names,
        elsewhere,
        records,
        access,
        networks,
        balance,
        site,
    })
}

/// The routes Teitunnel last wrote to this Mac's other tunnels in `account`.
async fn routes_elsewhere(
    local: &Local,
    account: &str,
    target: Option<&super::local::LocalTunnel>,
) -> Result<Vec<RouteElsewhere>, ObserveError> {
    let mut found = Vec::new();
    for tunnel in local.tunnels(account).await? {
        if target.is_some_and(|t| t.tunnel_id == tunnel.tunnel_id) {
            continue;
        }
        let ingress = local.applied_ingress(&tunnel.tunnel_id).await?;
        found.extend(ingress.unwrap_or_default().into_iter().filter_map(|rule| {
            Some(RouteElsewhere {
                tunnel: tunnel.name.clone(),
                hostname: rule.hostname?,
                path: rule.path,
            })
        }));
    }
    found.sort_by(|a, b| (&a.hostname, &a.path).cmp(&(&b.hostname, &b.path)));
    Ok(found)
}

/// Reads the account's private network routes and its default virtual network.
async fn observe_networks<C: CloudApi>(
    api: &C,
    account: &str,
    want: Want,
) -> Result<Option<NetworkState>, ObserveError> {
    if want == Want::No {
        return Ok(None);
    }
    let read = tokio::try_join!(
        api.network_routes(account),
        api.default_virtual_network(account)
    );
    let (routes, default_vnet) = match read {
        Ok(read) => read,
        Err(err) if want == Want::IfAllowed && err.is_auth() => {
            tracing::warn!("couldn't read private network routes: {err}");
            return Ok(None);
        }
        Err(err) => return Err(err.into()),
    };
    let mut routes: Vec<ObservedNetworkRoute> = routes
        .into_iter()
        .map(|r| ObservedNetworkRoute {
            id: r.id,
            network: r.network,
            tunnel_id: r.tunnel_id,
            tunnel_name: r.tunnel_name,
            virtual_network_id: r.virtual_network_id,
            comment: r.comment,
        })
        .collect();
    routes.sort_by(|a, b| (&a.network, &a.id).cmp(&(&b.network, &b.id)));
    Ok(Some(NetworkState {
        default_vnet,
        routes,
    }))
}

/// Reads Access as far as `need` asks: the setup, the applications for the requested
/// domains, and the ones Teitunnel created for hostnames in `names`.
async fn observe_access<C: CloudApi>(
    api: &C,
    local: &Local,
    account: &str,
    need: &AccessNeed,
    names: &[String],
) -> Result<Option<AccessState>, ObserveError> {
    if need.is_empty() {
        return Ok(None);
    }
    let owned = local.owned_access_apps(account).await?;
    let mut domains = need.domains.clone();
    if need.owned {
        domains.extend(
            owned
                .iter()
                .filter(|(_, domain)| {
                    let host = domain_host(domain);
                    names.iter().any(|n| n.eq_ignore_ascii_case(host))
                })
                .map(|(_, domain)| domain.clone()),
        );
    }
    domains.sort_unstable();
    domains.dedup();
    if domains.is_empty() && !need.setup {
        return Ok(None);
    }
    let owned_ids: HashSet<&str> = owned.iter().map(|(id, _)| id.as_str()).collect();
    let setup = async {
        if need.setup {
            api.access_setup(account).await.map(Some)
        } else {
            Ok(None)
        }
    };
    let apps = stream::iter(domains)
        .map(|domain| async move { api.access_apps_for(account, &domain).await })
        .buffered(CONCURRENCY)
        .try_concat();
    let (setup, apps) = match tokio::try_join!(setup, apps) {
        Ok(read) => read,
        // Reading Teitunnel's own logins is incidental to changes that don't ask for
        // one: a token that lost its Access permission shouldn't block them.
        Err(err) if err.is_auth() && !need.setup && need.domains.is_empty() => {
            tracing::warn!("couldn't read Access applications: {err}");
            return Ok(None);
        }
        Err(err) if err.is_auth() => return Err(ObserveError::AccessPermission),
        Err(err) => return Err(err.into()),
    };
    let mut apps: Vec<ObservedAccessApp> = apps
        .into_iter()
        .map(|app| ObservedAccessApp {
            owned: owned_ids.contains(app.id.as_str()),
            rule: AccessRule::from_app(&app),
            definition: definition_of(&app),
            domain: app.domain,
            id: app.id,
        })
        .collect();
    apps.sort_by(|a, b| (&a.domain, &a.id).cmp(&(&b.domain, &b.id)));
    apps.dedup_by(|a, b| a.id == b.id);
    Ok(Some(AccessState {
        organization: setup.map(|(org, _)| org),
        login_methods: setup.map(|(_, n)| n),
        apps,
    }))
}

fn is_owned(record: &cf_api::DnsRecord, index: &HashSet<String>) -> bool {
    index.contains(&record.id)
        || record
            .comment
            .as_deref()
            .is_some_and(|c| c.starts_with(OWNERSHIP_PREFIX))
}

async fn observe_tunnel<C: CloudApi>(
    api: &C,
    account: &str,
    id: Option<&str>,
) -> Result<Option<ObservedTunnel>, cf_api::Error> {
    let Some(id) = id else { return Ok(None) };
    let Some(tunnel) = api.tunnel(account, id).await? else {
        return Ok(None);
    };
    let config = api.tunnel_config(account, id).await?;
    Ok(Some(ObservedTunnel {
        id: tunnel.id,
        name: tunnel.name,
        config_version: config.version,
        ingress: config.config.map(|c| c.ingress).unwrap_or_default(),
    }))
}
