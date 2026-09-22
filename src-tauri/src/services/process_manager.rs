use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use regex::Regex;

use crate::error::AppError;
use crate::models::{CloudflareTunnel, QuickTunnelState, TunnelLogEvent, TunnelProcessState};
use crate::services::binary_manager::BinaryManager;

struct ActiveProcess {
    child: Child,
    tunnel_id: String,
    mode: String,
    started_at: String,
    metrics_port: Option<u16>,
}

#[derive(Clone)]
pub struct ProcessManager {
    processes: Arc<Mutex<HashMap<String, ActiveProcess>>>,
    quick_tunnel: Arc<Mutex<Option<QuickTunnelState>>>,
    login_process: Arc<Mutex<Option<Child>>>,
    app_handle: Option<AppHandle>,
}

impl ProcessManager {
    pub fn new() -> Self {
        Self {
            processes: Arc::new(Mutex::new(HashMap::new())),
            quick_tunnel: Arc::new(Mutex::new(None)),
            login_process: Arc::new(Mutex::new(None)),
            app_handle: None,
        }
    }

    pub fn set_app_handle(&mut self, handle: AppHandle) {
        self.app_handle = Some(handle);
    }

    /// Finds an available local TCP port for metrics scraping
    pub fn find_available_port() -> u16 {
        std::net::TcpListener::bind("127.0.0.1:0")
            .and_then(|l| l.local_addr())
            .map(|a| a.port())
            .unwrap_or(20241)
    }

    /// Starts a 1-click Quick Ephemeral Tunnel (trycloudflare.com)
    pub async fn start_quick_tunnel(&self, local_port: u16) -> Result<QuickTunnelState, AppError> {
        let binary_path = BinaryManager::find_binary().ok_or_else(|| {
            AppError::BinaryNotFound("cloudflared binary is not installed".into())
        })?;

        // Stop any existing quick tunnel
        self.stop_quick_tunnel().await?;

        let mut cmd = Command::new(binary_path);
        cmd.arg("tunnel")
            .arg("--url")
            .arg(format!("http://localhost:{}", local_port))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AppError::ProcessError(format!("Failed to start quick tunnel process: {}", e))
        })?;

        let pid = child.id();
        let started_at = chrono::Utc::now().to_rfc3339();

        let initial_state = QuickTunnelState {
            is_running: true,
            pid,
            local_port,
            public_url: None,
            started_at: Some(started_at.clone()),
            logs: vec![],
        };

        {
            let mut qt = self.quick_tunnel.lock().await;
            *qt = Some(initial_state.clone());
        }

        let stderr = child.stderr.take().ok_or_else(|| {
            AppError::ProcessError("Failed to capture stderr from quick tunnel".into())
        })?;

        let app_handle = self.app_handle.clone();
        let quick_tunnel_arc = self.quick_tunnel.clone();

        // Spawn background task to process output & detect trycloudflare.com URL
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            let url_regex = Regex::new(r"https://[a-zA-Z0-9-]+\.trycloudflare\.com").unwrap();

            while let Ok(Some(line)) = lines.next_line().await {
                // Check if line contains public trycloudflare URL
                if let Some(mat) = url_regex.find(&line) {
                    let public_url = mat.as_str().to_string();
                    let mut qt = quick_tunnel_arc.lock().await;
                    if let Some(ref mut state) = *qt {
                        state.public_url = Some(public_url.clone());
                    }
                    if let Some(ref app) = app_handle {
                        let _ = app.emit("quick-tunnel-ready", public_url);
                    }
                }

                // Append to logs buffer
                {
                    let mut qt = quick_tunnel_arc.lock().await;
                    if let Some(ref mut state) = *qt {
                        if state.logs.len() > 500 {
                            state.logs.remove(0);
                        }
                        state.logs.push(line.clone());
                    }
                }

                // Emit live log event
                if let Some(ref app) = app_handle {
                    let _ = app.emit(
                        "tunnel-log",
                        TunnelLogEvent {
                            tunnel_id: "quick-tunnel".into(),
                            line,
                            level: "INFO".into(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                }
            }

            // Mark as stopped when process terminates
            let mut qt = quick_tunnel_arc.lock().await;
            if let Some(ref mut state) = *qt {
                state.is_running = false;
            }
            if let Some(ref app) = app_handle {
                let _ = app.emit("quick-tunnel-stopped", ());
            }
        });

        // Store active process
        {
            let mut procs = self.processes.lock().await;
            procs.insert(
                "quick-tunnel".to_string(),
                ActiveProcess {
                    child,
                    tunnel_id: "quick-tunnel".into(),
                    mode: "quick".into(),
                    started_at,
                    metrics_port: None,
                },
            );
        }

        Ok(initial_state)
    }

    /// Stops the running quick tunnel
    pub async fn stop_quick_tunnel(&self) -> Result<(), AppError> {
        let mut procs = self.processes.lock().await;
        if let Some(mut proc) = procs.remove("quick-tunnel") {
            let _ = proc.child.kill().await;
        }

        let mut qt = self.quick_tunnel.lock().await;
        if let Some(ref mut state) = *qt {
            state.is_running = false;
        }

        if let Some(ref app) = self.app_handle {
            let _ = app.emit("quick-tunnel-stopped", ());
        }

        Ok(())
    }

    /// Returns the current state of the quick tunnel
    pub async fn get_quick_tunnel_state(&self) -> Option<QuickTunnelState> {
        let qt = self.quick_tunnel.lock().await;
        qt.clone()
    }

    /// Starts a remotely-managed tunnel using its token
    pub async fn start_remote_tunnel(
        &self,
        tunnel_id: &str,
        tunnel_token: &str,
    ) -> Result<TunnelProcessState, AppError> {
        let binary_path = BinaryManager::find_binary().ok_or_else(|| {
            AppError::BinaryNotFound("cloudflared binary is not installed".into())
        })?;

        // Stop if already running
        self.stop_tunnel(tunnel_id).await?;

        let metrics_port = Self::find_available_port();
        let mut cmd = Command::new(binary_path);
        cmd.arg("tunnel")
            .arg("--metrics")
            .arg(format!("127.0.0.1:{}", metrics_port))
            .arg("run")
            .arg("--token")
            .arg(tunnel_token)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AppError::ProcessError(format!("Failed to spawn tunnel process: {}", e))
        })?;

        let pid = child.id();
        let started_at = chrono::Utc::now().to_rfc3339();

        let stderr = child.stderr.take().ok_or_else(|| {
            AppError::ProcessError("Failed to capture stderr from tunnel".into())
        })?;

        let app_handle = self.app_handle.clone();
        let tid = tunnel_id.to_string();

        // Background task to pipe stderr logs
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let level = if line.contains("ERR") || line.contains("error") {
                    "ERR"
                } else if line.contains("WRN") || line.contains("warn") {
                    "WARN"
                } else {
                    "INFO"
                };

                if let Some(ref app) = app_handle {
                    let _ = app.emit(
                        "tunnel-log",
                        TunnelLogEvent {
                            tunnel_id: tid.clone(),
                            line,
                            level: level.into(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                }
            }

            if let Some(ref app) = app_handle {
                let _ = app.emit("tunnel-status-changed", (tid, "stopped"));
            }
        });

        let state = TunnelProcessState {
            tunnel_id: tunnel_id.to_string(),
            pid,
            is_running: true,
            started_at: Some(started_at.clone()),
            metrics_port: Some(metrics_port),
            mode: "remote".into(),
        };

        {
            let mut procs = self.processes.lock().await;
            procs.insert(
                tunnel_id.to_string(),
                ActiveProcess {
                    child,
                    tunnel_id: tunnel_id.to_string(),
                    mode: "remote".into(),
                    started_at,
                    metrics_port: Some(metrics_port),
                },
            );
        }

        if let Some(ref app) = self.app_handle {
            let _ = app.emit("tunnel-status-changed", (tunnel_id, "running"));
        }

        Ok(state)
    }

    /// Stops a running tunnel by ID or name
    pub async fn stop_tunnel(&self, tunnel_id_or_name: &str) -> Result<(), AppError> {
        let mut procs = self.processes.lock().await;
        if let Some(mut proc) = procs.remove(tunnel_id_or_name) {
            let _ = proc.child.kill().await;
        }

        let matching_keys: Vec<String> = procs
            .iter()
            .filter(|(_, p)| p.tunnel_id == tunnel_id_or_name)
            .map(|(k, _)| k.clone())
            .collect();

        for k in matching_keys {
            if let Some(mut proc) = procs.remove(&k) {
                let _ = proc.child.kill().await;
            }
        }

        if let Some(ref app) = self.app_handle {
            let _ = app.emit("tunnel-status-changed", (tunnel_id_or_name, "stopped"));
        }

        Ok(())
    }

    /// Returns list of all currently active tunnel processes
    pub async fn get_active_processes(&self) -> Vec<TunnelProcessState> {
        let procs = self.processes.lock().await;
        procs
            .values()
            .map(|p| TunnelProcessState {
                tunnel_id: p.tunnel_id.clone(),
                pid: p.child.id(),
                is_running: true,
                started_at: Some(p.started_at.clone()),
                metrics_port: p.metrics_port,
                mode: p.mode.clone(),
            })
            .collect()
    }

    /// Initiates browser login via `cloudflared tunnel login`
    pub async fn start_browser_login(&self) -> Result<(), AppError> {
        let binary_path = BinaryManager::find_binary().ok_or_else(|| {
            AppError::BinaryNotFound("cloudflared binary is not installed".into())
        })?;

        self.cancel_browser_login().await?;

        let mut cmd = Command::new(binary_path);
        cmd.arg("tunnel")
            .arg("login")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AppError::ProcessError(format!("Failed to start cloudflared login: {}", e))
        })?;

        let stderr = child.stderr.take();
        let app_handle = self.app_handle.clone();
        let login_process_arc = self.login_process.clone();

        {
            let mut proc_guard = self.login_process.lock().await;
            *proc_guard = Some(child);
        }

        tokio::spawn(async move {
            let url_regex = Regex::new(r"https://dash\.cloudflare\.com/argotunnel\?callback=[^\s]+").unwrap();

            if let Some(stderr) = stderr {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();

                while let Ok(Some(line)) = lines.next_line().await {
                    if let Some(mat) = url_regex.find(&line) {
                        let login_url = mat.as_str().to_string();
                        if let Some(ref app) = app_handle {
                            let _ = app.emit("browser-login-url", login_url);
                        }
                    }

                    if let Some(ref app) = app_handle {
                        let _ = app.emit(
                            "tunnel-log",
                            TunnelLogEvent {
                                tunnel_id: "login".into(),
                                line,
                                level: "INFO".into(),
                                timestamp: chrono::Utc::now().to_rfc3339(),
                            },
                        );
                    }
                }
            }

            let mut proc_guard = login_process_arc.lock().await;
            if let Some(mut child) = proc_guard.take() {
                let status = child.wait().await;
                let cert_exists = BinaryManager::get_cert_path().is_some();
                if let Some(ref app) = app_handle {
                    if cert_exists || status.map(|s| s.success()).unwrap_or(false) {
                        let _ = app.emit("browser-login-success", ());
                    } else {
                        let _ = app.emit("browser-login-failed", "Login was cancelled or failed");
                    }
                }
            }
        });

        Ok(())
    }

    /// Cancels running browser login process
    pub async fn cancel_browser_login(&self) -> Result<(), AppError> {
        let mut proc_guard = self.login_process.lock().await;
        if let Some(mut child) = proc_guard.take() {
            let _ = child.kill().await;
        }
        Ok(())
    }

    /// Starts a named tunnel locally configured with cert.pem
    pub async fn start_named_tunnel(&self, name: &str) -> Result<TunnelProcessState, AppError> {
        let binary_path = BinaryManager::find_binary().ok_or_else(|| {
            AppError::BinaryNotFound("cloudflared binary is not installed".into())
        })?;

        self.stop_tunnel(name).await?;

        let metrics_port = Self::find_available_port();
        let mut cmd = Command::new(binary_path);
        cmd.arg("tunnel")
            .arg("--metrics")
            .arg(format!("127.0.0.1:{}", metrics_port))
            .arg("run")
            .arg(name)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            AppError::ProcessError(format!("Failed to spawn named tunnel: {}", e))
        })?;

        let pid = child.id();
        let started_at = chrono::Utc::now().to_rfc3339();

        let stderr = child.stderr.take().ok_or_else(|| {
            AppError::ProcessError("Failed to capture stderr from tunnel".into())
        })?;

        let app_handle = self.app_handle.clone();
        let tid = name.to_string();

        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let level = if line.contains("ERR") || line.contains("error") {
                    "ERR"
                } else if line.contains("WRN") || line.contains("warn") {
                    "WARN"
                } else {
                    "INFO"
                };

                if let Some(ref app) = app_handle {
                    let _ = app.emit(
                        "tunnel-log",
                        TunnelLogEvent {
                            tunnel_id: tid.clone(),
                            line,
                            level: level.into(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                        },
                    );
                }
            }

            if let Some(ref app) = app_handle {
                let _ = app.emit("tunnel-status-changed", (tid, "stopped"));
            }
        });

        let state = TunnelProcessState {
            tunnel_id: name.to_string(),
            pid,
            is_running: true,
            started_at: Some(started_at.clone()),
            metrics_port: Some(metrics_port),
            mode: "local".into(),
        };

        {
            let mut procs = self.processes.lock().await;
            procs.insert(
                name.to_string(),
                ActiveProcess {
                    child,
                    tunnel_id: name.to_string(),
                    mode: "local".into(),
                    started_at,
                    metrics_port: Some(metrics_port),
                },
            );
        }

        if let Some(ref app) = self.app_handle {
            let _ = app.emit("tunnel-status-changed", (name, "running"));
        }

        Ok(state)
    }

    /// Lists tunnels using the origin certificate (cert.pem)
    pub async fn list_cert_tunnels(&self) -> Result<Vec<CloudflareTunnel>, AppError> {
        let binary_path = BinaryManager::find_binary().ok_or_else(|| {
            AppError::BinaryNotFound("cloudflared binary is not installed".into())
        })?;

        let output = Command::new(binary_path)
            .arg("tunnel")
            .arg("list")
            .arg("-o")
            .arg("json")
            .output()
            .await
            .map_err(|e| AppError::ProcessError(format!("Failed to run tunnel list: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::ProcessError(format!("cloudflared tunnel list failed: {}", err)));
        }

        let text = String::from_utf8_lossy(&output.stdout);
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed == "null" {
            return Ok(Vec::new());
        }

        let tunnels: Option<Vec<CloudflareTunnel>> = serde_json::from_str(trimmed)
            .map_err(|e| AppError::SerializationError(format!("Failed to parse tunnel list JSON: {}", e)))?;

        Ok(tunnels.unwrap_or_default())
    }

    /// Creates a tunnel using origin certificate (cert.pem)
    pub async fn create_cert_tunnel(&self, name: &str) -> Result<CloudflareTunnel, AppError> {
        let binary_path = BinaryManager::find_binary().ok_or_else(|| {
            AppError::BinaryNotFound("cloudflared binary is not installed".into())
        })?;

        let output = Command::new(binary_path)
            .arg("tunnel")
            .arg("create")
            .arg("-o")
            .arg("json")
            .arg(name)
            .output()
            .await
            .map_err(|e| AppError::ProcessError(format!("Failed to create tunnel: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            return Err(AppError::ProcessError(format!("cloudflared tunnel create failed: {}", err)));
        }

        if let Ok(tunnel) = serde_json::from_slice::<CloudflareTunnel>(&output.stdout) {
            return Ok(tunnel);
        }

        let all = self.list_cert_tunnels().await?;
        all.into_iter()
            .find(|t| t.name == name)
            .ok_or_else(|| AppError::ProcessError("Tunnel created but not found in tunnel list".into()))
    }

    /// Deletes a tunnel using origin certificate (cert.pem)
    pub async fn delete_cert_tunnel(&self, tunnel_id_or_name: &str) -> Result<(), AppError> {
        let _ = self.stop_tunnel(tunnel_id_or_name).await;

        let binary_path = BinaryManager::find_binary().ok_or_else(|| {
            AppError::BinaryNotFound("cloudflared binary is not installed".into())
        })?;

        let output = Command::new(binary_path)
            .arg("tunnel")
            .arg("delete")
            .arg("-f")
            .arg(tunnel_id_or_name)
            .output()
            .await
            .map_err(|e| AppError::ProcessError(format!("Failed to delete tunnel: {}", e)))?;

        if !output.status.success() {
            let err = String::from_utf8_lossy(&output.stderr);
            let lower = err.to_lowercase();
            if !lower.contains("not found") && !lower.contains("does not exist") && !lower.contains("already deleted") {
                return Err(AppError::ProcessError(format!("cloudflared tunnel delete failed: {}", err)));
            }
        }

        // Clean up ~/.cloudflared/<tunnel-id>.json if present
        if let Some(home) = dirs::home_dir() {
            let creds_file = home.join(".cloudflared").join(format!("{}.json", tunnel_id_or_name));
            if creds_file.exists() {
                let _ = std::fs::remove_file(creds_file);
            }
        }

        Ok(())
    }
}
