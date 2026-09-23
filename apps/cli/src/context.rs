//! What every command needs: the app's database and keychain, and the engine.

use std::{path::PathBuf, sync::Arc};

use teitunnel_core::{
    accounts::{Account, Accounts},
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
            engine: Engine::new(Local::new(store)),
            machine_name: teitunnel_core::machine::machine_name(),
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
        }
    }

    /// This Mac's connector for `account`, probed.
    pub(crate) async fn connectors(&self, account: &Account) -> ProbedConnectors {
        let mut connectors = ProbedConnectors::default();
        if let Ok(Some(tunnel)) = self.engine.local().machine_tunnel(&account.id).await {
            connectors
                .probe(&tunnel.tunnel_id, tunnel.metrics_port)
                .await;
        }
        connectors
    }
}
