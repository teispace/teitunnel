//! When each AI agent last used Teitunnel's MCP server, by the name its client gives
//! (`claude-code`, `cursor-vscode`…): Settings ▸ AI & Integrations shows "Last used"
//! from it, the surest sign a connected tool really works. Kept in the `settings`
//! table, newest first, at most [`MAX`] names.

use serde::{Deserialize, Serialize};

use crate::{
    settings::{read, write},
    store::{Store, StoreError},
};

const KEY: &str = "agentsSeen";

/// Names kept (older ones are dropped).
pub const MAX: usize = 50;

/// The client name Teitunnel's own connection check uses: never recorded.
pub const CHECK_CLIENT: &str = "teitunnel-check";

/// An agent's last visit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SeenAgent {
    /// The name its MCP client gives.
    pub name: String,
    /// Its version, when given.
    pub version: Option<String>,
    /// When it was last connected (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub last_seen_at: u64,
}

/// Records that `name` is connected at `now`.
///
/// # Errors
/// The database can't be written.
pub async fn record(
    store: &Store,
    name: &str,
    version: Option<&str>,
    now: u64,
) -> Result<(), StoreError> {
    if name == CHECK_CLIENT {
        return Ok(());
    }
    let seen = SeenAgent {
        name: name.to_owned(),
        version: version.map(ToOwned::to_owned),
        last_seen_at: now,
    };
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            let mut all: Vec<SeenAgent> = read(&tx, KEY)?.unwrap_or_default();
            all.retain(|a| a.name != seen.name);
            all.insert(0, seen);
            all.truncate(MAX);
            write(&tx, KEY, &all)?;
            tx.commit()?;
            Ok(())
        })
        .await
}

/// Every agent seen, most recent first.
///
/// # Errors
/// The database can't be read.
pub async fn list(store: &Store) -> Result<Vec<SeenAgent>, StoreError> {
    store
        .call(|conn| Ok(read(conn, KEY)?.unwrap_or_default()))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn keeps_the_latest_visit_of_each_agent() {
        let store = Store::open_in_memory().unwrap();
        record(&store, "claude-code", Some("2.1"), 10)
            .await
            .unwrap();
        record(&store, "cursor-vscode", None, 20).await.unwrap();
        record(&store, "claude-code", Some("2.2"), 30)
            .await
            .unwrap();
        record(&store, CHECK_CLIENT, None, 40).await.unwrap();
        let seen = list(&store).await.unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].name, "claude-code");
        assert_eq!(seen[0].version.as_deref(), Some("2.2"));
        assert_eq!(seen[0].last_seen_at, 30);

        for i in 0..MAX + 5 {
            record(&store, &format!("agent-{i}"), None, 100 + i as u64)
                .await
                .unwrap();
        }
        assert_eq!(list(&store).await.unwrap().len(), MAX);
    }
}
