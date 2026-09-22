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
    LogLevel, Protocol, RunCmd, TokenSource, TunnelToken,
    launchd::{self, LaunchAgent},
};

use crate::{
    Secret,
    binary::BinaryManager,
    engine::{CloudApi, Connectors, Local},
    runtime::{ConnectorId, ConnectorSpec, ConnectorState, PortAllocator, Supervisor},
    secrets::{Secrets, spawn_blocking},
    service::{ServiceManager, write_token_file},
};

/// How long a new connector may take to connect before a mode switch is abandoned.
const SWITCH_TIMEOUT: Duration = Duration::from_secs(30);

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
    traffic: crate::traffic::TrafficLog,
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
            traffic: crate::traffic::TrafficLog::default(),
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

    /// The last hour of a tunnel connector's traffic (None until it has been sampled).
    pub fn traffic(&self, tunnel_id: &str) -> Option<crate::traffic::Traffic> {
        self.traffic.get(tunnel_id)
    }

    /// Samples every connector's metrics every 10 s, and refreshes Always-on connectors'
    /// state from their `/ready` endpoint. Run once, in the background.
    pub async fn sample_forever(self) {
        let mut tick = tokio::time::interval(crate::traffic::INTERVAL);
        loop {
            tick.tick().await;
            self.sample_once().await;
        }
    }

    /// One sampling pass (also used by tests).
    pub async fn sample_once(&self) {
        let mut running: Vec<(String, u16)> =
            self.held().iter().map(|(t, p)| (t.clone(), *p)).collect();
        let services: Vec<(String, u16)> = self
            .services()
            .iter()
            .map(|(t, s)| (t.clone(), s.port))
            .collect();
        running.extend(services.iter().cloned());
        for (tunnel, port) in running {
            let Ok(endpoints) = cloudflared::Endpoints::new(port) else {
                continue;
            };
            if let Ok(metrics) = endpoints.metrics().await {
                self.traffic.record_now(&tunnel, metrics);
            }
            if services.iter().any(|(t, _)| *t == tunnel) {
                let state = match endpoints.ready().await {
                    Ok(ready) if ready.ready_connections > 0 => ConnectorState::Healthy {
                        connections: ready.ready_connections,
                    },
                    Ok(_) => ConnectorState::Connecting,
                    Err(_) => ConnectorState::Stopped,
                };
                if let Some(service) = self.services().get_mut(&tunnel) {
                    service.state = state;
                }
            }
        }
    }

    /// The connector's newest log events, oldest first. Always-on connectors' come from
    /// their log file.
    pub fn logs(&self, tunnel_id: &str, limit: usize) -> Vec<Arc<cloudflared::LogEvent>> {
        if self.is_always_on(tunnel_id)
            && let Some(paths) = &self.paths
        {
            let file = paths.logs.join(format!("{tunnel_id}.log"));
            let text = std::fs::read_to_string(file).unwrap_or_default();
            let lines: Vec<&str> = text.lines().collect();
            return lines[lines.len().saturating_sub(limit)..]
                .iter()
                .map(|line| Arc::new(cloudflared::parse_line(line)))
                .collect();
        }
        self.supervisor
            .logs(&connector_id(tunnel_id), limit)
            .unwrap_or_default()
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
        let agent = LaunchAgent::new(
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
        self.traffic.forget(tunnel_id);
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
            let _ = self.supervisor.stop(&connector_id(id)).await;
            let old = self.held().remove(id);
            if let Some(old) = old {
                self.ports.release(old);
            }
        } else {
            let service_port = self.services().get(id).map(|s| s.port);
            // Start the app's connector next to the service (on its own port).
            self.services().remove(id);
            if let Err(err) = self.start(account, id, token).await {
                self.restore_service(id, service_port);
                return Err(err);
            }
            let healthy = self
                .supervisor
                .wait_for(&connector_id(id), SWITCH_TIMEOUT, |s| {
                    matches!(s, ConnectorState::Healthy { .. })
                })
                .await
                .is_some();
            if !healthy {
                let _ = self.stop(id).await;
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
        let binary = self.binary.current().await.map_err(|e| e.to_string())?;

        // A crash-looped connector is still registered; clear it before starting again.
        let id = connector_id(tunnel_id);
        if self.supervisor.state(&id).is_some() {
            self.stop(tunnel_id).await?;
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

    async fn stop(&self, tunnel_id: &str) -> Result<(), String> {
        if self.is_always_on(tunnel_id) {
            return self.uninstall_service(tunnel_id).await;
        }
        // Not running is fine: the goal is that it's stopped.
        let _ = self.supervisor.stop(&connector_id(tunnel_id)).await;
        self.traffic.forget(tunnel_id);
        let port = self.held().remove(tunnel_id);
        if let Some(port) = port {
            self.ports.release(port);
        }
        Ok(())
    }

    async fn deleted(&self, tunnel_id: &str) {
        self.remove_token_file(tunnel_id);
        let secrets = self.secrets.clone();
        let key = token_key(tunnel_id);
        if let Err(err) = spawn_blocking(move || secrets.delete(&key)).await {
            tracing::warn!(%err, "couldn't delete the tunnel token from the keychain");
        }
    }
}
