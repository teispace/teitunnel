//! App preferences, stored as one JSON value per key in the `settings` table.
//!
//! Reading is forgiving: a missing or unreadable key falls back to its default (and is
//! logged), so a bad value can never stop the app from starting.

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::store::{Store, StoreError};

/// Appearance override.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follow the system appearance.
    #[default]
    System,
    /// Always light.
    Light,
    /// Always dark.
    Dark,
}

/// All preferences, with defaults applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Appearance override.
    pub theme: Theme,
    /// Show the Teitunnel icon in the menu bar.
    pub show_in_menu_bar: bool,
    /// Notify when this Mac's connector goes down, comes back or crash-loops.
    pub notify_connectors: bool,
    /// Notify when a Quick Share goes live or fails.
    pub notify_quick_shares: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            show_in_menu_bar: true,
            notify_connectors: true,
            notify_quick_shares: true,
        }
    }
}

/// A partial update: only the fields that are set change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SettingsPatch {
    /// New appearance override.
    #[serde(default)]
    pub theme: Option<Theme>,
    /// New menu bar visibility.
    #[serde(default)]
    pub show_in_menu_bar: Option<bool>,
    /// Connector notifications on or off.
    #[serde(default)]
    pub notify_connectors: Option<bool>,
    /// Quick Share notifications on or off.
    #[serde(default)]
    pub notify_quick_shares: Option<bool>,
}

const THEME: &str = "theme";
const SHOW_IN_MENU_BAR: &str = "showInMenuBar";
const NOTIFY_CONNECTORS: &str = "notifyConnectors";
const NOTIFY_QUICK_SHARES: &str = "notifyQuickShares";

/// Loads all settings.
///
/// # Errors
/// Fails only if the database can't be read; invalid values fall back to defaults.
pub async fn load(store: &Store) -> Result<Settings, StoreError> {
    store
        .call(|conn| {
            let defaults = Settings::default();
            Ok(Settings {
                theme: read(conn, THEME)?.unwrap_or(defaults.theme),
                show_in_menu_bar: read(conn, SHOW_IN_MENU_BAR)?
                    .unwrap_or(defaults.show_in_menu_bar),
                notify_connectors: read(conn, NOTIFY_CONNECTORS)?
                    .unwrap_or(defaults.notify_connectors),
                notify_quick_shares: read(conn, NOTIFY_QUICK_SHARES)?
                    .unwrap_or(defaults.notify_quick_shares),
            })
        })
        .await
}

/// Applies `patch` atomically and returns the resulting settings.
///
/// # Errors
/// Fails if the database can't be written.
pub async fn update(store: &Store, patch: SettingsPatch) -> Result<Settings, StoreError> {
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            if let Some(theme) = patch.theme {
                write(&tx, THEME, &theme)?;
            }
            if let Some(show) = patch.show_in_menu_bar {
                write(&tx, SHOW_IN_MENU_BAR, &show)?;
            }
            if let Some(on) = patch.notify_connectors {
                write(&tx, NOTIFY_CONNECTORS, &on)?;
            }
            if let Some(on) = patch.notify_quick_shares {
                write(&tx, NOTIFY_QUICK_SHARES, &on)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    load(store).await
}

fn read<T: DeserializeOwned>(
    conn: &rusqlite::Connection,
    key: &str,
) -> Result<Option<T>, StoreError> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            params![key],
            |row| row.get(0),
        )
        .optional()?;
    Ok(raw.and_then(|raw| match serde_json::from_str(&raw) {
        Ok(value) => Some(value),
        Err(err) => {
            tracing::warn!(key, error = %err, "ignoring invalid setting");
            None
        }
    }))
}

fn write<T: Serialize>(
    conn: &rusqlite::Connection,
    key: &str,
    value: &T,
) -> Result<(), StoreError> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, serde_json::to_string(value)?],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn defaults_when_empty() {
        let store = Store::open_in_memory().unwrap();
        assert_eq!(load(&store).await.unwrap(), Settings::default());
    }

    #[tokio::test]
    async fn patches_only_given_fields() {
        let store = Store::open_in_memory().unwrap();
        let after = update(
            &store,
            SettingsPatch {
                theme: Some(Theme::Dark),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            after,
            Settings {
                theme: Theme::Dark,
                ..Settings::default()
            }
        );

        let after = update(
            &store,
            SettingsPatch {
                show_in_menu_bar: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(
            after,
            Settings {
                theme: Theme::Dark,
                show_in_menu_bar: false,
                ..Settings::default()
            }
        );

        let after = update(
            &store,
            SettingsPatch {
                notify_connectors: Some(false),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(!after.notify_connectors);
        assert!(after.notify_quick_shares);
    }

    #[tokio::test]
    async fn invalid_values_fall_back_to_defaults() {
        let store = Store::open_in_memory().unwrap();
        store
            .call(|conn| {
                Ok(conn.execute(
                    "INSERT INTO settings (key, value) VALUES ('theme', '\"purple\"')",
                    [],
                )?)
            })
            .await
            .unwrap();
        assert_eq!(load(&store).await.unwrap().theme, Theme::System);
    }

    #[test]
    fn patch_accepts_partial_json() {
        let patch: SettingsPatch = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(patch.theme, Some(Theme::Light));
        assert_eq!(patch.show_in_menu_bar, None);
    }
}
