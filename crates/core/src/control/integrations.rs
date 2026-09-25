//! Settings ▸ Integrations: whether the control connection and `teitunnel://` links are
//! on, which programs the person always allows to make changes, and the global shortcut.
//! Stored in the `settings` table under their own keys (no migration).

use serde::{Deserialize, Serialize};
use teitunnel_control::protocol::ClientInfo;

use crate::{
    settings::{read, write},
    store::{Store, StoreError},
    text::{Text, UserText, english_display, msg::error::shortcut as m},
};

const CONTROL_ENABLED: &str = "controlEnabled";
const DEEP_LINKS_ENABLED: &str = "deepLinksEnabled";
const CONTROL_CLIENTS: &str = "controlClients";
const GLOBAL_SHORTCUT: &str = "globalShortcut";

/// The most programs remembered (the oldest are forgotten first).
const MAX_CLIENTS: usize = 50;

/// The shortcut offered until the person picks another (off until they turn it on).
pub const DEFAULT_SHORTCUT: &str = "CommandOrControl+Alt+Shift+S";

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

/// What the global shortcut does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ShortcutAction {
    /// Share the running dev server (copy its address if it's shared already); with
    /// none or several, open the Quick Share sheet to choose.
    #[default]
    ShareDevServer,
    /// Always open the Quick Share sheet.
    OpenQuickShare,
}

/// A system-wide shortcut, off by default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct GlobalShortcut {
    /// Registered with the system.
    pub enabled: bool,
    /// The keys, e.g. `CommandOrControl+Alt+Shift+S` (Tauri's accelerator syntax).
    pub keys: String,
    /// What it does.
    pub action: ShortcutAction,
}

impl Default for GlobalShortcut {
    fn default() -> Self {
        Self {
            enabled: false,
            keys: DEFAULT_SHORTCUT.into(),
            action: ShortcutAction::default(),
        }
    }
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
    /// The system-wide shortcut.
    pub shortcut: GlobalShortcut,
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
    /// A new global shortcut (checked and normalized before it's saved).
    #[serde(default)]
    pub shortcut: Option<GlobalShortcut>,
}

/// Why a shortcut can't be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShortcutError {
    /// Not a combination of modifiers and one key.
    Invalid(String),
    /// Only Shift (or no modifier): it would swallow ordinary typing.
    NeedsModifier,
    /// Another app has it.
    Taken,
    /// The system doesn't offer global shortcuts to apps (e.g. Wayland).
    Unsupported,
}

impl UserText for ShortcutError {
    fn text(&self) -> Text {
        match self {
            Self::Invalid(keys) => m::invalid(keys),
            Self::NeedsModifier => m::needs_modifier(),
            Self::Taken => m::taken(),
            Self::Unsupported => m::unsupported(),
        }
    }
}

english_display!(ShortcutError);

/// Why the Integrations settings couldn't change.
#[derive(Debug, thiserror::Error)]
pub enum IntegrationsError {
    /// The shortcut was refused (nothing changed).
    #[error("{0}")]
    Shortcut(#[from] ShortcutError),
    /// The database failed.
    #[error("{0}")]
    Store(#[from] StoreError),
}

/// Modifier spellings Tauri accepts, and the one the app writes.
const MODIFIERS: &[(&str, &str)] = &[
    ("commandorcontrol", "CommandOrControl"),
    ("cmdorctrl", "CommandOrControl"),
    ("commandorctrl", "CommandOrControl"),
    ("cmdorcontrol", "CommandOrControl"),
    ("command", "Command"),
    ("cmd", "Command"),
    ("super", "Super"),
    ("meta", "Super"),
    ("control", "Control"),
    ("ctrl", "Control"),
    ("alt", "Alt"),
    ("option", "Alt"),
    ("altgr", "AltGr"),
    ("shift", "Shift"),
];

/// The order modifiers are written in.
const MODIFIER_ORDER: [&str; 7] = [
    "CommandOrControl",
    "Command",
    "Super",
    "Control",
    "Alt",
    "AltGr",
    "Shift",
];

const NAMED_KEYS: &[&str] = &[
    "Space",
    "Enter",
    "Tab",
    "Backspace",
    "Delete",
    "Escape",
    "Home",
    "End",
    "PageUp",
    "PageDown",
    "Up",
    "Down",
    "Left",
    "Right",
    "Insert",
];

const PUNCTUATION: &[char] = &['-', '=', '[', ']', '\\', ';', '\'', ',', '.', '/', '`'];

/// Checks a shortcut and writes it the one way the app registers it: modifiers in a fixed
/// order, then one key (a letter, digit, F1–F24, a named key or punctuation).
///
/// # Errors
/// [`ShortcutError::Invalid`] or [`ShortcutError::NeedsModifier`].
pub fn normalize_shortcut(keys: &str) -> Result<String, ShortcutError> {
    let invalid = || ShortcutError::Invalid(keys.trim().chars().take(64).collect());
    let parts: Vec<&str> = keys.split('+').map(str::trim).collect();
    let (key, modifiers) = parts.split_last().ok_or_else(invalid)?;
    let mut found: Vec<&str> = Vec::new();
    for part in modifiers {
        let lower = part.to_ascii_lowercase();
        let name = MODIFIERS
            .iter()
            .find(|(alias, _)| *alias == lower)
            .map(|(_, name)| *name)
            .ok_or_else(invalid)?;
        if found.contains(&name) {
            return Err(invalid());
        }
        found.push(name);
    }
    let key = normalize_key(key).ok_or_else(invalid)?;
    if found.iter().all(|m| *m == "Shift") {
        return Err(ShortcutError::NeedsModifier);
    }
    let mut out: Vec<&str> = MODIFIER_ORDER
        .into_iter()
        .filter(|m| found.contains(m))
        .collect();
    out.push(&key);
    Ok(out.join("+"))
}

fn normalize_key(key: &str) -> Option<String> {
    let mut chars = key.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_alphanumeric() {
            return Some(c.to_ascii_uppercase().to_string());
        }
        return PUNCTUATION.contains(&c).then(|| c.to_string());
    }
    if let Some(n) = key
        .strip_prefix(['F', 'f'])
        .and_then(|n| n.parse::<u8>().ok())
        && (1..=24).contains(&n)
    {
        return Some(format!("F{n}"));
    }
    NAMED_KEYS
        .iter()
        .find(|k| k.eq_ignore_ascii_case(key))
        .map(|k| (*k).to_owned())
}

/// Loads the settings (both switches default to on, the shortcut to off).
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
                shortcut: read(conn, GLOBAL_SHORTCUT)?.unwrap_or_default(),
            })
        })
        .await
}

/// Changes the switches and the shortcut.
///
/// # Errors
/// The shortcut isn't usable (nothing changes), or the database can't be written.
pub async fn update(
    store: &Store,
    mut patch: IntegrationsPatch,
) -> Result<Integrations, IntegrationsError> {
    if let Some(shortcut) = &mut patch.shortcut {
        shortcut.keys = normalize_shortcut(&shortcut.keys)?;
    }
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            if let Some(on) = patch.control_enabled {
                write(&tx, CONTROL_ENABLED, &on)?;
            }
            if let Some(on) = patch.deep_links_enabled {
                write(&tx, DEEP_LINKS_ENABLED, &on)?;
            }
            if let Some(shortcut) = &patch.shortcut {
                write(&tx, GLOBAL_SHORTCUT, shortcut)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await?;
    Ok(load(store).await?)
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

    #[test]
    fn normalizes_shortcuts() {
        assert_eq!(
            normalize_shortcut("shift+alt+cmdorctrl+s").unwrap(),
            "CommandOrControl+Alt+Shift+S"
        );
        assert_eq!(
            normalize_shortcut("Ctrl + Option + space").unwrap(),
            "Control+Alt+Space"
        );
        assert_eq!(normalize_shortcut("Super+f12").unwrap(), "Super+F12");
        assert_eq!(normalize_shortcut("Alt+/").unwrap(), "Alt+/");
        assert_eq!(
            normalize_shortcut("Shift+A"),
            Err(ShortcutError::NeedsModifier)
        );
        assert_eq!(normalize_shortcut("A"), Err(ShortcutError::NeedsModifier));
        for bad in [
            "",
            "Ctrl+",
            "Ctrl+Ctrl+A",
            "Hyper+A",
            "Ctrl+F25",
            "Ctrl+Alt",
            "Ctrl+é",
        ] {
            assert!(
                matches!(normalize_shortcut(bad), Err(ShortcutError::Invalid(_))),
                "{bad}"
            );
        }
    }

    #[tokio::test]
    async fn saves_a_checked_shortcut_off_by_default() {
        let store = Store::open_in_memory().unwrap();
        let settings = load(&store).await.unwrap();
        assert_eq!(settings.shortcut, GlobalShortcut::default());
        assert!(!settings.shortcut.enabled);
        let saved = update(
            &store,
            IntegrationsPatch {
                shortcut: Some(GlobalShortcut {
                    enabled: true,
                    keys: "ctrl+shift+space".into(),
                    action: ShortcutAction::OpenQuickShare,
                }),
                ..IntegrationsPatch::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(saved.shortcut.keys, "Control+Shift+Space");
        assert!(saved.control_enabled, "other settings unchanged");
        let refused = update(
            &store,
            IntegrationsPatch {
                shortcut: Some(GlobalShortcut {
                    enabled: true,
                    keys: "Shift+K".into(),
                    action: ShortcutAction::ShareDevServer,
                }),
                ..IntegrationsPatch::default()
            },
        )
        .await;
        assert!(matches!(
            refused,
            Err(IntegrationsError::Shortcut(ShortcutError::NeedsModifier))
        ));
        assert_eq!(
            load(&store).await.unwrap().shortcut.keys,
            "Control+Shift+Space"
        );
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
