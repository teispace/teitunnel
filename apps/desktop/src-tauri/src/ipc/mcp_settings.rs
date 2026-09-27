//! Settings ▸ AI tools: how Teitunnel's MCP server treats agents, and how MCP servers
//! shared with OAuth let clients connect. They live in `mcp.json` in the data folder
//! (`teitunnel_mcp::Settings`), which every `teitunnel mcp`, `teitunnel serve` and
//! `teitunnel share --mcp` reads when it starts; servers already running keep what they
//! started with.

use serde::{Deserialize, Serialize};
use teitunnel_core::text::msg::ai_clients as m;
use teitunnel_mcp::{Mode, OAuthSettings, Settings, config::MAX_GRANT_DAYS};

use crate::error::AppError;

/// What an agent may do through the MCP server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum McpMode {
    /// Look only.
    ReadOnly,
    /// Every change waits for the person's approval.
    Ask,
    /// Changes apply without asking (still through reviewed plans).
    Full,
}

impl From<Mode> for McpMode {
    fn from(mode: Mode) -> Self {
        match mode {
            Mode::ReadOnly => Self::ReadOnly,
            Mode::Ask => Self::Ask,
            Mode::Full => Self::Full,
        }
    }
}

impl From<McpMode> for Mode {
    fn from(mode: McpMode) -> Self {
        match mode {
            McpMode::ReadOnly => Self::ReadOnly,
            McpMode::Ask => Self::Ask,
            McpMode::Full => Self::Full,
        }
    }
}

/// The MCP server's settings, as Settings shows them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct McpSettings {
    /// The mode for AI tools connected without their own (`--mode` wins).
    pub mode: McpMode,
    /// Agents see credentials in captured traffic and unredacted logs.
    pub allow_secrets: bool,
    /// In `ask` mode, a change needs the person's answer in Teitunnel or in the AI
    /// tool's own question; the agent saying the person agreed isn't enough.
    pub approve_in_app: bool,
    /// MCP servers shared with OAuth accept clients that register themselves (older
    /// clients); clients with a published identity connect either way.
    pub dynamic_registration: bool,
    /// How many days a connection to a shared MCP server lasts before the person
    /// approves it again (1 to 365).
    pub max_grant_days: u32,
}

impl From<Settings> for McpSettings {
    fn from(settings: Settings) -> Self {
        Self {
            mode: settings.mode.into(),
            allow_secrets: settings.allow_secrets,
            approve_in_app: settings.approve_in_app,
            dynamic_registration: settings.oauth.dynamic_registration,
            max_grant_days: settings.oauth.max_grant_days,
        }
    }
}

impl From<McpSettings> for Settings {
    fn from(view: McpSettings) -> Self {
        Self {
            mode: view.mode.into(),
            allow_secrets: view.allow_secrets,
            approve_in_app: view.approve_in_app,
            oauth: OAuthSettings {
                dynamic_registration: view.dynamic_registration,
                max_grant_days: view.max_grant_days,
            },
        }
    }
}

fn read(dir: &std::path::Path) -> Result<McpSettings, AppError> {
    Settings::read(dir).map(McpSettings::from).map_err(|err| {
        tracing::warn!(error = %err, "MCP settings");
        AppError::internal(m::settings_unreadable(err))
    })
}

/// The MCP server's settings (defaults when they were never changed).
#[tauri::command]
#[specta::specta]
pub async fn mcp_settings_get(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<McpSettings, AppError> {
    let dir = state.data_dir.clone();
    super::off_main(move || read(&dir)).await?
}

/// Saves the MCP server's settings; returns them as saved. AI tools pick them up when
/// they next start Teitunnel's MCP server.
#[tauri::command]
#[specta::specta]
pub async fn mcp_settings_save(
    state: tauri::State<'_, crate::state::AppState>,
    settings: McpSettings,
) -> Result<McpSettings, AppError> {
    if !(1..=MAX_GRANT_DAYS).contains(&settings.max_grant_days) {
        return Err(AppError::invalid(
            "maxGrantDays",
            m::grant_days(u64::from(MAX_GRANT_DAYS)),
        ));
    }
    let dir = state.data_dir.clone();
    super::off_main(move || {
        Settings::from(settings).save(&dir).map_err(|err| {
            tracing::warn!(error = %err, "MCP settings");
            AppError::internal(m::settings_failed(err))
        })?;
        read(&dir)
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_the_view() {
        let settings = Settings {
            mode: Mode::ReadOnly,
            allow_secrets: false,
            approve_in_app: false,
            oauth: OAuthSettings {
                dynamic_registration: false,
                max_grant_days: 30,
            },
        };
        let view = McpSettings::from(settings.clone());
        assert_eq!(view.mode, McpMode::ReadOnly);
        assert!(!view.approve_in_app);
        assert_eq!(Settings::from(view), settings);
        let defaults = McpSettings::from(Settings::default());
        assert!(defaults.approve_in_app && defaults.dynamic_registration);
        assert_eq!(defaults.max_grant_days, 90);
    }
}
