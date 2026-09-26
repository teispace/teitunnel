//! Load balancing a route across tunnels: Cloudflare Load Balancing (a
//! paid add-on) with one pool whose endpoints are the tunnels that route the hostname
//! (`<tunnel id>.cfargotunnel.com`, the hostname as Host header), a health monitor, and
//! a load balancer named after the hostname, which takes precedence over its DNS record.
//! Teitunnel marks what it creates (`teitunnel:lb=<hostname>` in the description) and
//! changes only those, on any machine; everything goes through plan → apply.

use std::collections::BTreeMap;

use futures_util::{StreamExt, stream};
use serde::Serialize;

use super::{cloud::CloudApi, observe::Want, types::ZoneRef};
use crate::domain::Hostname;

/// The ownership marker in the description of what Teitunnel creates for `hostname`.
pub fn marker(hostname: &str) -> String {
    format!("teitunnel:lb={}", hostname.to_ascii_lowercase())
}

/// The pool's name: letters, digits, `-` and `_` only.
pub fn pool_name(hostname: &str) -> String {
    let body: String = hostname
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("teitunnel-{body}")
}

/// The health monitor for `hostname`: HTTPS through the tunnel with the route's Host
/// header, any 2xx after following redirects.
pub fn monitor_for(hostname: &str) -> cf_api::Monitor {
    cf_api::Monitor {
        id: String::new(),
        kind: "https".into(),
        description: marker(hostname),
        path: "/".into(),
        expected_codes: "2xx".into(),
        follow_redirects: true,
        header: BTreeMap::from([("Host".into(), vec![hostname.to_ascii_lowercase()])]),
    }
}

/// A pool endpoint for one tunnel.
pub fn origin_for(hostname: &str, tunnel_id: &str, name: &str) -> cf_api::Origin {
    cf_api::Origin {
        name: name.to_owned(),
        address: super::types::tunnel_target(tunnel_id),
        enabled: true,
        header: BTreeMap::from([("Host".into(), vec![hostname.to_ascii_lowercase()])]),
    }
}

/// A tunnel of the account whose configuration routes the hostname.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServingTunnel {
    /// Tunnel id.
    pub id: String,
    /// Name.
    pub name: String,
}

/// Load balancing for one hostname, as observed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BalanceState {
    /// The hostname.
    pub hostname: String,
    /// Its zone.
    pub zone_id: String,
    /// The load balancer for it, if any.
    pub balancer: Option<cf_api::LoadBalancer>,
    /// Teitunnel's pool for it, if any.
    pub pool: Option<cf_api::Pool>,
    /// Teitunnel's monitor for it, if any.
    pub monitor: Option<cf_api::Monitor>,
    /// Tunnels routing the hostname (read when balancing, or when a balancer exists).
    pub serving: Vec<ServingTunnel>,
}

impl BalanceState {
    /// Whether the load balancer is Teitunnel's (or there's none).
    pub fn owned(&self) -> bool {
        self.balancer
            .as_ref()
            .is_none_or(|lb| lb.description == marker(&self.hostname))
    }

    /// Whether Teitunnel balances the hostname now.
    pub fn active(&self) -> bool {
        self.balancer.is_some() && self.owned()
    }
}

/// What a change needs to know about load balancing.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BalanceNeed {
    /// The hostname concerned.
    pub hostname: Option<String>,
    /// How much it matters.
    pub want: Want,
}

/// Reads the load balancer, pool and monitor for the hostname, and (when balancing, or
/// when a balancer exists) which tunnels route it. `None` when not needed, or when it
/// can't be read and isn't required (no add-on, no permission).
pub(crate) async fn observe<C: CloudApi>(
    api: &C,
    account: &str,
    zones: &[ZoneRef],
    need: &BalanceNeed,
) -> Result<Option<BalanceState>, cf_api::Error> {
    let Some(hostname) = need.hostname.as_deref().filter(|_| need.want != Want::No) else {
        return Ok(None);
    };
    let Some(zone) = Hostname::parse(hostname)
        .ok()
        .and_then(|h| h.zone_in(zones).cloned())
    else {
        return Ok(None);
    };
    let read = async {
        let balancer = api
            .load_balancers(&zone.id)
            .await?
            .into_iter()
            .find(|lb| lb.name.eq_ignore_ascii_case(hostname));
        let required = need.want == Want::Yes;
        if balancer.is_none() && !required {
            return Ok::<_, cf_api::Error>(BalanceState {
                hostname: hostname.to_owned(),
                zone_id: zone.id.clone(),
                balancer: None,
                pool: None,
                monitor: None,
                serving: Vec::new(),
            });
        }
        let mark = marker(hostname);
        let (pools, monitors) = tokio::try_join!(api.lb_pools(account), api.lb_monitors(account))?;
        let pool = pools.into_iter().find(|p| p.description == mark);
        let monitor = monitors.into_iter().find(|m| m.description == mark);
        let serving = serving_tunnels(api, account, hostname).await?;
        Ok(BalanceState {
            hostname: hostname.to_owned(),
            zone_id: zone.id.clone(),
            balancer,
            pool,
            monitor,
            serving,
        })
    };
    match read.await {
        Ok(state) => Ok(Some(state)),
        // Optional for this change: without the add-on or the permission (whatever the
        // API answers then), the route is simply not load balanced. A load balancer, if
        // there is one, still takes precedence over the DNS record.
        Err(err) if need.want == Want::IfAllowed => {
            tracing::debug!("load balancing isn't readable: {err}");
            Ok(None)
        }
        Err(err) => Err(err),
    }
}

/// How one machine's tunnel behind a balanced route does, from Cloudflare's health
/// checks in each region.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct EndpointHealth {
    /// Tunnel id.
    pub tunnel_id: String,
    /// The endpoint's name (the tunnel's).
    pub name: String,
    /// Whether it gets traffic.
    pub enabled: bool,
    /// Regions whose checks pass.
    pub healthy_regions: u32,
    /// Regions that checked it (0 until the first checks come in).
    pub regions: u32,
    /// Why checks fail, in Cloudflare's words (e.g. "HTTP timeout occurred").
    pub reason: Option<String>,
}

/// The health of each endpoint of Teitunnel's pool for `hostname`, in pool order;
/// empty when there's no such pool.
///
/// # Errors
/// API errors (no add-on or permission).
pub async fn health<C: CloudApi>(
    api: &C,
    account: &str,
    hostname: &str,
) -> Result<Vec<EndpointHealth>, crate::Error> {
    let mark = marker(hostname);
    let Some(pool) = api
        .lb_pools(account)
        .await?
        .into_iter()
        .find(|p| p.description == mark)
    else {
        return Ok(Vec::new());
    };
    let health = api.lb_pool_health(account, &pool.id).await?;
    Ok(pool
        .origins
        .iter()
        .map(|origin| {
            let seen: Vec<&cf_api::OriginHealth> = health
                .regions
                .values()
                .flatten()
                .filter(|h| h.address.eq_ignore_ascii_case(&origin.address))
                .collect();
            let healthy = seen.iter().filter(|h| h.healthy).count();
            let reason = seen.iter().filter(|h| !h.healthy).find_map(|h| {
                h.failure_reason.clone().or_else(|| {
                    h.response_code
                        .filter(|c| *c > 0)
                        .map(|c| format!("HTTP {c}"))
                })
            });
            EndpointHealth {
                tunnel_id: origin
                    .address
                    .strip_suffix(".cfargotunnel.com")
                    .unwrap_or(&origin.address)
                    .to_owned(),
                name: origin.name.clone(),
                enabled: origin.enabled,
                healthy_regions: u32::try_from(healthy).unwrap_or(u32::MAX),
                regions: u32::try_from(seen.len()).unwrap_or(u32::MAX),
                reason,
            }
        })
        .collect())
}

/// The account's tunnels whose remote configuration routes `hostname`, by name.
async fn serving_tunnels<C: CloudApi>(
    api: &C,
    account: &str,
    hostname: &str,
) -> Result<Vec<ServingTunnel>, cf_api::Error> {
    let tunnels = api.tunnels(account).await?;
    let mut serving: Vec<ServingTunnel> =
        stream::iter(tunnels.into_iter().filter(|t| t.remote_config))
            .map(|tunnel| async move {
                let config = api.tunnel_config(account, &tunnel.id).await.ok()?;
                config
                    .config?
                    .ingress
                    .iter()
                    .any(|r| {
                        r.hostname
                            .as_deref()
                            .is_some_and(|h| h.eq_ignore_ascii_case(hostname))
                    })
                    .then_some(ServingTunnel {
                        id: tunnel.id,
                        name: tunnel.name,
                    })
            })
            .buffered(4)
            .filter_map(std::future::ready)
            .collect()
            .await;
    serving.sort_by(|a, b| (&a.name, &a.id).cmp(&(&b.name, &b.id)));
    Ok(serving)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_what_it_creates() {
        assert_eq!(marker("App.XYZ.com"), "teitunnel:lb=app.xyz.com");
        assert_eq!(pool_name("*.app.xyz.com"), "teitunnel-__app_xyz_com");
        let monitor = monitor_for("app.xyz.com");
        assert_eq!(monitor.header["Host"], ["app.xyz.com"]);
        assert_eq!(
            origin_for("app.xyz.com", "t1", "Mac").address,
            "t1.cfargotunnel.com"
        );
    }
}
