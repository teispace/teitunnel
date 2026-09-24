//! Settings ▸ Integrations: whether the control connection and `teitunnel://` links are
//! on, and which programs the person always allows to make changes. Stored in the
//! `settings` table under their own keys (no migration).

use serde::{Deserialize, Serialize};
use teitunnel_control::protocol::ClientInfo;

use crate::{
    settings::{read, write},
    store::{Store, StoreError},
};

const CONTROL_ENABLED: &str = "controlEnabled";
const DEEP_LINKS_ENABLED: &str = "deepLinksEnabled";
const CONTROL_CLIENTS: &str = "controlClients";

/// The most programs remembered (the oldest are forgotten first).
const MAX_CLIENTS: usize = 50;

/// A program the person always allows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ApprovedClient {
    /// The name it introduced itself with (`teitunnel-cli`, `vscode`, …).
    pub name: String,
    /// Its version when it was allowed.
    pub version: String,
    /// When (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub approved_at: u64,
}

/// The Integrations settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Integrations {
    /// The CLI, extensions and launchers may connect to the running app.
    pub control_enabled: bool,
    /// `teitunnel://` links are handled.
    pub deep_links_enabled: bool,
    /// Programs that make changes without asking each time, oldest first.
    pub clients: Vec<ApprovedClient>,
}

/// A change to the switches.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct IntegrationsPatch {
    /// Turn the control connection on or off.
    #[serde(default)]
    pub control_enabled: Option<bool>,
    /// Turn links on or off.
    #[serde(default)]
    pub deep_links_enabled: Option<bool>,
}

/// Loads the settings (both switches default to on).
///
/// # Errors
/// The database can't be read.
pub async fn load(store: &Store) -> Result<Integrations, StoreError> {
    store
        .call(|conn| {
            Ok(Integrations {
                control_enabled: read(conn, CONTROL_ENABLED)?.unwrap_or(true),
                deep_links_enabled: read(conn, DEEP_LINKS_ENABLED)?.unwrap_or(true),
                clients: read(conn, CONTROL_CLIENTS)?.unwrap_or_default(),
            })
        })
        .await
}

/// Changes the switches.
///
/// # Errors
/// The database can't be written.
pub async fn update(store: &Store, patch: IntegrationsPatch) -> Result<Integrations, StoreError> {
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            if let Some(on) = patch.control_enabled {
                write(&tx, CONTROL_ENABLED, &on)?;
            }
            if let Some(on) = patch.deep_links_enabled {
                write(&tx, DEEP_LINKS_ENABLED, &on)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    load(store).await
}

/// Whether the person always allows the program called `name`.
pub async fn is_approved(store: &Store, name: &str) -> bool {
    load(store)
        .await
        .is_ok_and(|s| s.clients.iter().any(|c| c.name == name))
}

/// Remembers that the person always allows `client`.
///
/// # Errors
/// The database can't be written.
pub async fn approve(store: &Store, client: &ClientInfo) -> Result<(), StoreError> {
    let client = ApprovedClient {
        name: client.name.clone(),
        version: client.version.clone(),
        approved_at: crate::domain_shares::now_ms(),
    };
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            let mut clients: Vec<ApprovedClient> = read(&tx, CONTROL_CLIENTS)?.unwrap_or_default();
            clients.retain(|c| c.name != client.name);
            clients.push(client);
            let excess = clients.len().saturating_sub(MAX_CLIENTS);
            clients.drain(..excess);
            write(&tx, CONTROL_CLIENTS, &clients)?;
            tx.commit()?;
            Ok(())
        })
        .await
}

/// Stops always allowing the program called `name` (it's asked again next time).
///
/// # Errors
/// The database can't be written.
pub async fn revoke(store: &Store, name: &str) -> Result<Integrations, StoreError> {
    let name = name.to_owned();
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            let mut clients: Vec<ApprovedClient> = read(&tx, CONTROL_CLIENTS)?.unwrap_or_default();
            clients.retain(|c| c.name != name);
            write(&tx, CONTROL_CLIENTS, &clients)?;
            tx.commit()?;
            Ok(())
        })
        .await?;
    load(store).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(name: &str) -> ClientInfo {
        ClientInfo {
            name: name.into(),
            version: "1.0".into(),
        }
    }

    #[tokio::test]
    async fn on_by_default_and_switchable() {
        let store = Store::open_in_memory().unwrap();
        let settings = load(&store).await.unwrap();
        assert!(settings.control_enabled && settings.deep_links_enabled);
        assert!(settings.clients.is_empty());
        let settings = update(
            &store,
            IntegrationsPatch {
                deep_links_enabled: Some(false),
                ..IntegrationsPatch::default()
            },
        )
        .await
        .unwrap();
        assert!(settings.control_enabled && !settings.deep_links_enabled);
    }

    #[tokio::test]
    async fn remembers_and_revokes_programs() {
        let store = Store::open_in_memory().unwrap();
        assert!(!is_approved(&store, "vscode").await);
        approve(&store, &client("vscode")).await.unwrap();
        approve(&store, &client("raycast")).await.unwrap();
        approve(&store, &client("vscode")).await.unwrap();
        let settings = load(&store).await.unwrap();
        let names: Vec<_> = settings.clients.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["raycast", "vscode"], "once each, latest last");
        assert!(is_approved(&store, "vscode").await);
        let settings = revoke(&store, "vscode").await.unwrap();
        assert_eq!(settings.clients.len(), 1);
        assert!(!is_approved(&store, "vscode").await);
    }

    #[tokio::test]
    async fn keeps_a_bounded_list() {
        let store = Store::open_in_memory().unwrap();
        for i in 0..(MAX_CLIENTS + 5) {
            approve(&store, &client(&format!("p{i}"))).await.unwrap();
        }
        let settings = load(&store).await.unwrap();
        assert_eq!(settings.clients.len(), MAX_CLIENTS);
        assert_eq!(settings.clients[0].name, "p5");
    }
}
