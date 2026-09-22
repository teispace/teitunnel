//! The observer: one consistent read of everything a plan depends on.

use std::collections::HashSet;

use futures_util::{StreamExt, TryStreamExt, stream};

use super::{
    cloud::CloudApi,
    local::Local,
    types::{ObservedRecord, ObservedTunnel, Snapshot},
};
use crate::{domain::Hostname, store::StoreError};

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
}

/// Reads the zones, this Mac's tunnel and the DNS records for `hostnames` (every
/// routed hostname when `None`).
///
/// # Errors
/// API or database errors. A tunnel deleted elsewhere isn't an error: it's observed as
/// missing, so the planner creates a new one.
pub async fn observe<C: CloudApi>(
    api: &C,
    local: &Local,
    account: &str,
    machine_name: &str,
    hostnames: Option<&[&Hostname]>,
) -> Result<Snapshot, ObserveError> {
    let machine = local.machine_tunnel(account).await?;
    let (zones, tunnel, owned) = tokio::join!(
        api.zones(account),
        observe_tunnel(api, account, machine.as_ref().map(|m| m.tunnel_id.as_str())),
        local.owned_records(account),
    );
    let (mut zones, tunnel, owned) = (zones?, tunnel?, owned?);
    let tunnel_names = if tunnel.is_some() {
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
        .into_iter()
        .filter_map(|name| {
            let zone = Hostname::parse(&name).ok()?.zone_in(&zones)?.id.clone();
            Some((zone, name))
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

    Ok(Snapshot {
        account_id: account.to_owned(),
        machine_name: machine.map_or_else(|| machine_name.to_owned(), |m| m.name),
        zones,
        tunnel,
        tunnel_names,
        records,
    })
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
