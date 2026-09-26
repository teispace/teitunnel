//! How a server instance behaves: its permission mode, whether secrets may reach the
//! agent, whether approvals need a person, and the OAuth policy for MCP servers it shares.
//! Read from `<data>/mcp.json`, then the environment, then flags (each later one wins).
//!
//! Every process that serves MCP or shares an MCP server with OAuth (`teitunnel mcp`,
//! `teitunnel serve`, `teitunnel share --mcp`) reads this file when it starts; the app
//! reads and writes it with [`Settings::read`] and [`Settings::save`].

use std::{path::Path, str::FromStr};

use serde::{Deserialize, Serialize};

/// What an agent may do through this server.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Look only: no Cloudflare change, no share, nothing stopped. Tools that change
    /// something aren't even listed.
    ReadOnly,
    /// Every change needs the person's approval: in the Teitunnel app while it runs, or
    /// through the client (MCP elicitation) when it can ask. With
    /// [`Settings::approve_in_app`] off, a second call with `confirmed: true` after
    /// showing the plan counts too. Nothing is ever applied without one of those.
    #[default]
    Ask,
    /// Changes apply without asking (still only through reviewed plans, and records
    /// Teitunnel didn't create still need `confirmed: true`).
    Full,
}

impl Mode {
    /// The mode's name as written in flags and files.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Ask => "ask",
            Self::Full => "full",
        }
    }
}

impl FromStr for Mode {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "read-only" | "readonly" | "read_only" | "ro" => Ok(Self::ReadOnly),
            "ask" => Ok(Self::Ask),
            "full" => Ok(Self::Full),
            other => Err(format!(
                "\"{other}\" isn't a mode. Use read-only, ask or full."
            )),
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A server instance's settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Permission mode.
    pub mode: Mode,
    /// Show secrets to the agent: `Authorization`, `Cookie` and similar headers in
    /// captured traffic, and unredacted log lines. Off unless explicitly turned on.
    pub allow_secrets: bool,
    /// In `ask` mode, a change needs a person's answer: in the Teitunnel app, or in the
    /// AI client's own question (MCP elicitation). The agent's `confirmed: true` alone
    /// isn't enough, so an agent misled by something it read can't approve its own
    /// change. On by default; off, `confirmed: true` counts when nobody can be asked.
    pub approve_in_app: bool,
    /// How MCP servers shared with OAuth (`expose_mcp_server`, `teitunnel share --mcp`)
    /// let clients connect.
    pub oauth: OAuthSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: Mode::default(),
            allow_secrets: false,
            approve_in_app: true,
            oauth: OAuthSettings::default(),
        }
    }
}

/// The OAuth policy for MCP servers shared from this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct OAuthSettings {
    /// Let clients register themselves (Dynamic Client Registration: deprecated in MCP
    /// 2026-07-28, still what older clients use). Clients identified by a Client ID
    /// Metadata Document connect either way. On by default.
    pub dynamic_registration: bool,
    /// How long an approved connection lasts before the person approves it again, in
    /// days (1 to 365; default 90).
    pub max_grant_days: u32,
}

/// The longest a connection may last, in days.
pub const MAX_GRANT_DAYS: u32 = 365;

impl Default for OAuthSettings {
    fn default() -> Self {
        Self {
            dynamic_registration: true,
            max_grant_days: 90,
        }
    }
}

impl OAuthSettings {
    /// The policy for `teitunnel_core::mcp_auth`.
    pub fn policy(self) -> teitunnel_core::mcp_auth::Policy {
        teitunnel_core::mcp_auth::Policy {
            dynamic_registration: self.dynamic_registration,
            max_grant: std::time::Duration::from_secs(
                u64::from(self.max_grant_days.clamp(1, MAX_GRANT_DAYS)) * 24 * 3600,
            ),
        }
    }
}

/// The settings file's name in the data folder.
pub const FILE: &str = "mcp.json";

impl Settings {
    /// Reads `<dir>/mcp.json` (defaults if it's missing), then applies
    /// `TEITUNNEL_MCP_MODE`, `TEITUNNEL_MCP_ALLOW_SECRETS` and
    /// `TEITUNNEL_MCP_APPROVE_IN_APP` from `env`.
    ///
    /// # Errors
    /// An unreadable or invalid file, or an invalid variable, as a message.
    pub fn load(dir: &Path, env: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let path = dir.join(FILE);
        let mut settings = match std::fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes)
                .map_err(|e| format!("{} isn't valid: {e}", path.display()))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => return Err(format!("Couldn't read {}: {e}", path.display())),
        };
        if let Some(mode) = env("TEITUNNEL_MCP_MODE").filter(|m| !m.trim().is_empty()) {
            settings.mode = mode.parse()?;
        }
        if let Some(allow) = env("TEITUNNEL_MCP_ALLOW_SECRETS") {
            settings.allow_secrets = matches!(allow.trim(), "1" | "true" | "yes");
        }
        if let Some(approve) = env("TEITUNNEL_MCP_APPROVE_IN_APP") {
            settings.approve_in_app = !matches!(approve.trim(), "0" | "false" | "no");
        }
        Ok(settings)
    }

    /// Reads `<dir>/mcp.json` alone (no environment), for the app's settings.
    ///
    /// # Errors
    /// An unreadable or invalid file, as a message.
    pub fn read(dir: &Path) -> Result<Self, String> {
        Self::load(dir, |_| None)
    }

    /// Writes `<dir>/mcp.json` through a temporary file, so readers never see half of it.
    /// Servers already running keep the settings they started with.
    ///
    /// # Errors
    /// The file couldn't be written, as a message.
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let path = dir.join(FILE);
        let temporary = dir.join(format!("{FILE}.tmp"));
        let json = serde_json::to_vec_pretty(self).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(dir)
            .and_then(|()| std::fs::write(&temporary, json))
            .and_then(|()| std::fs::rename(&temporary, &path))
            .map_err(|e| format!("Couldn't save {}: {e}", path.display()))
    }

    /// Applies command-line flags (they win over the file and the environment).
    #[must_use]
    pub fn with_flags(mut self, mode: Option<Mode>, allow_secrets: bool) -> Self {
        if let Some(mode) = mode {
            self.mode = mode;
        }
        self.allow_secrets |= allow_secrets;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes() {
        assert_eq!("read-only".parse::<Mode>(), Ok(Mode::ReadOnly));
        assert_eq!(" Full ".parse::<Mode>(), Ok(Mode::Full));
        assert_eq!("ask".parse::<Mode>(), Ok(Mode::Ask));
        assert!("yolo".parse::<Mode>().is_err());
        assert_eq!(Mode::default(), Mode::Ask, "asking is the default");
    }

    #[test]
    fn file_then_environment_then_flags() {
        let dir = tempfile::tempdir().unwrap();
        let none = |_: &str| None;
        assert_eq!(
            Settings::load(dir.path(), none).unwrap(),
            Settings::default()
        );

        std::fs::write(dir.path().join(FILE), r#"{ "mode": "full" }"#).unwrap();
        let settings = Settings::load(dir.path(), none).unwrap();
        assert_eq!(settings.mode, Mode::Full);
        assert!(!settings.allow_secrets, "secrets stay hidden by default");

        let env = |name: &str| match name {
            "TEITUNNEL_MCP_MODE" => Some("read-only".to_owned()),
            "TEITUNNEL_MCP_ALLOW_SECRETS" => Some("true".to_owned()),
            _ => None,
        };
        let settings = Settings::load(dir.path(), env).unwrap();
        assert_eq!(settings.mode, Mode::ReadOnly);
        assert!(settings.allow_secrets);

        let settings = settings.with_flags(Some(Mode::Ask), false);
        assert_eq!(settings.mode, Mode::Ask);

        std::fs::write(dir.path().join(FILE), "{ nope").unwrap();
        assert!(Settings::load(dir.path(), none).is_err());
        let bad = |_: &str| Some("sometimes".to_owned());
        std::fs::remove_file(dir.path().join(FILE)).unwrap();
        assert!(Settings::load(dir.path(), bad).is_err());
    }

    #[test]
    fn approvals_need_a_person_unless_turned_off() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings::read(dir.path()).unwrap();
        assert!(settings.approve_in_app, "on by default");
        assert!(settings.oauth.dynamic_registration);
        assert_eq!(settings.oauth.max_grant_days, 90);
        // A file written before the setting existed keeps the safe default.
        std::fs::write(dir.path().join(FILE), r#"{ "mode": "ask" }"#).unwrap();
        assert!(Settings::read(dir.path()).unwrap().approve_in_app);
        let off = |name: &str| (name == "TEITUNNEL_MCP_APPROVE_IN_APP").then(|| "0".to_owned());
        assert!(!Settings::load(dir.path(), off).unwrap().approve_in_app);
    }

    #[test]
    fn saves_and_reads_back() {
        let dir = tempfile::tempdir().unwrap();
        let settings = Settings {
            approve_in_app: false,
            oauth: OAuthSettings {
                dynamic_registration: false,
                max_grant_days: 30,
            },
            ..Settings::default()
        };
        settings.save(dir.path()).unwrap();
        let text = std::fs::read_to_string(dir.path().join(FILE)).unwrap();
        assert!(text.contains("\"approveInApp\": false"), "{text}");
        assert!(text.contains("\"dynamicRegistration\": false"), "{text}");
        assert_eq!(Settings::read(dir.path()).unwrap(), settings);
        let policy = OAuthSettings {
            max_grant_days: 5000,
            ..OAuthSettings::default()
        }
        .policy();
        assert_eq!(
            policy.max_grant,
            std::time::Duration::from_secs(365 * 24 * 3600)
        );
    }
}
