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

/// Hours when alerts are recorded but don't notify, in minutes after local midnight.
/// `from` after `to` spans midnight (22:00–07:00).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct QuietHours {
    /// Whether quiet hours apply.
    pub enabled: bool,
    /// Start, minutes after midnight.
    pub from: u16,
    /// End, minutes after midnight.
    pub to: u16,
}

impl Default for QuietHours {
    fn default() -> Self {
        Self {
            enabled: false,
            from: 22 * 60,
            to: 7 * 60,
        }
    }
}

impl QuietHours {
    /// Whether `minute` (after local midnight) is quiet.
    pub fn contains(&self, minute: u16) -> bool {
        crate::alerts::is_quiet(self.enabled, self.from, self.to, minute)
    }
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
    /// Notify when the Doctor finds a new error.
    pub notify_doctor: bool,
    /// Notify about alerts (routes down or back, errors, slowness).
    pub notify_alerts: bool,
    /// When alerts and connector notices stay quiet.
    pub quiet_hours: QuietHours,
    /// Check for app updates by itself (at launch and daily).
    pub check_for_updates: bool,
    /// The one-time "Install teitunnel?" offer was answered (Install or Not now), on
    /// installs where the CLI isn't put on the PATH by the installer (D-090).
    pub cli_offer_dismissed: bool,
    /// Doctor issues the user chose to ignore (stable issue ids). Changed with
    /// [`set_ignored`], not through a patch, so concurrent toggles can't lose one.
    pub ignored_issues: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            theme: Theme::System,
            show_in_menu_bar: true,
            notify_connectors: true,
            notify_quick_shares: true,
            notify_doctor: true,
            notify_alerts: true,
            quiet_hours: QuietHours::default(),
            check_for_updates: true,
            cli_offer_dismissed: false,
            ignored_issues: Vec::new(),
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
    /// Doctor notifications on or off.
    #[serde(default)]
    pub notify_doctor: Option<bool>,
    /// Alert notifications on or off.
    #[serde(default)]
    pub notify_alerts: Option<bool>,
    /// New quiet hours.
    #[serde(default)]
    pub quiet_hours: Option<QuietHours>,
    /// Automatic update checks on or off.
    #[serde(default)]
    pub check_for_updates: Option<bool>,
    /// The command line offer answered.
    #[serde(default)]
    pub cli_offer_dismissed: Option<bool>,
}

const THEME: &str = "theme";
const SHOW_IN_MENU_BAR: &str = "showInMenuBar";
const NOTIFY_CONNECTORS: &str = "notifyConnectors";
const NOTIFY_QUICK_SHARES: &str = "notifyQuickShares";
const NOTIFY_DOCTOR: &str = "notifyDoctor";
const NOTIFY_ALERTS: &str = "notifyAlerts";
const QUIET_HOURS: &str = "quietHours";
const IGNORED_ISSUES: &str = "ignoredIssues";
const CHECK_FOR_UPDATES: &str = "checkForUpdates";
const CLI_OFFER_DISMISSED: &str = "cliOfferDismissed";

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
                notify_doctor: read(conn, NOTIFY_DOCTOR)?.unwrap_or(defaults.notify_doctor),
                notify_alerts: read(conn, NOTIFY_ALERTS)?.unwrap_or(defaults.notify_alerts),
                quiet_hours: read::<QuietHours>(conn, QUIET_HOURS)?
                    .filter(|q| q.from < 24 * 60 && q.to < 24 * 60)
                    .unwrap_or(defaults.quiet_hours),
                check_for_updates: read(conn, CHECK_FOR_UPDATES)?
                    .unwrap_or(defaults.check_for_updates),
                cli_offer_dismissed: read(conn, CLI_OFFER_DISMISSED)?
                    .unwrap_or(defaults.cli_offer_dismissed),
                ignored_issues: read(conn, IGNORED_ISSUES)?.unwrap_or(defaults.ignored_issues),
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
            if let Some(on) = patch.notify_doctor {
                write(&tx, NOTIFY_DOCTOR, &on)?;
            }
            if let Some(on) = patch.notify_alerts {
                write(&tx, NOTIFY_ALERTS, &on)?;
            }
            if let Some(quiet) = patch.quiet_hours {
                let quiet = QuietHours {
                    from: quiet.from.min(24 * 60 - 1),
                    to: quiet.to.min(24 * 60 - 1),
                    ..quiet
                };
                write(&tx, QUIET_HOURS, &quiet)?;
            }
            if let Some(on) = patch.check_for_updates {
                write(&tx, CHECK_FOR_UPDATES, &on)?;
            }
            if let Some(done) = patch.cli_offer_dismissed {
                write(&tx, CLI_OFFER_DISMISSED, &done)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    load(store).await
}

/// Ignores (or stops ignoring) Doctor issues, atomically, and returns the settings.
///
/// # Errors
/// Fails if the database can't be written.
pub async fn set_ignored(
    store: &Store,
    ids: Vec<String>,
    ignored: bool,
) -> Result<Settings, StoreError> {
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            let mut current: Vec<String> = read(&tx, IGNORED_ISSUES)?.unwrap_or_default();
            if ignored {
                for id in ids {
                    if !current.contains(&id) {
                        current.push(id);
                    }
                }
            } else {
                current.retain(|id| !ids.contains(id));
            }
            write(&tx, IGNORED_ISSUES, &current)?;
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
    async fn ignores_and_restores_issues() {
        let store = Store::open_in_memory().unwrap();
        let ids = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        set_ignored(&store, ids(&["a", "b"]), true).await.unwrap();
        let after = set_ignored(&store, ids(&["b", "c"]), true).await.unwrap();
        assert_eq!(after.ignored_issues, ["a", "b", "c"]);
        let after = set_ignored(&store, ids(&["a", "c"]), false).await.unwrap();
        assert_eq!(after.ignored_issues, ["b"]);
    }

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

    #[tokio::test]
    async fn remembers_the_answered_cli_offer() {
        let store = Store::open_in_memory().unwrap();
        assert!(!load(&store).await.unwrap().cli_offer_dismissed);
        let patch: SettingsPatch = serde_json::from_str(r#"{"cliOfferDismissed":true}"#).unwrap();
        let after = update(&store, patch).await.unwrap();
        assert!(after.cli_offer_dismissed);
        assert!(after.show_in_menu_bar, "other settings keep their values");
        assert!(load(&store).await.unwrap().cli_offer_dismissed);
    }

    #[tokio::test]
    async fn stores_alert_notifications_and_quiet_hours() {
        let store = Store::open_in_memory().unwrap();
        let before = load(&store).await.unwrap();
        assert!(before.notify_alerts);
        assert!(!before.quiet_hours.enabled);
        let patch: SettingsPatch = serde_json::from_str(
            r#"{"notifyAlerts":false,"quietHours":{"enabled":true,"from":1380,"to":9999}}"#,
        )
        .unwrap();
        let after = update(&store, patch).await.unwrap();
        assert!(!after.notify_alerts);
        assert_eq!(
            after.quiet_hours,
            QuietHours {
                enabled: true,
                from: 23 * 60,
                to: 24 * 60 - 1
            }
        );
        assert!(after.quiet_hours.contains(23 * 60 + 30));
        assert!(!after.quiet_hours.contains(12 * 60));
    }

    #[test]
    fn patch_accepts_partial_json() {
        let patch: SettingsPatch = serde_json::from_str(r#"{"theme":"light"}"#).unwrap();
        assert_eq!(patch.theme, Some(Theme::Light));
        assert_eq!(patch.show_in_menu_bar, None);
    }
}
