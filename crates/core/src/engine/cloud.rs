//! Ports the engine talks through, so it can run against Cloudflare or a fake.

use std::future::Future;

use cf_api::{Client, DnsRecord, NewDnsRecord, Tunnel, TunnelConfig, VersionedConfig};

use super::types::ZoneRef;
use crate::{Secret, runtime::ConnectorState};

/// The Cloudflare operations the engine needs, for one credential.
pub trait CloudApi: Send + Sync {
    /// Zones in the account.
    fn zones(&self, account: &str) -> impl Future<Output = cf_api::Result<Vec<ZoneRef>>> + Send;
    /// A tunnel, or `None` if it doesn't exist (any more).
    fn tunnel(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<Option<Tunnel>>> + Send;
    /// A tunnel's remote configuration.
    fn tunnel_config(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<VersionedConfig>> + Send;
    /// Replaces a tunnel's remote configuration.
    fn put_tunnel_config(
        &self,
        account: &str,
        id: &str,
        config: &TunnelConfig,
    ) -> impl Future<Output = cf_api::Result<VersionedConfig>> + Send;
    /// The account's tunnels (not deleted), with their connections.
    fn tunnels(&self, account: &str) -> impl Future<Output = cf_api::Result<Vec<Tunnel>>> + Send;
    /// Names of the account's tunnels (to give a new one a unique name).
    fn tunnel_names(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<Vec<String>>> + Send {
        async move {
            Ok(self
                .tunnels(account)
                .await?
                .into_iter()
                .map(|t| t.name)
                .collect())
        }
    }
    /// Removes a tunnel's stale connections.
    fn clean_connections(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<()>> + Send;
    /// The token a connector runs the tunnel with.
    fn tunnel_token(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<Secret<String>>> + Send;
    /// Creates a remotely-managed tunnel.
    fn create_tunnel(
        &self,
        account: &str,
        name: &str,
    ) -> impl Future<Output = cf_api::Result<Tunnel>> + Send;
    /// Removes stale connections, then deletes the tunnel.
    fn delete_tunnel(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<()>> + Send;
    /// Records with exactly this name.
    fn records_named(
        &self,
        zone: &str,
        name: &str,
    ) -> impl Future<Output = cf_api::Result<Vec<DnsRecord>>> + Send;
    /// Every CNAME in a zone.
    fn cname_records(
        &self,
        zone: &str,
    ) -> impl Future<Output = cf_api::Result<Vec<DnsRecord>>> + Send;
    /// Creates a record.
    fn create_record(
        &self,
        zone: &str,
        record: &NewDnsRecord,
    ) -> impl Future<Output = cf_api::Result<DnsRecord>> + Send;
    /// Updates a record.
    fn update_record(
        &self,
        zone: &str,
        id: &str,
        record: &NewDnsRecord,
    ) -> impl Future<Output = cf_api::Result<DnsRecord>> + Send;
    /// Deletes a record.
    fn delete_record(
        &self,
        zone: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<()>> + Send;
}

/// This Mac's side of a tunnel: the connector process and its token.
pub trait Connectors: Send + Sync {
    /// The connector's state (`None` when it isn't running).
    fn state(&self, tunnel_id: &str) -> Option<ConnectorState>;
    /// Whether the connector for a tunnel is running (or restarting).
    fn is_running(&self, tunnel_id: &str) -> bool {
        self.state(tunnel_id).is_some_and(|s| {
            !matches!(
                s,
                ConnectorState::Stopped | ConnectorState::CrashLoop { .. }
            )
        })
    }
    /// Starts the connector for a tunnel with its run token.
    fn start(
        &self,
        account: &str,
        tunnel_id: &str,
        token: Secret<String>,
    ) -> impl Future<Output = Result<(), String>> + Send;
    /// Stops the connector for a tunnel.
    fn stop(&self, tunnel_id: &str) -> impl Future<Output = Result<(), String>> + Send;
    /// The tunnel was deleted: forget its token.
    fn deleted(&self, tunnel_id: &str) -> impl Future<Output = ()> + Send;
}

fn gone(err: &cf_api::Error) -> bool {
    err.status() == Some(404)
}

impl CloudApi for Client {
    async fn zones(&self, account: &str) -> cf_api::Result<Vec<ZoneRef>> {
        Ok(Client::zones(self, account)
            .await?
            .into_iter()
            .map(|z| ZoneRef {
                id: z.id,
                name: z.name,
            })
            .collect())
    }

    async fn tunnel(&self, account: &str, id: &str) -> cf_api::Result<Option<Tunnel>> {
        match Client::tunnel(self, account, id).await {
            Ok(tunnel) if tunnel.deleted_at.is_none() => Ok(Some(tunnel)),
            Ok(_) => Ok(None),
            Err(err) if gone(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }

    async fn tunnel_config(&self, account: &str, id: &str) -> cf_api::Result<VersionedConfig> {
        Client::tunnel_config(self, account, id).await
    }

    async fn put_tunnel_config(
        &self,
        account: &str,
        id: &str,
        config: &TunnelConfig,
    ) -> cf_api::Result<VersionedConfig> {
        Client::put_tunnel_config(self, account, id, config).await
    }

    async fn tunnels(&self, account: &str) -> cf_api::Result<Vec<Tunnel>> {
        Client::tunnels(self, account).await
    }

    async fn clean_connections(&self, account: &str, id: &str) -> cf_api::Result<()> {
        Client::clean_connections(self, account, id).await
    }

    async fn tunnel_token(&self, account: &str, id: &str) -> cf_api::Result<Secret<String>> {
        Client::tunnel_token(self, account, id)
            .await
            .map(Secret::new)
    }

    async fn create_tunnel(&self, account: &str, name: &str) -> cf_api::Result<Tunnel> {
        Client::create_tunnel(self, account, name).await
    }

    async fn delete_tunnel(&self, account: &str, id: &str) -> cf_api::Result<()> {
        match Client::clean_connections(self, account, id).await {
            Err(err) if !gone(&err) => return Err(err),
            _ => {}
        }
        Client::delete_tunnel(self, account, id).await
    }

    async fn records_named(&self, zone: &str, name: &str) -> cf_api::Result<Vec<DnsRecord>> {
        self.dns_records_named(zone, name).await
    }

    async fn cname_records(&self, zone: &str) -> cf_api::Result<Vec<DnsRecord>> {
        self.dns_records_of_type(zone, "CNAME").await
    }

    async fn create_record(&self, zone: &str, record: &NewDnsRecord) -> cf_api::Result<DnsRecord> {
        self.create_dns_record(zone, record).await
    }

    async fn update_record(
        &self,
        zone: &str,
        id: &str,
        record: &NewDnsRecord,
    ) -> cf_api::Result<DnsRecord> {
        self.update_dns_record(zone, id, record).await
    }

    async fn delete_record(&self, zone: &str, id: &str) -> cf_api::Result<()> {
        self.delete_dns_record(zone, id).await
    }
}
