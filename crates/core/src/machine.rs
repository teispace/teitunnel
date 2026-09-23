//! This Mac's tunnels: one connector per account's machine tunnel, run either by the app
//! (Session mode) or by the OS as a service (Always-on, survives quit and reboot).
//!
//! Session connectors get their run token through the `TUNNEL_TOKEN` environment
//! variable, never argv; the token lives in the keychain (`tunnel:<id>`). A service can't
//! read the keychain, so Always-on writes the token to a 0600 file that exists only while
//! the service is installed (SECURITY_MODEL). Each tunnel keeps its metrics port in
//! `tunnels_local`.

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use cloudflared::{
    LogLevel, Protocol, RunCmd, TokenSource, TunnelToken, launchd, service::ServiceSpec,
};

use crate::{
    Secret,
    binary::BinaryManager,
    engine::{CloudApi, Connectors, Local},
    runtime::{ConnectorId, ConnectorSpec, ConnectorState, PortAllocator, Supervisor},
    secrets::{Secrets, spawn_blocking},
    service::{ServiceManager, write_token_file},
    traffic,
};

/// How long a new connector may take to connect before a mode switch is abandoned.
const SWITCH_TIMEOUT: Duration = Duration::from_secs(30);
/// How many of a connector's newest log lines are searched for one route's.
const ROUTE_LOG_SCAN: usize = 5_000;

/// The supervisor id of a tunnel's connector.
pub fn connector_id(tunnel_id: &str) -> ConnectorId {
    ConnectorId(format!("tunnel-{tunnel_id}"))
}

fn token_key(tunnel_id: &str) -> String {
    format!("tunnel:{tunnel_id}")
}

/// This Mac's name for a new tunnel: the host name without `.local`.
pub fn machine_name() -> String {
    sysinfo::System::host_name()
        .map(|host| host.trim_end_matches(".local").to_owned())
        .filter(|host| !host.is_empty())
        .unwrap_or_else(|| "My Mac".to_owned())
}

/// Where Always-on connectors keep their token files and logs.
#[derive(Debug, Clone)]
pub struct ServicePaths {
    /// `<app_data>/tokens` (0700).
    pub tokens: PathBuf,
    /// `<logs>/connectors`.
    pub logs: PathBuf,
}

/// An Always-on connector as the app sees it.
#[derive(Debug, Clone)]
struct Service {
    port: u16,
    state: ConnectorState,
}

/// Runs this Mac's tunnel connectors. Cheap to clone.
#[derive(Debug, Clone)]
pub struct MachineTunnels {
    supervisor: Supervisor,
    binary: BinaryManager,
    ports: PortAllocator,
    secrets: Secrets,
    local: Local,
    /// Metrics ports of Session connectors, by tunnel id.
    held: Arc<Mutex<HashMap<String, u16>>>,
    /// Always-on connectors, by tunnel id.
    services: Arc<Mutex<HashMap<String, Service>>>,
    manager: Option<Arc<dyn ServiceManager>>,
    paths: Option<ServicePaths>,
    traffic: traffic::TrafficLog,
}

async fn wait_ready(port: u16, timeout: Duration) -> bool {
    let Ok(endpoints) = cloudflared::Endpoints::new(port) else {
        return false;
    };
    let deadline = tokio::time::Instant::now() + timeout;
    while tokio::time::Instant::now() < deadline {
        if endpoints
            .ready()
            .await
            .is_ok_and(|r| r.ready_connections > 0)
        {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    false
}

impl MachineTunnels {
    /// Connectors on `supervisor`, with metrics ports from `ports` (`TUNNEL_PORTS`).
    pub fn new(
        supervisor: Supervisor,
        binary: BinaryManager,
        ports: PortAllocator,
        secrets: Secrets,
        local: Local,
    ) -> Self {
        Self {
            supervisor,
            binary,
            ports,
            secrets,
            local,
            held: Arc::default(),
            services: Arc::default(),
            manager: None,
            paths: None,
            traffic: traffic::TrafficLog::default(),
        }
    }

    /// Enables Always-on through `manager` (launchd on macOS).
    #[must_use]
    pub fn with_services(mut self, manager: Arc<dyn ServiceManager>, paths: ServicePaths) -> Self {
        self.manager = Some(manager);
        self.paths = Some(paths);
        self
    }

    /// Whether Always-on is available.
    pub fn supports_always_on(&self) -> bool {
        self.manager.is_some()
    }

    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<String, u16>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn services(&self) -> std::sync::MutexGuard<'_, HashMap<String, Service>> {
        self.services.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether a tunnel's connector runs as a service.
    pub fn is_always_on(&self, tunnel_id: &str) -> bool {
        self.services().contains_key(tunnel_id)
    }

    /// A tunnel connector's recent traffic: the samples after `since` (ms; all of the
    /// last hour without it) and the latest numbers. None until it has been sampled.
    /// Reading keeps the connector on the 1 s sampling rate for a few seconds.
    pub fn traffic(&self, tunnel_id: &str, since: Option<f64>) -> Option<traffic::Traffic> {
        self.traffic.watch(tunnel_id);
        self.traffic.get(tunnel_id, since)
    }

    /// A tunnel's persisted traffic for `range`, bucketed for charting.
    ///
    /// # Errors
    /// Database errors, as a message.
    pub async fn traffic_history(
        &self,
        tunnel_id: &str,
        range: traffic::HistoryRange,
    ) -> Result<traffic::TrafficSeries, String> {
        #[allow(clippy::cast_possible_truncation)]
        let now = (traffic::now_ms() / 60_000.0) as i64;
        let rollups = self
            .local
            .rollups(tunnel_id, now - range.minutes())
            .await
            .map_err(|e| e.to_string())?;
        Ok(traffic::bucket(&rollups, range))
    }

    /// Samples connectors' metrics (every second while watched, else every 10 s),
    /// persists finished minutes, and refreshes Always-on connectors' state from their
    /// `/ready` endpoint. Run once, in the background.
    pub async fn sample_forever(self) {
        let mut tick = tokio::time::interval(traffic::LIVE_INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tick.tick().await;
            self.sample(false).await;
        }
    }

    /// Samples every connector now (tests).
    pub async fn sample_once(&self) {
        self.sample(true).await;
    }

    /// The metrics port of each running connector. While switching modes both run;
    /// the service is the one that stays when switching on, and it's out of `services`
    /// when switching off, so it wins.
    fn metrics_ports(&self) -> HashMap<String, (u16, bool)> {
        let mut ports: HashMap<String, (u16, bool)> = self
            .held()
            .iter()
            .map(|(t, p)| (t.clone(), (*p, false)))
            .collect();
        ports.extend(
            self.services()
                .iter()
                .map(|(t, s)| (t.clone(), (s.port, true))),
        );
        ports
    }

    async fn sample(&self, force: bool) {
        let now = std::time::Instant::now();
        let due: Vec<(String, (u16, bool))> = self
            .metrics_ports()
            .into_iter()
            .filter(|(tunnel, _)| self.traffic.take_due(tunnel, now) || force)
            .collect();
        let scrapes = due
            .iter()
            .map(|(tunnel, (port, service))| self.sample_one(tunnel, *port, *service));
        let rollups: Vec<_> = futures_util::future::join_all(scrapes)
            .await
            .into_iter()
            .flatten()
            .collect();
        if let Err(err) = self.local.save_rollups(rollups).await {
            tracing::warn!(%err, "couldn't save connector traffic");
        }
    }

    async fn sample_one(
        &self,
        tunnel: &str,
        port: u16,
        service: bool,
    ) -> Option<traffic::MinuteRollup> {
        let endpoints = cloudflared::Endpoints::new(port).ok()?;
        let rollup = match endpoints.metrics().await {
            Ok(metrics) => self.traffic.record_now(tunnel, metrics),
            Err(_) => None,
        };
        if service {
            // launchd appends to the log forever; keep it bounded.
            if let Some(file) = self.service_log(tunnel)
                && let Err(err) = crate::connector_logs::rotate_if_large(&file)
            {
                tracing::warn!(%err, "couldn't rotate a connector log");
            }
            let state = match endpoints.ready().await {
                Ok(ready) if ready.ready_connections > 0 => ConnectorState::Healthy {
                    connections: ready.ready_connections,
                },
                Ok(_) => ConnectorState::Connecting,
                Err(_) => ConnectorState::Stopped,
            };
            if let Some(service) = self.services().get_mut(tunnel) {
                service.state = state;
            }
        }
        rollup
    }

    /// Forgets a tunnel's live traffic once no connector runs for it, keeping its
    /// unfinished minute.
    async fn traffic_stopped(&self, tunnel_id: &str) {
        if self.held().contains_key(tunnel_id) || self.services().contains_key(tunnel_id) {
            return;
        }
        if let Some(partial) = self.traffic.forget(tunnel_id) {
            let _ = self.local.save_rollups(vec![partial]).await;
        }
    }

    /// The connector's newest log events, oldest first. Always-on connectors' come from
    /// their log file.
    pub fn logs(&self, tunnel_id: &str, limit: usize) -> Vec<Arc<cloudflared::LogEvent>> {
        if let Some(file) = self.service_log(tunnel_id) {
            return crate::connector_logs::tail_lines(&file, limit)
                .iter()
                .map(|line| Arc::new(cloudflared::parse_line(line)))
                .collect();
        }
        self.supervisor
            .logs(&connector_id(tunnel_id), limit)
            .unwrap_or_default()
    }

    /// The newest `limit` log events about requests for one route of `account`'s tunnel
    /// (matched by its rule in the ingress Teitunnel applied), oldest first.
    ///
    /// # Errors
    /// Database errors, as a message.
    pub async fn route_logs(
        &self,
        account: &str,
        hostname: &str,
        path: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Arc<cloudflared::LogEvent>>, String> {
        let local = |e: crate::store::StoreError| e.to_string();
        let Some(tunnel) = self.local.machine_tunnel(account).await.map_err(local)? else {
            return Ok(Vec::new());
        };
        let ingress = self
            .local
            .applied_ingress(account)
            .await
            .map_err(local)?
            .unwrap_or_default();
        let Some(filter) = crate::connector_logs::RouteFilter::new(&ingress, hostname, path) else {
            return Ok(Vec::new());
        };
        // Scan everything held: most lines aren't about this route.
        let mut matching: Vec<_> = self
            .logs(&tunnel.tunnel_id, ROUTE_LOG_SCAN)
            .into_iter()
            .filter(|e| filter.matches(e))
            .collect();
        matching.drain(..matching.len().saturating_sub(limit));
        Ok(matching)
    }

    /// An Always-on connector's log file.
    fn service_log(&self, tunnel_id: &str) -> Option<PathBuf> {
        let paths = self.paths.as_ref()?;
        self.is_always_on(tunnel_id)
            .then(|| paths.logs.join(format!("{tunnel_id}.log")))
    }

    /// The run token: from the keychain, else fetched (and stored).
    async fn token<C: CloudApi>(
        &self,
        api: &C,
        account: &str,
        tunnel_id: &str,
    ) -> Result<Secret<String>, String> {
        let secrets = self.secrets.clone();
        let key = token_key(tunnel_id);
        if let Some(token) = spawn_blocking(move || secrets.get(&key))
            .await
            .map_err(|e| e.to_string())?
        {
            return Ok(token);
        }
        let token = api
            .tunnel_token(account, tunnel_id)
            .await
            .map_err(|e| e.to_string())?;
        let secrets = self.secrets.clone();
        let key = token_key(tunnel_id);
        let stored = token.clone();
        spawn_blocking(move || secrets.set(&key, &stored))
            .await
            .map_err(|e| e.to_string())?;
        Ok(token)
    }

    /// Starts `account`'s machine tunnel if it has one and it isn't running (app launch):
    /// a Session connector, or makes sure its service is installed.
    ///
    /// # Errors
    /// A message for the UI.
    pub async fn resume<C: CloudApi>(&self, api: &C, account: &str) -> Result<bool, String> {
        let Some(tunnel) = self
            .local
            .machine_tunnel(account)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(false);
        };
        if tunnel.always_on && self.manager.is_some() {
            let port = tunnel.metrics_port.unwrap_or(0);
            let label = launchd::label(&tunnel.tunnel_id);
            let loaded = match &self.manager {
                Some(manager) => manager.state(&label).await.loaded,
                None => false,
            };
            if loaded && port != 0 {
                self.ports.claim(port);
                self.services().insert(
                    tunnel.tunnel_id.clone(),
                    Service {
                        port,
                        state: ConnectorState::Connecting,
                    },
                );
                return Ok(true);
            }
            // The service is gone (removed by hand, a new Mac…): install it again.
            let token = self.token(api, account, &tunnel.tunnel_id).await?;
            self.install_service(account, &tunnel.tunnel_id, &token)
                .await?;
            return Ok(true);
        }
        if self.is_running(&tunnel.tunnel_id) {
            return Ok(true);
        }
        let token = self.token(api, account, &tunnel.tunnel_id).await?;
        self.start(account, &tunnel.tunnel_id, token).await?;
        Ok(true)
    }

    /// Writes the token file and installs the service on a fresh port. Returns the port.
    async fn install_service(
        &self,
        account: &str,
        tunnel_id: &str,
        token: &Secret<String>,
    ) -> Result<u16, String> {
        let (Some(manager), Some(paths)) = (&self.manager, &self.paths) else {
            return Err("Always-on isn't available on this system.".into());
        };
        let binary = self.binary.current().await.map_err(|e| e.to_string())?;
        let (dir, id, secret) = (
            paths.tokens.clone(),
            tunnel_id.to_owned(),
            token.expose().clone(),
        );
        let token_file = tokio::task::spawn_blocking(move || write_token_file(&dir, &id, &secret))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Couldn't write the connector's token file: {e}"))?;
        let port = self
            .ports
            .allocate()
            .ok_or("No free port for the connector's metrics (20300–20399 are all busy)")?;
        let command = RunCmd {
            token: TokenSource::File(token_file),
            metrics_port: port,
            protocol: Protocol::Auto,
            log_level: LogLevel::Info,
            log_dir: None,
        }
        .build(&binary.path);
        let agent = ServiceSpec::new(
            tunnel_id,
            &command,
            paths.logs.join(format!("{tunnel_id}.log")),
        )
        .map_err(|e| e.to_string())?;
        if let Err(err) = manager.install(&agent).await {
            self.ports.release(port);
            self.remove_token_file(tunnel_id);
            return Err(err);
        }
        self.services().insert(
            tunnel_id.to_owned(),
            Service {
                port,
                state: ConnectorState::Connecting,
            },
        );
        if let Err(err) = self.local.set_metrics_port(account, port).await {
            tracing::warn!(%err, "couldn't remember the metrics port");
        }
        Ok(port)
    }

    fn remove_token_file(&self, tunnel_id: &str) {
        if let Some(paths) = &self.paths {
            let _ = std::fs::remove_file(paths.tokens.join(tunnel_id));
        }
    }

    async fn uninstall_service(&self, tunnel_id: &str) -> Result<(), String> {
        if let Some(manager) = &self.manager {
            manager.uninstall(&launchd::label(tunnel_id)).await?;
        }
        self.remove_token_file(tunnel_id);
        let service = self.services().remove(tunnel_id);
        if let Some(service) = service {
            self.ports.release(service.port);
        }
        self.traffic_stopped(tunnel_id).await;
        Ok(())
    }

    /// Switches `account`'s connector between Session and Always-on without a gap: the
    /// new one starts and connects before the old one stops. If the new one doesn't
    /// connect within 30 s, the old one keeps running and this returns an error.
    ///
    /// # Errors
    /// A message for the UI.
    pub async fn set_always_on<C: CloudApi>(
        &self,
        api: &C,
        account: &str,
        always_on: bool,
    ) -> Result<(), String> {
        let tunnel = self
            .local
            .machine_tunnel(account)
            .await
            .map_err(|e| e.to_string())?
            .ok_or("This Mac doesn't have a tunnel yet. Add a route first.")?;
        let id = tunnel.tunnel_id.as_str();
        if self.is_always_on(id) == always_on {
            return self
                .local
                .set_always_on(account, always_on)
                .await
                .map_err(|e| e.to_string());
        }
        let token = self.token(api, account, id).await?;
        if always_on {
            let port = self.install_service(account, id, &token).await?;
            if !wait_ready(port, SWITCH_TIMEOUT).await {
                let _ = self.uninstall_service(id).await;
                let old = self.held().get(id).copied();
                if let Some(old) = old {
                    let _ = self.local.set_metrics_port(account, old).await;
                }
                return Err("The always-on connector didn't connect within 30 seconds. This Mac keeps using the app's connector.".into());
            }
            // Now stop the app's connector.
            self.stop_session(id).await;
        } else {
            let service_port = self.services().get(id).map(|s| s.port);
            // Start the app's connector next to the service (on its own port).
            self.services().remove(id);
            if let Err(err) = self.start_session(account, id, &token).await {
                self.restore_service(id, service_port);
                return Err(err);
            }
            if !self.wait_session_healthy(id).await {
                self.stop_session(id).await;
                self.restore_service(id, service_port);
                return Err("The app's connector didn't connect within 30 seconds. The always-on connector keeps running.".into());
            }
            if let Some(manager) = &self.manager {
                manager.uninstall(&launchd::label(id)).await?;
            }
            self.remove_token_file(id);
            if let Some(port) = service_port {
                self.ports.release(port);
            }
        }
        self.local
            .set_always_on(account, always_on)
            .await
            .map_err(|e| e.to_string())
    }

    /// Moves `account`'s connector onto the current cloudflared binary (after an update),
    /// without a gap for an Always-on connector: a temporary app connector serves while
    /// the service is reinstalled, and goes once the service is ready again. A Session
    /// connector restarts in place (a blip shorter than the 20 s before "down" notifies).
    /// Returns whether a connector was restarted.
    ///
    /// # Errors
    /// A message for the UI. Whatever was serving before keeps serving.
    pub async fn restart_on_current_binary<C: CloudApi>(
        &self,
        api: &C,
        account: &str,
    ) -> Result<bool, String> {
        let Some(tunnel) = self
            .local
            .machine_tunnel(account)
            .await
            .map_err(|e| e.to_string())?
        else {
            return Ok(false);
        };
        let id = tunnel.tunnel_id.as_str();
        let service_port = self.services().get(id).map(|s| s.port);
        let running = service_port.is_some() || self.held().contains_key(id);
        if !running {
            return Ok(false);
        }
        let token = self.token(api, account, id).await?;
        let Some(old_port) = service_port else {
            self.stop_session(id).await;
            self.start_session(account, id, &token).await?;
            return if self.wait_session_healthy(id).await {
                Ok(true)
            } else {
                Err("The connector didn't reconnect within 30 seconds after the update.".into())
            };
        };

        // Bridge: the app's connector carries traffic while the service restarts.
        self.start_session(account, id, &token).await?;
        if !self.wait_session_healthy(id).await {
            self.stop_session(id).await;
            return Err("Couldn't restart the always-on connector on the new cloudflared: a temporary connector didn't connect. It keeps running the previous version.".into());
        }
        let port = match self.install_service(account, id, &token).await {
            Ok(port) => port,
            Err(err) => {
                // The service is gone; the bridge keeps the routes up until relaunch
                // reinstalls it (`resume`).
                self.ports.release(old_port);
                return Err(format!(
                    "Couldn't reinstall the always-on connector: {err}. The app's connector serves the routes for now."
                ));
            }
        };
        self.ports.release(old_port);
        if !wait_ready(port, SWITCH_TIMEOUT).await {
            return Err("The always-on connector didn't reconnect within 30 seconds after the update. The app's connector serves the routes for now.".into());
        }
        self.stop_session(id).await;
        Ok(true)
    }

    /// Starts the app's own connector for a tunnel (never a service).
    async fn start_session(
        &self,
        account: &str,
        tunnel_id: &str,
        token: &Secret<String>,
    ) -> Result<(), String> {
        let binary = self.binary.current().await.map_err(|e| e.to_string())?;
        // A crash-looped connector is still registered; clear it before starting again.
        let id = connector_id(tunnel_id);
        if self.supervisor.state(&id).is_some() {
            self.stop_session(tunnel_id).await;
        }
        let port = self.port_for(account).await?;
        let command = RunCmd {
            token: TokenSource::Env(TunnelToken::new(token.expose().clone())),
            metrics_port: port,
            protocol: Protocol::Auto,
            log_level: LogLevel::Info,
            log_dir: None,
        }
        .build(&binary.path);
        match self.supervisor.start(ConnectorSpec::new(id, command, port)) {
            Ok(()) => {
                self.held().insert(tunnel_id.to_owned(), port);
                Ok(())
            }
            Err(err) => {
                self.ports.release(port);
                Err(err.to_string())
            }
        }
    }

    /// Stops the app's own connector for a tunnel, if it runs, and releases its port.
    async fn stop_session(&self, tunnel_id: &str) {
        // Not running is fine: the goal is that it's stopped.
        let _ = self.supervisor.stop(&connector_id(tunnel_id)).await;
        let port = self.held().remove(tunnel_id);
        if let Some(port) = port {
            self.ports.release(port);
        }
        self.traffic_stopped(tunnel_id).await;
    }

    /// Waits up to 30 s for the app's connector to reach the edge.
    async fn wait_session_healthy(&self, tunnel_id: &str) -> bool {
        self.supervisor
            .wait_for(&connector_id(tunnel_id), SWITCH_TIMEOUT, |s| {
                matches!(s, ConnectorState::Healthy { .. })
            })
            .await
            .is_some()
    }

    fn restore_service(&self, tunnel_id: &str, port: Option<u16>) {
        if let Some(port) = port {
            self.services().insert(
                tunnel_id.to_owned(),
                Service {
                    port,
                    state: ConnectorState::Connecting,
                },
            );
        }
    }

    /// Stops `account`'s connector and deletes its token (before signing out).
    pub async fn forget_account(&self, account: &str) {
        if let Ok(Some(tunnel)) = self.local.machine_tunnel(account).await {
            let _ = self.stop(&tunnel.tunnel_id).await;
            self.deleted(&tunnel.tunnel_id).await;
        }
    }

    /// The remembered metrics port if it's still free, else a new one (remembered).
    async fn port_for(&self, account: &str) -> Result<u16, String> {
        let remembered = self
            .local
            .machine_tunnel(account)
            .await
            .map_err(|e| e.to_string())?
            .and_then(|t| t.metrics_port);
        if let Some(port) = remembered
            && self.ports.claim(port)
        {
            return Ok(port);
        }
        let port = self
            .ports
            .allocate()
            .ok_or("No free port for the connector's metrics (20300–20399 are all busy)")?;
        if let Err(err) = self.local.set_metrics_port(account, port).await {
            tracing::warn!(%err, "couldn't remember the metrics port");
        }
        Ok(port)
    }
}

impl Connectors for MachineTunnels {
    fn state(&self, tunnel_id: &str) -> Option<ConnectorState> {
        if let Some(service) = self.services().get(tunnel_id) {
            return Some(service.state.clone());
        }
        self.supervisor.state(&connector_id(tunnel_id))
    }

    fn recent_logs(&self, tunnel_id: &str, limit: usize) -> Vec<String> {
        self.logs(tunnel_id, limit)
            .iter()
            .map(|e| match &e.error {
                Some(error) => format!("{} {error}", e.message),
                None => e.message.clone(),
            })
            .collect()
    }

    async fn start(
        &self,
        account: &str,
        tunnel_id: &str,
        token: Secret<String>,
    ) -> Result<(), String> {
        let secrets = self.secrets.clone();
        let key = token_key(tunnel_id);
        let stored = token.clone();
        spawn_blocking(move || secrets.set(&key, &stored))
            .await
            .map_err(|e| e.to_string())?;
        // An Always-on connector that stopped is reinstalled, not doubled by a Session one.
        if self.is_always_on(tunnel_id) {
            let _ = self.uninstall_service(tunnel_id).await;
            self.install_service(account, tunnel_id, &token).await?;
            return Ok(());
        }
        self.start_session(account, tunnel_id, &token).await
    }

    async fn stop(&self, tunnel_id: &str) -> Result<(), String> {
        if self.is_always_on(tunnel_id) {
            return self.uninstall_service(tunnel_id).await;
        }
        self.stop_session(tunnel_id).await;
        Ok(())
    }

    async fn deleted(&self, tunnel_id: &str) {
        self.remove_token_file(tunnel_id);
        if let Err(err) = self.local.forget_rollups(tunnel_id).await {
            tracing::warn!(%err, "couldn't delete the tunnel's traffic history");
        }
        let secrets = self.secrets.clone();
        let key = token_key(tunnel_id);
        if let Err(err) = spawn_blocking(move || secrets.delete(&key)).await {
            tracing::warn!(%err, "couldn't delete the tunnel token from the keychain");
        }
    }
}
