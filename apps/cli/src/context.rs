//! What every command needs: the app's database and keychain, and the engine.

use std::{path::PathBuf, sync::Arc};

use teitunnel_core::{
    accounts::{Account, Accounts},
    binary::{BinaryManager, Locator},
    engine::{Context, Engine, Local},
    secrets::{KeychainStore, Secrets},
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

/// Shared state for one command.
#[derive(Debug)]
pub(crate) struct App {
    pub(crate) accounts: Accounts,
    pub(crate) engine: Engine,
    pub(crate) machine_name: String,
    /// The cloudflared the app uses (its managed copy, or one on the system).
    pub(crate) binary: BinaryManager,
    store: Store,
}

/// The cloudflared the app would use, found the same way (`TEITUNNEL_CLOUDFLARED`, the
/// app's managed copy, then the system's).
pub(crate) fn binary(dir: &std::path::Path) -> BinaryManager {
    BinaryManager::new(Locator::from_env(dir.join("bin")))
}

impl App {
    /// Opens the app's database and keychain.
    pub(crate) fn open() -> Result<Self, String> {
        let dir = data_dir()?;
        if !dir.join("teitunnel.db").exists() {
            return Err(
                "Teitunnel hasn't been set up on this Mac yet. Open the app and connect an account first."
                    .to_owned(),
            );
        }
        let store = Store::open(&dir.join("teitunnel.db")).map_err(|e| e.to_string())?;
        let secrets: Secrets = Arc::new(KeychainStore);
        Ok(Self {
            accounts: Accounts::new(store.clone(), secrets),
            engine: Engine::new(Local::new(store.clone())),
            machine_name: teitunnel_core::machine::machine_name(),
            binary: binary(&dir),
            store,
        })
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
