//! Ports the engine talks through, so it can run against Cloudflare or a fake.

use std::future::Future;

use cf_api::{
    AccessApp, Client, DnsRecord, NewAccessApp, NewDnsRecord, Tunnel, TunnelConfig, VersionedConfig,
};

use super::types::ZoneRef;
use crate::text::Text;
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
    /// Whether Zero Trust is set up (an Access organization exists), and how many login
    /// methods the account has.
    fn access_setup(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<(bool, usize)>> + Send;
    /// Access applications for exactly this domain.
    fn access_apps_for(
        &self,
        account: &str,
        domain: &str,
    ) -> impl Future<Output = cf_api::Result<Vec<AccessApp>>> + Send;
    /// Creates an Access application.
    fn create_access_app(
        &self,
        account: &str,
        app: &NewAccessApp,
    ) -> impl Future<Output = cf_api::Result<AccessApp>> + Send;
    /// Replaces an Access application.
    fn update_access_app(
        &self,
        account: &str,
        id: &str,
        app: &NewAccessApp,
    ) -> impl Future<Output = cf_api::Result<AccessApp>> + Send;
    /// Deletes an Access application.
    fn delete_access_app(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<()>> + Send;
    /// Every private network route in the account.
    fn network_routes(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<Vec<cf_api::NetworkRoute>>> + Send;
    /// The default virtual network's id.
    fn default_virtual_network(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<Option<String>>> + Send;
    /// Routes a range to a tunnel (in the default virtual network when `None`).
    fn create_network_route(
        &self,
        account: &str,
        network: &str,
        tunnel: &str,
        comment: &str,
        virtual_network: Option<&str>,
    ) -> impl Future<Output = cf_api::Result<cf_api::NetworkRoute>> + Send;
    /// Deletes a route.
    fn delete_network_route(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<()>> + Send;
    /// Account-wide WARP client settings (Gateway proxy).
    fn device_settings(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<cf_api::DeviceSettings>> + Send;
    /// The default device profile's Split Tunnels.
    fn default_device_profile(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<cf_api::DefaultDeviceProfile>> + Send;
    /// Adds One-time PIN as a login method; returns its id.
    fn create_one_time_pin(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<String>> + Send;
    /// Removes a login method.
    fn delete_login_method(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<()>> + Send;
    /// The account's load-balancing monitors (a paid add-on; 403 without it).
    fn lb_monitors(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<Vec<cf_api::Monitor>>> + Send;
    /// Creates a monitor.
    fn create_lb_monitor(
        &self,
        account: &str,
        monitor: &cf_api::Monitor,
    ) -> impl Future<Output = cf_api::Result<cf_api::Monitor>> + Send;
    /// Deletes a monitor.
    fn delete_lb_monitor(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<()>> + Send;
    /// The account's load-balancing pools.
    fn lb_pools(
        &self,
        account: &str,
    ) -> impl Future<Output = cf_api::Result<Vec<cf_api::Pool>>> + Send;
    /// Creates a pool.
    fn create_lb_pool(
        &self,
        account: &str,
        pool: &cf_api::Pool,
    ) -> impl Future<Output = cf_api::Result<cf_api::Pool>> + Send;
    /// Replaces a pool.
    fn update_lb_pool(
        &self,
        account: &str,
        id: &str,
        pool: &cf_api::Pool,
    ) -> impl Future<Output = cf_api::Result<cf_api::Pool>> + Send;
    /// Deletes a pool.
    fn delete_lb_pool(
        &self,
        account: &str,
        id: &str,
    ) -> impl Future<Output = cf_api::Result<()>> + Send;
    /// A zone's load balancers.
    fn load_balancers(
        &self,
        zone: &str,
    ) -> impl Future<Output = cf_api::Result<Vec<cf_api::LoadBalancer>>> + Send;
    /// Creates a load balancer.
    fn create_load_balancer(
        &self,
        zone: &str,
        balancer: &cf_api::LoadBalancer,
    ) -> impl Future<Output = cf_api::Result<cf_api::LoadBalancer>> + Send;
    /// Deletes a load balancer.
    fn delete_load_balancer(
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
    ) -> impl Future<Output = Result<(), Text>> + Send;
    /// Stops the connector for a tunnel.
    fn stop(&self, tunnel_id: &str) -> impl Future<Output = Result<(), Text>> + Send;
    /// The connector's newest log lines (message and error), oldest first.
    fn recent_logs(&self, _tunnel_id: &str, _limit: usize) -> Vec<String> {
        Vec::new()
    }
    /// The tunnel was deleted: forget its token.
    fn deleted(&self, tunnel_id: &str) -> impl Future<Output = ()> + Send;
    /// The id Cloudflare knows this machine's connector for a tunnel by (from its
    /// `/ready` endpoint), when it's running.
    fn connector_id(&self, _tunnel_id: &str) -> impl Future<Output = Option<String>> + Send {
        std::future::ready(None)
    }
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

    async fn access_setup(&self, account: &str) -> cf_api::Result<(bool, usize)> {
        if Client::access_organization(self, account).await?.is_none() {
            return Ok((false, 0));
        }
        Ok((true, Client::identity_providers(self, account).await?.len()))
    }

    async fn access_apps_for(&self, account: &str, domain: &str) -> cf_api::Result<Vec<AccessApp>> {
        Client::access_apps_for(self, account, domain).await
    }

    async fn create_access_app(
        &self,
        account: &str,
        app: &NewAccessApp,
    ) -> cf_api::Result<AccessApp> {
        Client::create_access_app(self, account, app).await
    }

    async fn update_access_app(
        &self,
        account: &str,
        id: &str,
        app: &NewAccessApp,
    ) -> cf_api::Result<AccessApp> {
        Client::update_access_app(self, account, id, app).await
    }

    async fn delete_access_app(&self, account: &str, id: &str) -> cf_api::Result<()> {
        Client::delete_access_app(self, account, id).await
    }

    async fn network_routes(&self, account: &str) -> cf_api::Result<Vec<cf_api::NetworkRoute>> {
        Client::network_routes(self, account).await
    }

    async fn default_virtual_network(&self, account: &str) -> cf_api::Result<Option<String>> {
        Ok(Client::virtual_networks(self, account)
            .await?
            .into_iter()
            .find(|v| v.is_default_network)
            .map(|v| v.id))
    }

    async fn create_network_route(
        &self,
        account: &str,
        network: &str,
        tunnel: &str,
        comment: &str,
        virtual_network: Option<&str>,
    ) -> cf_api::Result<cf_api::NetworkRoute> {
        Client::create_network_route(self, account, network, tunnel, comment, virtual_network).await
    }

    async fn delete_network_route(&self, account: &str, id: &str) -> cf_api::Result<()> {
        Client::delete_network_route(self, account, id).await
    }

    async fn device_settings(&self, account: &str) -> cf_api::Result<cf_api::DeviceSettings> {
        Client::device_settings(self, account).await
    }

    async fn default_device_profile(
        &self,
        account: &str,
    ) -> cf_api::Result<cf_api::DefaultDeviceProfile> {
        Client::default_device_profile(self, account).await
    }

    async fn create_one_time_pin(&self, account: &str) -> cf_api::Result<String> {
        Ok(Client::create_one_time_pin(self, account).await?.id)
    }

    async fn delete_login_method(&self, account: &str, id: &str) -> cf_api::Result<()> {
        self.delete_identity_provider(account, id).await
    }

    async fn lb_monitors(&self, account: &str) -> cf_api::Result<Vec<cf_api::Monitor>> {
        Client::lb_monitors(self, account).await
    }

    async fn create_lb_monitor(
        &self,
        account: &str,
        monitor: &cf_api::Monitor,
    ) -> cf_api::Result<cf_api::Monitor> {
        Client::create_lb_monitor(self, account, monitor).await
    }

    async fn delete_lb_monitor(&self, account: &str, id: &str) -> cf_api::Result<()> {
        Client::delete_lb_monitor(self, account, id).await
    }

    async fn lb_pools(&self, account: &str) -> cf_api::Result<Vec<cf_api::Pool>> {
        Client::lb_pools(self, account).await
    }

    async fn create_lb_pool(
        &self,
        account: &str,
        pool: &cf_api::Pool,
    ) -> cf_api::Result<cf_api::Pool> {
        Client::create_lb_pool(self, account, pool).await
    }

    async fn update_lb_pool(
        &self,
        account: &str,
        id: &str,
        pool: &cf_api::Pool,
    ) -> cf_api::Result<cf_api::Pool> {
        Client::update_lb_pool(self, account, id, pool).await
    }

    async fn delete_lb_pool(&self, account: &str, id: &str) -> cf_api::Result<()> {
        Client::delete_lb_pool(self, account, id).await
    }

    async fn load_balancers(&self, zone: &str) -> cf_api::Result<Vec<cf_api::LoadBalancer>> {
        Client::load_balancers(self, zone).await
    }

    async fn create_load_balancer(
        &self,
        zone: &str,
        balancer: &cf_api::LoadBalancer,
    ) -> cf_api::Result<cf_api::LoadBalancer> {
        Client::create_load_balancer(self, zone, balancer).await
    }

    async fn delete_load_balancer(&self, zone: &str, id: &str) -> cf_api::Result<()> {
        Client::delete_load_balancer(self, zone, id).await
    }
}
