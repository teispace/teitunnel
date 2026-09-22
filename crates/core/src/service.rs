//! Always-on connectors: OS services that run a tunnel's connector without the app
//! (launchd agents on macOS). The app installs, removes and observes them; it never
//! supervises their process.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};

use cloudflared::launchd::{self, AgentState, LaunchAgent};

/// A boxed future (the port is object-safe so the app can pick a backend at runtime).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Installs and removes connector services.
pub trait ServiceManager: std::fmt::Debug + Send + Sync {
    /// Writes the agent and starts it (replacing an existing one with the same label).
    fn install<'a>(&'a self, agent: &'a LaunchAgent) -> BoxFuture<'a, Result<(), String>>;
    /// Stops and removes an agent. Removing one that isn't installed is fine.
    fn uninstall<'a>(&'a self, label: &'a str) -> BoxFuture<'a, Result<(), String>>;
    /// Whether an agent is loaded, and its pid.
    fn state<'a>(&'a self, label: &'a str) -> BoxFuture<'a, AgentState>;
}

/// The current user's id, from the owner of the home directory.
#[cfg(unix)]
pub fn user_id() -> Option<u32> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(std::env::var_os("HOME")?)
        .ok()
        .map(|m| m.uid())
}

/// The current user's id (not used off Unix).
#[cfg(not(unix))]
pub fn user_id() -> Option<u32> {
    None
}

/// launchd, in the user's GUI domain; agents live in `~/Library/LaunchAgents`.
#[derive(Debug, Clone)]
pub struct Launchd {
    agents_dir: PathBuf,
    domain: String,
}

impl Launchd {
    /// launchd for the current user, or `None` when the user can't be determined.
    pub fn for_current_user() -> Option<Self> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(Self {
            agents_dir: home.join("Library/LaunchAgents"),
            domain: launchd::gui_domain(user_id()?),
        })
    }

    fn plist_path(&self, label: &str) -> PathBuf {
        self.agents_dir.join(format!("{label}.plist"))
    }
}

async fn run(mut command: tokio::process::Command) -> Result<(i32, String), String> {
    let output = command.output().await.map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&output.stdout).into_owned()
        + &String::from_utf8_lossy(&output.stderr);
    Ok((output.status.code().unwrap_or(-1), text))
}

impl ServiceManager for Launchd {
    fn install<'a>(&'a self, agent: &'a LaunchAgent) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            tokio::fs::create_dir_all(&self.agents_dir)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(dir) = agent.log_file.parent() {
                tokio::fs::create_dir_all(dir)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            // Replace a previous version cleanly.
            let _ = run(launchd::bootout(&self.domain, &agent.label)).await;
            let path = self.plist_path(&agent.label);
            tokio::fs::write(&path, agent.plist())
                .await
                .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
            let (code, text) = run(launchd::bootstrap(&self.domain, &path)).await?;
            if code == 0 {
                Ok(())
            } else {
                Err(format!(
                    "launchctl couldn't start the connector: {}",
                    text.trim()
                ))
            }
        })
    }

    fn uninstall<'a>(&'a self, label: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let _ = run(launchd::bootout(&self.domain, label)).await;
            match tokio::fs::remove_file(self.plist_path(label)).await {
                Ok(()) => Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e.to_string()),
            }
        })
    }

    fn state<'a>(&'a self, label: &'a str) -> BoxFuture<'a, AgentState> {
        Box::pin(async move {
            match run(launchd::print(&self.domain, label)).await {
                Ok((0, text)) => launchd::parse_print(&text),
                _ => AgentState::default(),
            }
        })
    }
}

/// Writes a run token for a service (directory 0700, file 0600; SECURITY_MODEL).
///
/// # Errors
/// File system errors.
pub fn write_token_file(dir: &Path, tunnel_id: &str, token: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(tunnel_id);
    #[cfg(unix)]
    {
        use std::{
            io::Write,
            os::unix::fs::{OpenOptionsExt, PermissionsExt},
        };
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        let _ = std::fs::remove_file(&path);
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)?;
        file.write_all(token.as_bytes())?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, token)?;
    Ok(path)
}

/// A service manager that runs agents as child processes, for tests (and platforms
/// without a supported service manager in development).
#[derive(Debug, Default)]
pub struct ProcessServices {
    children: std::sync::Mutex<std::collections::HashMap<String, tokio::process::Child>>,
    /// Every install/uninstall, in order (`install <label>`, `uninstall <label>`).
    pub calls: std::sync::Mutex<Vec<String>>,
}

impl ServiceManager for ProcessServices {
    fn install<'a>(&'a self, agent: &'a LaunchAgent) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let mut command = tokio::process::Command::new(&agent.program);
            command
                .args(&agent.args)
                .env_remove("TUNNEL_TOKEN")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true);
            let child = command.spawn().map_err(|e| e.to_string())?;
            let mut children = self
                .children
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            children.insert(agent.label.clone(), child);
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(format!("install {}", agent.label));
            Ok(())
        })
    }

    fn uninstall<'a>(&'a self, label: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            let child = self
                .children
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .remove(label);
            if let Some(mut child) = child {
                let _ = child.kill().await;
            }
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(format!("uninstall {label}"));
            Ok(())
        })
    }

    fn state<'a>(&'a self, label: &'a str) -> BoxFuture<'a, AgentState> {
        Box::pin(async move {
            let mut children = self
                .children
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match children.get_mut(label) {
                Some(child) => {
                    let running = matches!(child.try_wait(), Ok(None));
                    AgentState {
                        loaded: true,
                        pid: if running { child.id() } else { None },
                    }
                }
                None => AgentState::default(),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn token_files_are_private() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tokens = dir.path().join("tokens");
        let path = write_token_file(&tokens, "t1", "secret").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "secret");
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(&tokens), 0o700);
        // Rewriting replaces the file (a rotated token).
        write_token_file(&tokens, "t1", "rotated").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "rotated");
    }
}
