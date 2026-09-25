//! What every command needs: the app's database and keychain, and the engine.

use std::{path::PathBuf, sync::Arc};

use teitunnel_core::{
    Secret,
    accounts::{Account, Accounts},
    binary::{BinaryManager, Locator},
    engine::{Context, Engine, Local},
    machine::{MachineTunnels, ServicePaths},
    runtime::{PidRegistry, PortAllocator, Supervisor, TUNNEL_PORTS},
    secrets::{KeychainStore, MemoryStore, Secrets},
    store::Store,
};

use crate::probe::ProbedConnectors;

/// The app's bundle identifier; its data folder is named after it.
const IDENTIFIER: &str = "com.teispace.teitunnel";

/// The app's data folder (`TEITUNNEL_DATA_DIR` overrides it, as for the app).
pub(crate) fn data_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("TEITUNNEL_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    dirs::data_dir()
        .map(|dir| dir.join(IDENTIFIER))
        .ok_or_else(|| "Couldn't find the data folder.".to_owned())
}

/// An API token from the environment (`CLOUDFLARE_API_TOKEN` or `TEITUNNEL_API_TOKEN`,
/// or a `…_FILE` variant naming a file that holds it, as Docker secrets do), for servers
/// and containers without a keychain. It's used for this command only, never stored.
fn env_token() -> Result<Option<Secret<String>>, String> {
    for name in ["TEITUNNEL_API_TOKEN", "CLOUDFLARE_API_TOKEN"] {
        if let Ok(token) = std::env::var(name)
            && !token.trim().is_empty()
        {
            return Ok(Some(Secret::new(token.trim().to_owned())));
        }
        if let Some(path) = std::env::var_os(format!("{name}_FILE")) {
            let token = std::fs::read_to_string(&path).map_err(|e| {
                format!(
                    "Couldn't read {name}_FILE ({}): {e}",
                    PathBuf::from(&path).display()
                )
            })?;
            return Ok(Some(Secret::new(token.trim().to_owned())));
        }
    }
    Ok(None)
}

/// Cloudflare's API, or (debug builds only, for tests) `TEITUNNEL_API_BASE`. A release
/// build never lets the environment redirect where the API token is sent.
fn accounts(store: Store, secrets: Secrets) -> Accounts {
    if cfg!(debug_assertions)
        && let Ok(base) = std::env::var("TEITUNNEL_API_BASE")
    {
        return Accounts::with_api_base(store, secrets, &base, None);
    }
    Accounts::new(store, secrets)
}

/// Where route checks go: Cloudflare's edge, or (debug builds only, for tests)
/// `TEITUNNEL_EDGE`.
pub(crate) fn edge() -> teitunnel_core::engine::Edge {
    if cfg!(debug_assertions)
        && let Some(addr) = std::env::var("TEITUNNEL_EDGE")
            .ok()
            .and_then(|a| a.parse().ok())
    {
        return teitunnel_core::engine::Edge::Test(addr);
    }
    teitunnel_core::engine::Edge::Cloudflare
}

/// Connects every account `token` reaches and stores it in the OS keychain (creating the
/// database if needed): `teitunnel setup` on a machine without the app.
pub(crate) async fn connect(token: Secret<String>) -> Result<Vec<Account>, String> {
    let dir = data_dir()?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let store = Store::open(&dir.join("teitunnel.db")).map_err(|e| e.to_string())?;
    let accounts = accounts(store, Arc::new(KeychainStore));
    accounts.add_token(token).await.map_err(|e| {
        format!("{e}. Without a keychain (a server or container), set CLOUDFLARE_API_TOKEN for each command instead.")
    })
}

/// Shared state for one command.
#[derive(Debug)]
pub(crate) struct App {
    pub(crate) accounts: Accounts,
    /// Shared with the MCP server when one runs in this process.
    pub(crate) engine: Arc<Engine>,
    pub(crate) machine_name: String,
    /// The cloudflared the app uses (its managed copy, or one on the system).
    pub(crate) binary: BinaryManager,
    /// The keychain, or (with a token from the environment) memory only.
    secrets: Secrets,
    dir: PathBuf,
    store: Store,
}

/// The cloudflared the app would use, found the same way (`TEITUNNEL_CLOUDFLARED`, the
/// app's managed copy, then the system's).
pub(crate) fn binary(dir: &std::path::Path) -> BinaryManager {
    BinaryManager::new(Locator::from_env(dir.join("bin")))
}

impl App {
    /// Opens the app's database and keychain. With an API token in the environment it
    /// works without the app (servers, containers): the database is created if needed,
    /// and the accounts the token reaches are used with the token kept in memory.
    pub(crate) async fn open() -> Result<Self, String> {
        let dir = data_dir()?;
        let token = env_token()?;
        if token.is_none() && !dir.join("teitunnel.db").exists() {
            return Err(
                "Teitunnel hasn't been set up on this machine yet. Open the app and connect an account, or set CLOUDFLARE_API_TOKEN (see `teitunnel setup --help`)."
                    .to_owned(),
            );
        }
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let store = Store::open(&dir.join("teitunnel.db")).map_err(|e| e.to_string())?;
        let secrets: Secrets = if token.is_some() {
            Arc::new(MemoryStore::default())
        } else {
            Arc::new(KeychainStore)
        };
        let accounts = accounts(store.clone(), Arc::clone(&secrets));
        if let Some(token) = token {
            accounts
                .add_token(token)
                .await
                .map_err(|e| format!("The API token from the environment didn't work: {e}"))?;
        }
        Ok(Self {
            accounts,
            engine: Arc::new(Engine::new(Local::new(store.clone()))),
            machine_name: teitunnel_core::machine::machine_name(),
            binary: binary(&dir),
            secrets,
            dir,
            store,
        })
    }

    /// Like [`App::open`], but on a machine where Teitunnel isn't set up yet it opens
    /// with no accounts (in memory) instead of failing: `teitunnel mcp` still offers
    /// Quick Shares, discovery and the rest that needs no account.
    pub(crate) async fn open_or_empty() -> Result<Self, String> {
        let dir = data_dir()?;
        if env_token()?.is_some() || dir.join("teitunnel.db").exists() {
            return Self::open().await;
        }
        let store = Store::open_in_memory().map_err(|e| e.to_string())?;
        let secrets: Secrets = Arc::new(MemoryStore::default());
        Ok(Self {
            accounts: accounts(store.clone(), Arc::clone(&secrets)),
            engine: Arc::new(Engine::new(Local::new(store.clone()))),
            machine_name: teitunnel_core::machine::machine_name(),
            binary: binary(&dir),
            secrets,
            dir,
            store,
        })
    }

    /// The app's data folder.
    pub(crate) fn dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// The keychain (memory only with a token from the environment).
    pub(crate) fn secrets(&self) -> &Secrets {
        &self.secrets
    }

    /// This machine's connectors, run by this process (`teitunnel up`), with the
    /// system's service manager for Always-on (`services`; systemd's system instance when
    /// running as root, for servers). The supervisor is returned too, to stop this
    /// process's connectors when it ends.
    pub(crate) async fn machine(&self, services: bool) -> (MachineTunnels, Supervisor) {
        let runs = self.dir.join("run-cli");
        PidRegistry::reap_abandoned(&runs).await;
        let supervisor = Supervisor::new(
            PidRegistry::for_this_process(&runs),
            tokio::runtime::Handle::current(),
        );
        let machine = MachineTunnels::new(
            supervisor.clone(),
            self.binary.clone(),
            PortAllocator::new(TUNNEL_PORTS).spread(std::process::id()),
            Arc::clone(&self.secrets),
            self.engine.local().clone(),
        );
        let manager = services
            .then(|| teitunnel_core::service::for_this_platform(&self.dir, true))
            .flatten();
        let machine = match manager {
            Some(manager) => machine.with_services(
                manager,
                ServicePaths {
                    tokens: self.dir.join("tokens"),
                    logs: self.dir.join("logs").join("connectors"),
                },
            ),
            None => machine,
        };
        (machine, supervisor)
    }

    /// The account named (by id or name) or, with none named, the only one.
    pub(crate) async fn account(&self, wanted: Option<&str>) -> Result<Account, String> {
        let accounts = self.accounts.list().await.map_err(|e| e.to_string())?;
        match wanted {
            Some(wanted) => accounts
                .into_iter()
                .find(|a| a.id == wanted || a.name.eq_ignore_ascii_case(wanted))
                .ok_or_else(|| format!("No connected account called \"{wanted}\".")),
            None => match accounts.len() {
                0 => Err("No Cloudflare account is connected. Connect one in Teitunnel.".into()),
                1 => Ok(accounts.into_iter().next().ok_or("No account.")?),
                _ => Err(format!(
                    "Several accounts are connected; choose one with --account ({}).",
                    accounts
                        .iter()
                        .map(|a| a.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
            },
        }
    }

    /// The database.
    pub(crate) fn store(&self) -> &Store {
        &self.store
    }

    /// Uptime checks and alerts for this machine's routes; `owner` names this process
    /// for the lease that keeps two processes from checking the same routes.
    pub(crate) fn monitor(
        &self,
        analytics: teitunnel_core::analytics::Analytics,
        owner: &str,
    ) -> teitunnel_core::uptime::Monitor {
        teitunnel_core::uptime::Monitor::new(
            self.store.clone(),
            self.accounts.clone(),
            analytics,
            edge(),
            owner,
        )
    }

    /// The engine context for `account`.
    pub(crate) fn context<'a>(&'a self, account: &'a Account) -> Context<'a> {
        Context {
            account: &account.id,
            machine_name: &self.machine_name,
            tunnel: None,
        }
    }

    /// Every account's connectors on this Mac, probed.
    pub(crate) async fn all_connectors(&self) -> ProbedConnectors {
        let mut connectors = ProbedConnectors::default();
        for account in self.accounts.list().await.unwrap_or_default() {
            for tunnel in self
                .engine
                .local()
                .tunnels(&account.id)
                .await
                .unwrap_or_default()
            {
                connectors
                    .probe(&tunnel.tunnel_id, tunnel.metrics_port)
                    .await;
            }
        }
        connectors
    }

    /// Doctor issues ignored in the app.
    pub(crate) async fn ignored_issues(&self) -> Vec<String> {
        teitunnel_core::settings::load(&self.store)
            .await
            .map(|s| s.ignored_issues)
            .unwrap_or_default()
    }

    /// This Mac's connectors for `account`, probed.
    pub(crate) async fn connectors(&self, account: &Account) -> ProbedConnectors {
        let mut connectors = ProbedConnectors::default();
        for tunnel in self
            .engine
            .local()
            .tunnels(&account.id)
            .await
            .unwrap_or_default()
        {
            connectors
                .probe(&tunnel.tunnel_id, tunnel.metrics_port)
                .await;
        }
        connectors
    }
}
