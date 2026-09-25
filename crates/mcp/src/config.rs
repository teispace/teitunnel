//! How a server instance behaves: its permission mode and whether secrets may reach the
//! agent. Read from `<data>/mcp.json`, then the environment, then flags (each later one
//! wins).

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
    /// Every change needs the person's approval: through the client (MCP elicitation)
    /// when it can ask, otherwise by calling again with `confirmed: true` after showing
    /// the plan. Nothing is ever applied without one of those.
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
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Permission mode.
    pub mode: Mode,
    /// Show secrets to the agent: `Authorization`, `Cookie` and similar headers in
    /// captured traffic, and unredacted log lines. Off unless explicitly turned on.
    pub allow_secrets: bool,
}

/// The settings file's name in the data folder.
pub const FILE: &str = "mcp.json";

impl Settings {
    /// Reads `<dir>/mcp.json` (defaults if it's missing), then applies
    /// `TEITUNNEL_MCP_MODE` and `TEITUNNEL_MCP_ALLOW_SECRETS` from `env`.
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
        Ok(settings)
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
}
