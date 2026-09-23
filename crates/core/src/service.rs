//! Always-on connectors: OS services that run a tunnel's connector without the app
//! (launchd agents on macOS; systemd user units and scheduled tasks are ready for the
//! Linux and Windows milestones). The app installs, removes and observes them; it never
//! supervises their process.

use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
};

use cloudflared::{
    launchd,
    service::{AgentState, ServiceSpec},
};

/// A boxed future (the port is object-safe so the app can pick a backend at runtime).
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Installs and removes connector services.
pub trait ServiceManager: std::fmt::Debug + Send + Sync {
    /// Writes the agent and starts it (replacing an existing one with the same label).
    fn install<'a>(&'a self, agent: &'a ServiceSpec) -> BoxFuture<'a, Result<(), String>>;
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
    fn install<'a>(&'a self, agent: &'a ServiceSpec) -> BoxFuture<'a, Result<(), String>> {
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
            // bootout returns before the job is gone; wait for launchd to let go of it.
            for _ in 0..50 {
                if !self.state(label).await.loaded {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            }
            remove_if_present(&self.plist_path(label)).await
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

/// Fails with the command's output unless it exited 0.
async fn check(command: tokio::process::Command, what: &str) -> Result<(), String> {
    let (code, text) = run(command).await?;
    if code == 0 {
        Ok(())
    } else {
        Err(format!("{what}: {}", text.trim()))
    }
}

/// Removes a file; one that's already gone is fine.
async fn remove_if_present(path: &Path) -> Result<(), String> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// systemd user units (Linux), in `~/.config/systemd/user`. Built and unit-tested now;
/// the app uses it from M7 (Linux).
#[derive(Debug, Clone)]
pub struct Systemd {
    units_dir: PathBuf,
}

impl Systemd {
    /// systemd for the current user, or `None` without a home directory.
    pub fn for_current_user() -> Option<Self> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(Self {
            units_dir: cloudflared::systemd::units_dir(&home),
        })
    }
}

impl ServiceManager for Systemd {
    fn install<'a>(&'a self, agent: &'a ServiceSpec) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            use cloudflared::systemd;
            tokio::fs::create_dir_all(&self.units_dir)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(dir) = agent.log_file.parent() {
                tokio::fs::create_dir_all(dir)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            let path = self.units_dir.join(agent.unit_name());
            tokio::fs::write(&path, agent.unit())
                .await
                .map_err(|e| format!("Couldn't write {}: {e}", path.display()))?;
            check(
                systemd::daemon_reload(),
                "systemd couldn't reload its units",
            )
            .await?;
            // `enable --now` restarts nothing that's already running: restart explicitly.
            let _ = run(systemd::disable_now(&agent.unit_name())).await;
            check(
                systemd::enable_now(&agent.unit_name()),
                "systemd couldn't start the connector",
            )
            .await
        })
    }

    fn uninstall<'a>(&'a self, label: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            use cloudflared::systemd;
            let unit = format!("{label}.service");
            let _ = run(systemd::disable_now(&unit)).await;
            remove_if_present(&self.units_dir.join(&unit)).await?;
            let _ = run(systemd::daemon_reload()).await;
            Ok(())
        })
    }

    fn state<'a>(&'a self, label: &'a str) -> BoxFuture<'a, AgentState> {
        Box::pin(async move {
            match run(cloudflared::systemd::show(&format!("{label}.service"))).await {
                Ok((0, text)) => cloudflared::systemd::parse_show(&text),
                _ => AgentState::default(),
            }
        })
    }
}

/// Windows scheduled tasks under `\Teitunnel\`, started at the user's logon. Built and
/// unit-tested now; the app uses it from M8 (Windows).
#[derive(Debug, Clone)]
pub struct TaskScheduler {
    /// Where task definitions are written for `schtasks /XML`.
    staging: PathBuf,
    /// `DOMAIN\user`, whose logon starts the tasks.
    user: String,
}

impl TaskScheduler {
    /// Task Scheduler for the current user (from `USERDOMAIN` and `USERNAME`), staging
    /// definitions in `staging`.
    pub fn for_current_user(staging: PathBuf) -> Option<Self> {
        let user = std::env::var("USERNAME").ok()?;
        let user = match std::env::var("USERDOMAIN") {
            Ok(domain) if !domain.is_empty() => format!("{domain}\\{user}"),
            _ => user,
        };
        Some(Self { staging, user })
    }
}

impl ServiceManager for TaskScheduler {
    fn install<'a>(&'a self, agent: &'a ServiceSpec) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            use cloudflared::task_scheduler as tasks;
            tokio::fs::create_dir_all(&self.staging)
                .await
                .map_err(|e| e.to_string())?;
            let name = agent.task_name();
            let _ = run(tasks::end(&name)).await;
            let xml = self.staging.join(format!("{}.xml", agent.label));
            tokio::fs::write(&xml, agent.task_file(&self.user))
                .await
                .map_err(|e| e.to_string())?;
            let created = check(
                tasks::create(&name, &xml),
                "Task Scheduler couldn't add the connector",
            )
            .await;
            let _ = remove_if_present(&xml).await;
            created?;
            check(
                tasks::run(&name),
                "Task Scheduler couldn't start the connector",
            )
            .await
        })
    }

    fn uninstall<'a>(&'a self, label: &'a str) -> BoxFuture<'a, Result<(), String>> {
        Box::pin(async move {
            use cloudflared::task_scheduler as tasks;
            let name = format!("{}{label}", tasks::FOLDER);
            let _ = run(tasks::end(&name)).await;
            let _ = run(tasks::delete(&name)).await;
            Ok(())
        })
    }

    fn state<'a>(&'a self, label: &'a str) -> BoxFuture<'a, AgentState> {
        Box::pin(async move {
            use cloudflared::task_scheduler as tasks;
            match run(tasks::query(&format!("{}{label}", tasks::FOLDER))).await {
                Ok((0, text)) => tasks::parse_query(&text),
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
    fn install<'a>(&'a self, agent: &'a ServiceSpec) -> BoxFuture<'a, Result<(), String>> {
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;

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
