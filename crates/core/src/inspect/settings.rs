//! The inspector's preferences, kept as one JSON value (`inspector`) in the `settings`
//! table so they sit apart from the app's other settings.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::store::{Store, StoreError};

const KEY: &str = "inspector";

/// Longest history kept, in hours.
pub const MAX_RETENTION_HOURS: u32 = 7 * 24;

/// How the inspector behaves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", default)]
pub struct InspectorSettings {
    /// Quick Shares go through the inspector (maintainer decision Q1: on by default).
    pub inspect_quick_shares: bool,
    /// Keep recent captures on disk, credentials masked, so a restart keeps them.
    pub keep_history: bool,
    /// Hours of history kept (1–168).
    pub retention_hours: u32,
    /// Stop shares after this many minutes without a request (`None`: never).
    pub idle_stop_minutes: Option<u32>,
    /// Paths that notify when requested, on every share and route (e.g.
    /// `/webhooks/*`).
    pub watched_paths: Vec<String>,
}

impl Default for InspectorSettings {
    fn default() -> Self {
        Self {
            inspect_quick_shares: true,
            keep_history: true,
            retention_hours: 24,
            idle_stop_minutes: None,
            watched_paths: Vec::new(),
        }
    }
}

/// A partial update: only the fields that are set change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InspectorSettingsPatch {
    /// Inspect new Quick Shares.
    #[serde(default)]
    pub inspect_quick_shares: Option<bool>,
    /// Keep history on disk.
    #[serde(default)]
    pub keep_history: Option<bool>,
    /// Hours of history (clamped to 1–168).
    #[serde(default)]
    pub retention_hours: Option<u32>,
    /// Minutes without a request before a share stops; 0 turns it off.
    #[serde(default)]
    pub idle_stop_minutes: Option<u32>,
    /// Watched paths (replacing the list).
    #[serde(default)]
    pub watched_paths: Option<Vec<String>>,
}

impl InspectorSettings {
    pub(crate) fn apply(&mut self, patch: InspectorSettingsPatch) {
        if let Some(on) = patch.inspect_quick_shares {
            self.inspect_quick_shares = on;
        }
        if let Some(on) = patch.keep_history {
            self.keep_history = on;
        }
        if let Some(hours) = patch.retention_hours {
            self.retention_hours = hours.clamp(1, MAX_RETENTION_HOURS);
        }
        if let Some(minutes) = patch.idle_stop_minutes {
            self.idle_stop_minutes = (minutes > 0).then_some(minutes.min(7 * 24 * 60));
        }
        if let Some(paths) = patch.watched_paths {
            self.watched_paths = paths
                .into_iter()
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect();
        }
    }
}

/// Loads the settings (defaults for anything missing or unreadable).
///
/// # Errors
/// The database can't be read.
pub(crate) async fn load(store: &Store) -> Result<InspectorSettings, StoreError> {
    store
        .call(|conn| {
            let raw: Option<String> = conn
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![KEY],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(raw
                .and_then(|raw| {
                    serde_json::from_str::<InspectorSettings>(&raw)
                        .inspect_err(|err| tracing::warn!(%err, "unreadable inspector settings"))
                        .ok()
                })
                .unwrap_or_default())
        })
        .await
}

/// Applies `patch` and returns the result.
///
/// # Errors
/// The database can't be written.
pub(crate) async fn update(
    store: &Store,
    patch: InspectorSettingsPatch,
) -> Result<InspectorSettings, StoreError> {
    let mut settings = load(store).await?;
    settings.apply(patch);
    let saved = settings.clone();
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT (key) DO UPDATE SET value = ?2",
                params![KEY, serde_json::to_string(&saved)?],
            )?;
            Ok(())
        })
        .await?;
    Ok(settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn defaults_then_patches() {
        let store = Store::open_in_memory().unwrap();
        let settings = load(&store).await.unwrap();
        assert!(settings.inspect_quick_shares && settings.keep_history);
        assert_eq!(settings.retention_hours, 24);
        let updated = update(
            &store,
            InspectorSettingsPatch {
                inspect_quick_shares: Some(false),
                retention_hours: Some(10_000),
                idle_stop_minutes: Some(15),
                watched_paths: Some(vec![" /webhooks/* ".into(), String::new()]),
                ..InspectorSettingsPatch::default()
            },
        )
        .await
        .unwrap();
        assert!(!updated.inspect_quick_shares);
        assert_eq!(updated.retention_hours, MAX_RETENTION_HOURS);
        assert_eq!(updated.idle_stop_minutes, Some(15));
        assert_eq!(updated.watched_paths, ["/webhooks/*"]);
        assert_eq!(load(&store).await.unwrap(), updated);
        let off = update(
            &store,
            InspectorSettingsPatch {
                idle_stop_minutes: Some(0),
                ..InspectorSettingsPatch::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(off.idle_stop_minutes, None);
    }
}
