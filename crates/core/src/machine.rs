//! This Mac's tunnels: one Session-mode connector per account's machine tunnel.
//!
//! The run token lives in the keychain (`tunnel:<id>`) and reaches cloudflared through
//! the `TUNNEL_TOKEN` environment variable, never argv. Each tunnel keeps the same
//! metrics port across restarts (remembered in `tunnels_local`).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
};

use cloudflared::{LogLevel, Protocol, RunCmd, TokenSource, TunnelToken};

use crate::{
    Secret,
    binary::BinaryManager,
    engine::{CloudApi, Connectors, Local},
    runtime::{ConnectorId, ConnectorSpec, ConnectorState, PortAllocator, Supervisor},
    secrets::{Secrets, spawn_blocking},
};

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

/// Runs this Mac's tunnel connectors. Cheap to clone.
#[derive(Debug, Clone)]
pub struct MachineTunnels {
    supervisor: Supervisor,
    binary: BinaryManager,
    ports: PortAllocator,
    secrets: Secrets,
    local: Local,
    /// Metrics ports of running connectors, by tunnel id.
    held: Arc<Mutex<HashMap<String, u16>>>,
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
        }
    }

    fn held(&self) -> std::sync::MutexGuard<'_, HashMap<String, u16>> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The connector's newest log events, oldest first.
    pub fn logs(
        &self,
        tunnel_id: &str,
        limit: usize,
    ) -> Vec<std::sync::Arc<cloudflared::LogEvent>> {
        self.supervisor
            .logs(&connector_id(tunnel_id), limit)
            .unwrap_or_default()
    }

    /// Starts `account`'s machine tunnel if it has one and it isn't running (app launch).
    /// Uses the token in the keychain, or fetches it if it's missing.
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
        if self.is_running(&tunnel.tunnel_id) {
            return Ok(true);
        }
        let secrets = self.secrets.clone();
        let key = token_key(&tunnel.tunnel_id);
        let stored = spawn_blocking(move || secrets.get(&key))
            .await
            .map_err(|e| e.to_string())?;
        let token = match stored {
            Some(token) => token,
            None => api
                .tunnel_token(account, &tunnel.tunnel_id)
                .await
                .map_err(|e| e.to_string())?,
        };
        self.start(account, &tunnel.tunnel_id, token).await?;
        Ok(true)
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
        let binary = self.binary.current().await.map_err(|e| e.to_string())?;
        let secrets = self.secrets.clone();
        let key = token_key(tunnel_id);
        let stored = token.clone();
        spawn_blocking(move || secrets.set(&key, &stored))
            .await
            .map_err(|e| e.to_string())?;

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
        // Not running is fine: the goal is that it's stopped.
        let _ = self.supervisor.stop(&connector_id(tunnel_id)).await;
        let port = self.held().remove(tunnel_id);
        if let Some(port) = port {
            self.ports.release(port);
        }
        Ok(())
    }

    async fn deleted(&self, tunnel_id: &str) {
        let secrets = self.secrets.clone();
        let key = token_key(tunnel_id);
        if let Err(err) = spawn_blocking(move || secrets.delete(&key)).await {
            tracing::warn!(%err, "couldn't delete the tunnel token from the keychain");
        }
    }
}
