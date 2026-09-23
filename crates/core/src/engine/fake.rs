//! In-memory Cloudflare and connectors for executor tests, with failure injection.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
};

use cf_api::{
    AccessApp, ApiMessage, DnsRecord, NewAccessApp, NewDnsRecord, Tunnel, TunnelConfig,
    VersionedConfig,
};

use super::{
    cloud::{CloudApi, Connectors},
    types::ZoneRef,
};
use crate::{Secret, runtime::ConnectorState};

/// A tunnel as the fake stores it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FakeTunnel {
    pub(crate) name: String,
    pub(crate) version: u64,
    pub(crate) config: Option<TunnelConfig>,
}

/// Everything the fake Cloudflare holds.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct CloudState {
    pub(crate) zones: Vec<ZoneRef>,
    pub(crate) tunnels: BTreeMap<String, FakeTunnel>,
    pub(crate) records: BTreeMap<String, Vec<DnsRecord>>,
    /// Zero Trust is set up.
    pub(crate) access_org: bool,
    /// Login method ids.
    pub(crate) login_methods: Vec<String>,
    /// Access applications by id.
    pub(crate) access_apps: BTreeMap<String, NewAccessApp>,
    /// The token can't read Access (reads fail with 403).
    pub(crate) access_forbidden: bool,
    /// Private network routes by id.
    pub(crate) network_routes: BTreeMap<String, cf_api::NetworkRoute>,
    /// The default virtual network's id.
    pub(crate) default_vnet: Option<String>,
    /// The token can't read private networks (reads fail with 403).
    pub(crate) networks_forbidden: bool,
    /// WARP client settings (`None`: can't be read).
    pub(crate) device_settings: Option<cf_api::DeviceSettings>,
    /// The default device profile (`None`: can't be read).
    pub(crate) device_profile: Option<cf_api::DefaultDeviceProfile>,
    /// Load balancing is available (the add-on and the permission); reads 403 otherwise.
    pub(crate) load_balancing: bool,
    /// Monitors, pools and load balancers (by zone) by id.
    pub(crate) lb_monitors: BTreeMap<String, cf_api::Monitor>,
    pub(crate) lb_pools: BTreeMap<String, cf_api::Pool>,
    pub(crate) load_balancers: BTreeMap<String, (String, cf_api::LoadBalancer)>,
    /// Pool endpoints (by address) whose health checks fail.
    pub(crate) lb_failing: BTreeSet<String>,
}

/// State with ids and versions stripped, for "is it back to how it was?" checks.
pub(crate) type Normalized = (
    Vec<(String, Option<serde_json::Value>)>,
    Vec<(String, String, String, String, bool, u32, Option<String>)>,
    Vec<serde_json::Value>,
    usize,
    Vec<(String, String, String, Option<String>)>,
);

impl CloudState {
    pub(crate) fn normalized(&self) -> Normalized {
        let tunnels = self
            .tunnels
            .values()
            .map(|t| {
                (
                    t.name.clone(),
                    t.config.as_ref().and_then(|c| serde_json::to_value(c).ok()),
                )
            })
            .collect();
        let mut records: Vec<_> = self
            .records
            .iter()
            .flat_map(|(zone, list)| {
                list.iter().map(|r| {
                    (
                        zone.clone(),
                        r.name.clone(),
                        r.kind.clone(),
                        r.content.clone(),
                        r.proxied,
                        r.ttl,
                        r.comment.clone(),
                    )
                })
            })
            .collect();
        records.sort();
        let mut apps: Vec<serde_json::Value> = self
            .access_apps
            .values()
            .filter_map(|a| serde_json::to_value(a).ok())
            .collect();
        apps.sort_by_key(ToString::to_string);
        let mut networks: Vec<_> = self
            .network_routes
            .values()
            .map(|r| {
                (
                    r.network.clone(),
                    r.tunnel_id.clone(),
                    r.comment.clone(),
                    r.virtual_network_id.clone(),
                )
            })
            .collect();
        networks.sort();
        (tunnels, records, apps, self.login_methods.len(), networks)
    }

    pub(crate) fn record_count(&self) -> usize {
        self.records.values().map(Vec::len).sum()
    }
}

/// The fake. Mutations are numbered from 0; `fail_from` makes mutation `n` and every
/// later one fail, `fail_once` only mutation `n`.
#[derive(Debug, Default)]
pub(crate) struct FakeCloud {
    pub(crate) state: Mutex<CloudState>,
    ids: AtomicU32,
    mutations: AtomicU32,
    fail_once: Mutex<Option<u32>>,
    fail_from: Mutex<Option<u32>>,
}

fn injected() -> cf_api::Error {
    cf_api::Error::Api {
        status: 500,
        errors: vec![ApiMessage {
            code: 1000,
            message: "injected failure".into(),
        }],
    }
}

fn forbidden() -> cf_api::Error {
    cf_api::Error::Api {
        status: 403,
        errors: vec![ApiMessage {
            code: 10000,
            message: "Authentication error".into(),
        }],
    }
}

fn conflict() -> cf_api::Error {
    cf_api::Error::Api {
        status: 409,
        errors: vec![ApiMessage {
            code: 1002,
            message: "still referenced".into(),
        }],
    }
}

fn not_found() -> cf_api::Error {
    cf_api::Error::Api {
        status: 404,
        errors: vec![ApiMessage {
            code: 1003,
            message: "not found".into(),
        }],
    }
}

impl FakeCloud {
    pub(crate) fn new(state: CloudState) -> Self {
        Self {
            state: Mutex::new(state),
            ..Self::default()
        }
    }

    pub(crate) fn snapshot(&self) -> CloudState {
        self.state.lock().unwrap().clone()
    }

    pub(crate) fn fail_once(&self, n: u32) {
        *self.fail_once.lock().unwrap() = Some(n);
    }

    pub(crate) fn fail_from(&self, n: u32) {
        *self.fail_from.lock().unwrap() = Some(n);
    }

    pub(crate) fn reset_failures(&self) {
        *self.fail_once.lock().unwrap() = None;
        *self.fail_from.lock().unwrap() = None;
        self.mutations.store(0, Ordering::SeqCst);
    }

    /// Mutations made so far.
    pub(crate) fn mutations(&self) -> u32 {
        self.mutations.load(Ordering::SeqCst)
    }

    fn mutate(&self) -> cf_api::Result<()> {
        let n = self.mutations.fetch_add(1, Ordering::SeqCst);
        let once = *self.fail_once.lock().unwrap() == Some(n);
        let from = self.fail_from.lock().unwrap().is_some_and(|f| n >= f);
        if once || from {
            Err(injected())
        } else {
            Ok(())
        }
    }

    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", self.ids.fetch_add(1, Ordering::SeqCst))
    }

    fn with_zone<T>(
        &self,
        zone: &str,
        f: impl FnOnce(&mut Vec<DnsRecord>) -> cf_api::Result<T>,
    ) -> cf_api::Result<T> {
        let mut state = self.state.lock().unwrap();
        if !state.zones.iter().any(|z| z.id == zone) {
            return Err(not_found());
        }
        f(state.records.entry(zone.to_owned()).or_default())
    }
}

fn tunnel_view(id: &str, t: &FakeTunnel) -> Tunnel {
    Tunnel {
        id: id.to_owned(),
        name: t.name.clone(),
        status: "inactive".into(),
        created_at: String::new(),
        deleted_at: None,
        remote_config: true,
        connections: Vec::new(),
    }
}

impl CloudApi for FakeCloud {
    async fn zones(&self, _account: &str) -> cf_api::Result<Vec<ZoneRef>> {
        Ok(self.state.lock().unwrap().zones.clone())
    }

    async fn tunnel(&self, _account: &str, id: &str) -> cf_api::Result<Option<Tunnel>> {
        let state = self.state.lock().unwrap();
        Ok(state.tunnels.get(id).map(|t| tunnel_view(id, t)))
    }

    async fn tunnel_config(&self, _account: &str, id: &str) -> cf_api::Result<VersionedConfig> {
        let state = self.state.lock().unwrap();
        let t = state.tunnels.get(id).ok_or_else(not_found)?;
        Ok(VersionedConfig {
            version: t.version,
            config: t.config.clone(),
        })
    }

    async fn put_tunnel_config(
        &self,
        _account: &str,
        id: &str,
        config: &TunnelConfig,
    ) -> cf_api::Result<VersionedConfig> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let t = state.tunnels.get_mut(id).ok_or_else(not_found)?;
        t.version += 1;
        t.config = Some(config.clone());
        Ok(VersionedConfig {
            version: t.version,
            config: t.config.clone(),
        })
    }

    async fn tunnels(&self, _account: &str) -> cf_api::Result<Vec<Tunnel>> {
        let state = self.state.lock().unwrap();
        Ok(state
            .tunnels
            .iter()
            .map(|(id, t)| tunnel_view(id, t))
            .collect())
    }

    async fn clean_connections(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        let state = self.state.lock().unwrap();
        state.tunnels.get(id).map(|_| ()).ok_or_else(not_found)
    }

    async fn tunnel_token(&self, _account: &str, id: &str) -> cf_api::Result<Secret<String>> {
        let state = self.state.lock().unwrap();
        state.tunnels.get(id).ok_or_else(not_found)?;
        Ok(Secret::new(format!("token-for-{id}")))
    }

    async fn create_tunnel(&self, _account: &str, name: &str) -> cf_api::Result<Tunnel> {
        self.mutate()?;
        let id = self.next_id("tunnel");
        let tunnel = FakeTunnel {
            name: name.to_owned(),
            version: 0,
            config: None,
        };
        let view = tunnel_view(&id, &tunnel);
        self.state.lock().unwrap().tunnels.insert(id, tunnel);
        Ok(view)
    }

    async fn delete_tunnel(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state
            .lock()
            .unwrap()
            .tunnels
            .remove(id)
            .map(|_| ())
            .ok_or_else(not_found)
    }

    async fn records_named(&self, zone: &str, name: &str) -> cf_api::Result<Vec<DnsRecord>> {
        self.with_zone(zone, |records| {
            Ok(records
                .iter()
                .filter(|r| r.name.eq_ignore_ascii_case(name))
                .cloned()
                .collect())
        })
    }

    async fn cname_records(&self, zone: &str) -> cf_api::Result<Vec<DnsRecord>> {
        self.with_zone(zone, |records| {
            Ok(records
                .iter()
                .filter(|r| r.kind == "CNAME")
                .cloned()
                .collect())
        })
    }

    async fn create_record(&self, zone: &str, record: &NewDnsRecord) -> cf_api::Result<DnsRecord> {
        self.mutate()?;
        let id = self.next_id("rec");
        self.with_zone(zone, |records| {
            let clash = records.iter().any(|r| {
                r.name.eq_ignore_ascii_case(&record.name)
                    && (r.kind == "CNAME" || record.kind == "CNAME")
            });
            if clash {
                return Err(cf_api::Error::Api {
                    status: 400,
                    errors: vec![ApiMessage {
                        code: 81053,
                        message: "An A, AAAA, or CNAME record with that host already exists."
                            .into(),
                    }],
                });
            }
            let created = DnsRecord {
                id,
                name: record.name.clone(),
                kind: record.kind.clone(),
                content: record.content.clone(),
                proxied: record.proxied,
                comment: record.comment.clone(),
                ttl: record.ttl,
            };
            records.push(created.clone());
            Ok(created)
        })
    }

    async fn update_record(
        &self,
        zone: &str,
        id: &str,
        record: &NewDnsRecord,
    ) -> cf_api::Result<DnsRecord> {
        self.mutate()?;
        self.with_zone(zone, |records| {
            let r = records
                .iter_mut()
                .find(|r| r.id == id)
                .ok_or_else(not_found)?;
            r.name.clone_from(&record.name);
            r.kind.clone_from(&record.kind);
            r.content.clone_from(&record.content);
            r.proxied = record.proxied;
            r.ttl = record.ttl;
            r.comment.clone_from(&record.comment);
            Ok(r.clone())
        })
    }

    async fn delete_record(&self, zone: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.with_zone(zone, |records| {
            let before = records.len();
            records.retain(|r| r.id != id);
            if records.len() == before {
                Err(not_found())
            } else {
                Ok(())
            }
        })
    }

    async fn access_setup(&self, _account: &str) -> cf_api::Result<(bool, usize)> {
        let state = self.state.lock().unwrap();
        if state.access_forbidden {
            return Err(forbidden());
        }
        Ok((state.access_org, state.login_methods.len()))
    }

    async fn access_apps_for(
        &self,
        _account: &str,
        domain: &str,
    ) -> cf_api::Result<Vec<AccessApp>> {
        let state = self.state.lock().unwrap();
        if state.access_forbidden {
            return Err(forbidden());
        }
        Ok(state
            .access_apps
            .iter()
            .filter(|(_, app)| app.domain.eq_ignore_ascii_case(domain))
            .map(|(id, app)| fake_app(id, app))
            .collect())
    }

    async fn create_access_app(
        &self,
        _account: &str,
        app: &NewAccessApp,
    ) -> cf_api::Result<AccessApp> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        if !state.access_org {
            return Err(cf_api::Error::Api {
                status: 400,
                errors: vec![ApiMessage {
                    code: 12130,
                    message: "access.api.error.not_enabled".into(),
                }],
            });
        }
        let id = self.next_id("app");
        state.access_apps.insert(id.clone(), app.clone());
        Ok(fake_app(&id, app))
    }

    async fn update_access_app(
        &self,
        _account: &str,
        id: &str,
        app: &NewAccessApp,
    ) -> cf_api::Result<AccessApp> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let slot = state.access_apps.get_mut(id).ok_or_else(not_found)?;
        *slot = app.clone();
        Ok(fake_app(id, app))
    }

    async fn delete_access_app(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        state
            .access_apps
            .remove(id)
            .map(|_| ())
            .ok_or_else(not_found)
    }

    async fn network_routes(&self, _account: &str) -> cf_api::Result<Vec<cf_api::NetworkRoute>> {
        let state = self.state.lock().unwrap();
        if state.networks_forbidden {
            return Err(forbidden());
        }
        Ok(state.network_routes.values().cloned().collect())
    }

    async fn default_virtual_network(&self, _account: &str) -> cf_api::Result<Option<String>> {
        let state = self.state.lock().unwrap();
        if state.networks_forbidden {
            return Err(forbidden());
        }
        Ok(state.default_vnet.clone())
    }

    async fn create_network_route(
        &self,
        _account: &str,
        network: &str,
        tunnel: &str,
        comment: &str,
        virtual_network: Option<&str>,
    ) -> cf_api::Result<cf_api::NetworkRoute> {
        self.mutate()?;
        let id = self.next_id("net");
        let mut state = self.state.lock().unwrap();
        let vnet = virtual_network
            .map(str::to_owned)
            .or_else(|| state.default_vnet.clone());
        // Cloudflare refuses a range that's already routed in the same virtual network.
        if state
            .network_routes
            .values()
            .any(|r| r.network == network && r.virtual_network_id == vnet)
        {
            return Err(cf_api::Error::Api {
                status: 409,
                errors: vec![ApiMessage {
                    code: 1014,
                    message: "route already exists".into(),
                }],
            });
        }
        let route = cf_api::NetworkRoute {
            id: id.clone(),
            network: network.to_owned(),
            tunnel_id: tunnel.to_owned(),
            tunnel_name: None,
            virtual_network_id: vnet,
            comment: comment.to_owned(),
        };
        state.network_routes.insert(id, route.clone());
        Ok(route)
    }

    async fn delete_network_route(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state
            .lock()
            .unwrap()
            .network_routes
            .remove(id)
            .map(|_| ())
            .ok_or_else(not_found)
    }

    async fn device_settings(&self, _account: &str) -> cf_api::Result<cf_api::DeviceSettings> {
        self.state
            .lock()
            .unwrap()
            .device_settings
            .ok_or_else(forbidden)
    }

    async fn default_device_profile(
        &self,
        _account: &str,
    ) -> cf_api::Result<cf_api::DefaultDeviceProfile> {
        self.state
            .lock()
            .unwrap()
            .device_profile
            .clone()
            .ok_or_else(forbidden)
    }

    async fn create_one_time_pin(&self, _account: &str) -> cf_api::Result<String> {
        self.mutate()?;
        let id = self.next_id("idp");
        self.state.lock().unwrap().login_methods.push(id.clone());
        Ok(id)
    }

    async fn delete_login_method(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let before = state.login_methods.len();
        state.login_methods.retain(|m| m != id);
        if state.login_methods.len() == before {
            Err(not_found())
        } else {
            Ok(())
        }
    }

    async fn lb_monitors(&self, _account: &str) -> cf_api::Result<Vec<cf_api::Monitor>> {
        let state = self.state.lock().unwrap();
        if !state.load_balancing {
            return Err(forbidden());
        }
        Ok(state.lb_monitors.values().cloned().collect())
    }

    async fn create_lb_monitor(
        &self,
        _account: &str,
        monitor: &cf_api::Monitor,
    ) -> cf_api::Result<cf_api::Monitor> {
        self.mutate()?;
        let mut created = monitor.clone();
        created.id = self.next_id("mon");
        self.state
            .lock()
            .unwrap()
            .lb_monitors
            .insert(created.id.clone(), created.clone());
        Ok(created)
    }

    async fn delete_lb_monitor(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        // Cloudflare refuses to delete a monitor a pool still uses.
        if state
            .lb_pools
            .values()
            .any(|p| p.monitor.as_deref() == Some(id))
        {
            return Err(conflict());
        }
        state.lb_monitors.remove(id);
        Ok(())
    }

    async fn lb_pools(&self, _account: &str) -> cf_api::Result<Vec<cf_api::Pool>> {
        let state = self.state.lock().unwrap();
        if !state.load_balancing {
            return Err(forbidden());
        }
        Ok(state.lb_pools.values().cloned().collect())
    }

    async fn create_lb_pool(
        &self,
        _account: &str,
        pool: &cf_api::Pool,
    ) -> cf_api::Result<cf_api::Pool> {
        self.mutate()?;
        let mut created = pool.clone();
        created.id = self.next_id("pool");
        self.state
            .lock()
            .unwrap()
            .lb_pools
            .insert(created.id.clone(), created.clone());
        Ok(created)
    }

    async fn update_lb_pool(
        &self,
        _account: &str,
        id: &str,
        pool: &cf_api::Pool,
    ) -> cf_api::Result<cf_api::Pool> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        let Some(existing) = state.lb_pools.get_mut(id) else {
            return Err(not_found());
        };
        let mut updated = pool.clone();
        updated.id = id.to_owned();
        *existing = updated.clone();
        Ok(updated)
    }

    async fn delete_lb_pool(&self, _account: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        let mut state = self.state.lock().unwrap();
        // Cloudflare refuses to delete a pool a load balancer still uses.
        if state
            .load_balancers
            .values()
            .any(|(_, lb)| lb.default_pools.iter().any(|p| p == id) || lb.fallback_pool == id)
        {
            return Err(conflict());
        }
        state.lb_pools.remove(id);
        Ok(())
    }

    async fn lb_pool_health(&self, _account: &str, id: &str) -> cf_api::Result<cf_api::PoolHealth> {
        let state = self.state.lock().unwrap();
        let pool = state.lb_pools.get(id).ok_or_else(not_found)?;
        let seen = pool
            .origins
            .iter()
            .map(|o| {
                let failing = state.lb_failing.contains(&o.address);
                cf_api::OriginHealth {
                    address: o.address.clone(),
                    healthy: !failing,
                    failure_reason: failing.then(|| "HTTP timeout occurred".to_owned()),
                    response_code: Some(if failing { 0 } else { 200 }),
                }
            })
            .collect::<Vec<_>>();
        Ok(cf_api::PoolHealth {
            regions: ["Amsterdam, NL", "Tokyo, JP"]
                .into_iter()
                .map(|r| (r.to_owned(), seen.clone()))
                .collect(),
        })
    }

    async fn load_balancers(&self, zone: &str) -> cf_api::Result<Vec<cf_api::LoadBalancer>> {
        let state = self.state.lock().unwrap();
        if !state.load_balancing {
            return Err(forbidden());
        }
        Ok(state
            .load_balancers
            .values()
            .filter(|(z, _)| z == zone)
            .map(|(_, lb)| lb.clone())
            .collect())
    }

    async fn create_load_balancer(
        &self,
        zone: &str,
        balancer: &cf_api::LoadBalancer,
    ) -> cf_api::Result<cf_api::LoadBalancer> {
        self.mutate()?;
        let mut created = balancer.clone();
        created.id = self.next_id("lb");
        self.state
            .lock()
            .unwrap()
            .load_balancers
            .insert(created.id.clone(), (zone.to_owned(), created.clone()));
        Ok(created)
    }

    async fn delete_load_balancer(&self, _zone: &str, id: &str) -> cf_api::Result<()> {
        self.mutate()?;
        self.state.lock().unwrap().load_balancers.remove(id);
        Ok(())
    }
}

fn fake_app(id: &str, app: &NewAccessApp) -> AccessApp {
    AccessApp {
        id: id.to_owned(),
        name: app.name.clone(),
        domain: app.domain.clone(),
        kind: app.kind.clone(),
        session_duration: Some(app.session_duration.clone()),
        policies: app
            .policies
            .iter()
            .enumerate()
            .map(|(i, p)| cf_api::AccessPolicy {
                id: Some(format!("{id}-policy-{i}")),
                ..p.clone()
            })
            .collect(),
    }
}

/// Records connector calls; `stop` can be made to fail.
#[derive(Debug, Default)]
pub(crate) struct FakeConnectors {
    pub(crate) calls: Mutex<Vec<String>>,
    pub(crate) running: Mutex<BTreeSet<String>>,
    pub(crate) fail_stop: AtomicBool,
    /// Connector ids of running connectors, by tunnel id.
    pub(crate) ids: Mutex<BTreeMap<String, String>>,
}

impl FakeConnectors {
    pub(crate) fn calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    fn call(&self, what: String) {
        self.calls.lock().unwrap().push(what);
    }
}

impl Connectors for FakeConnectors {
    async fn connector_id(&self, tunnel_id: &str) -> Option<String> {
        self.ids.lock().unwrap().get(tunnel_id).cloned()
    }

    fn state(&self, tunnel_id: &str) -> Option<ConnectorState> {
        self.running
            .lock()
            .unwrap()
            .contains(tunnel_id)
            .then_some(ConnectorState::Healthy { connections: 4 })
    }

    async fn start(
        &self,
        _account: &str,
        tunnel_id: &str,
        token: Secret<String>,
    ) -> Result<(), crate::text::Text> {
        assert_eq!(token.expose(), &format!("token-for-{tunnel_id}"));
        self.call(format!("start {tunnel_id}"));
        self.running.lock().unwrap().insert(tunnel_id.to_owned());
        Ok(())
    }

    async fn stop(&self, tunnel_id: &str) -> Result<(), crate::text::Text> {
        if self.fail_stop.load(Ordering::SeqCst) {
            return Err(crate::text::msg::raw("injected stop failure"));
        }
        self.call(format!("stop {tunnel_id}"));
        self.running.lock().unwrap().remove(tunnel_id);
        Ok(())
    }

    async fn deleted(&self, tunnel_id: &str) {
        self.call(format!("deleted {tunnel_id}"));
    }
}
