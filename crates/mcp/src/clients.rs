//! Connecting AI clients: writes (and removes) Teitunnel's entry in each client's MCP
//! configuration, for `teitunnel mcp install|uninstall|config|status` and the app's
//! "Connect an AI tool".
//!
//! Every write is a merge: only the `teitunnel` entry changes; other servers, settings
//! and comments (JSONC files such as VS Code's and Zed's) stay as they are. Nothing is
//! written when the entry is already right; otherwise the file is backed up first
//! (`<file>.teitunnel-backup`) and replaced atomically. A file that can't be parsed is
//! never touched. Locations and formats were checked against each client's
//! documentation on 2026-09-24.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use jsonc_parser::{
    ParseOptions,
    cst::{CstInputValue, CstObject, CstRootNode},
};
use serde::Serialize;
use serde_json::Value;

/// The entry's name in every client.
pub const SERVER_NAME: &str = "teitunnel";

/// An AI client Teitunnel can connect to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Client {
    /// Anthropic's Claude Code (user scope, `~/.claude.json`).
    ClaudeCode,
    /// Claude Desktop.
    ClaudeDesktop,
    /// Cursor (global `~/.cursor/mcp.json`).
    Cursor,
    /// Visual Studio Code with GitHub Copilot (user `mcp.json`).
    Vscode,
    /// OpenAI Codex CLI and IDE extension (`~/.codex/config.toml`).
    Codex,
    /// Windsurf (`~/.codeium/windsurf/mcp_config.json`).
    Windsurf,
    /// Zed (`settings.json`, `context_servers`).
    Zed,
    /// Gemini CLI (`~/.gemini/settings.json`).
    GeminiCli,
}

/// Why a configuration couldn't be written.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The file isn't valid JSON/JSONC/TOML: it's left alone.
    #[error(
        "{path} isn't valid {format} ({reason}), so Teitunnel won't change it. Fix it, or add the entry by hand (`teitunnel mcp config {client}`)."
    )]
    Unparseable {
        /// The file.
        path: String,
        /// JSON or TOML.
        format: &'static str,
        /// The parser's message.
        reason: String,
        /// The client's id.
        client: &'static str,
    },
    /// The file's top level isn't a table/object.
    #[error("{0} doesn't hold a settings object at its top level, so Teitunnel won't change it.")]
    Shape(String),
    /// Reading or writing failed.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// The error.
        source: std::io::Error,
    },
    /// The home folder couldn't be found.
    #[error("Couldn't find your home folder.")]
    NoHome,
}

/// Where configuration lives on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The home folder.
    pub home: PathBuf,
    /// Per-user application data: `~/Library/Application Support` (macOS), `%APPDATA%`
    /// (Windows), `$XDG_CONFIG_HOME` or `~/.config` (Linux).
    pub app_data: PathBuf,
    /// `$XDG_CONFIG_HOME` or `~/.config` (Zed on macOS and Linux).
    pub xdg_config: PathBuf,
    /// Which platform's layout to use.
    pub os: Os,
}

/// Platforms with their own layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
    /// Linux and other Unixes.
    Linux,
}

impl Paths {
    /// This machine's.
    ///
    /// # Errors
    /// No home folder.
    pub fn detect() -> Result<Self, ClientError> {
        let home = std::env::home_dir().ok_or(ClientError::NoHome)?;
        let xdg_config = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        let os = if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        };
        let app_data = match os {
            Os::MacOs => home.join("Library").join("Application Support"),
            Os::Windows => std::env::var_os("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join("AppData").join("Roaming")),
            Os::Linux => xdg_config.clone(),
        };
        Ok(Self {
            home,
            app_data,
            xdg_config,
            os,
        })
    }

    /// A layout rooted at `home` (tests).
    pub fn under(home: &Path, os: Os) -> Self {
        let xdg_config = home.join(".config");
        let app_data = match os {
            Os::MacOs => home.join("Library").join("Application Support"),
            Os::Windows => home.join("AppData").join("Roaming"),
            Os::Linux => xdg_config.clone(),
        };
        Self {
            home: home.to_path_buf(),
            app_data,
            xdg_config,
            os,
        }
    }
}

/// How to start the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ServerCommand {
    /// The executable (an absolute path).
    pub command: String,
    /// Its arguments, e.g. `["mcp"]` or `["mcp", "--mode", "read-only"]`.
    pub args: Vec<String>,
    /// Environment variables (never secrets).
    pub env: BTreeMap<String, String>,
}

enum Format {
    /// A JSON (or JSONC) object at `container` (nested keys), entries with or without a
    /// `"type": "stdio"` field.
    Json {
        container: &'static [&'static str],
        typed: bool,
    },
    /// `[mcp_servers.<name>]` in TOML.
    CodexToml,
}

impl Client {
    /// Every client.
    pub const ALL: [Self; 8] = [
        Self::ClaudeCode,
        Self::ClaudeDesktop,
        Self::Cursor,
        Self::Vscode,
        Self::Codex,
        Self::Windsurf,
        Self::Zed,
        Self::GeminiCli,
    ];

    /// Its id in commands, e.g. `claude-code`.
    pub fn id(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::ClaudeDesktop => "claude-desktop",
            Self::Cursor => "cursor",
            Self::Vscode => "vscode",
            Self::Codex => "codex",
            Self::Windsurf => "windsurf",
            Self::Zed => "zed",
            Self::GeminiCli => "gemini-cli",
        }
    }

    /// Its name, e.g. `Claude Code`.
    pub fn name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "Claude Code",
            Self::ClaudeDesktop => "Claude Desktop",
            Self::Cursor => "Cursor",
            Self::Vscode => "VS Code",
            Self::Codex => "Codex",
            Self::Windsurf => "Windsurf",
            Self::Zed => "Zed",
            Self::GeminiCli => "Gemini CLI",
        }
    }

    /// The client with this id (or a close spelling).
    pub fn parse(value: &str) -> Option<Self> {
        let value = value.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        Self::ALL
            .into_iter()
            .find(|c| c.id() == value)
            .or(match value.as_str() {
                "claude" => Some(Self::ClaudeCode),
                "code" | "vs-code" | "copilot" => Some(Self::Vscode),
                "gemini" => Some(Self::GeminiCli),
                _ => None,
            })
    }

    fn format(self) -> Format {
        match self {
            Self::ClaudeCode | Self::Cursor => Format::Json {
                container: &["mcpServers"],
                typed: true,
            },
            Self::Vscode => Format::Json {
                container: &["servers"],
                typed: true,
            },
            Self::ClaudeDesktop | Self::Windsurf | Self::GeminiCli => Format::Json {
                container: &["mcpServers"],
                typed: false,
            },
            Self::Zed => Format::Json {
                container: &["context_servers"],
                typed: false,
            },
            Self::Codex => Format::CodexToml,
        }
    }

    /// Its configuration file.
    pub fn config_path(self, paths: &Paths) -> PathBuf {
        let home = &paths.home;
        match self {
            Self::ClaudeCode => home.join(".claude.json"),
            Self::ClaudeDesktop => paths
                .app_data
                .join("Claude")
                .join("claude_desktop_config.json"),
            Self::Cursor => home.join(".cursor").join("mcp.json"),
            Self::Vscode => paths.app_data.join("Code").join("User").join("mcp.json"),
            Self::Codex => std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".codex"))
                .join("config.toml"),
            Self::Windsurf => home
                .join(".codeium")
                .join("windsurf")
                .join("mcp_config.json"),
            Self::Zed => match paths.os {
                Os::Windows => paths.app_data.join("Zed").join("settings.json"),
                Os::MacOs | Os::Linux => paths.xdg_config.join("zed").join("settings.json"),
            },
            Self::GeminiCli => home.join(".gemini").join("settings.json"),
        }
    }

    /// Whether the client seems installed: its configuration file or folder exists.
    pub fn detected(self, paths: &Paths) -> bool {
        let path = self.config_path(paths);
        path.exists()
            || match self {
                Self::ClaudeCode => paths.home.join(".claude").is_dir(),
                _ => path.parent().is_some_and(Path::is_dir),
            }
    }

    /// The entry, as this client wants it.
    fn entry(self, command: &ServerCommand) -> Value {
        let mut entry = serde_json::Map::new();
        if let Format::Json { typed: true, .. } = self.format() {
            entry.insert("type".into(), "stdio".into());
        }
        entry.insert("command".into(), command.command.clone().into());
        entry.insert("args".into(), command.args.clone().into());
        if !command.env.is_empty() || matches!(self, Self::Zed) {
            entry.insert(
                "env".into(),
                Value::Object(
                    command
                        .env
                        .iter()
                        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                        .collect(),
                ),
            );
        }
        Value::Object(entry)
    }

    /// The text to add by hand: the entry in the file's format, inside its container.
    pub fn snippet(self, command: &ServerCommand) -> String {
        match self.format() {
            Format::Json { container, .. } => {
                let mut value = serde_json::json!({ SERVER_NAME: self.entry(command) });
                for key in container.iter().rev() {
                    value = serde_json::json!({ *key: value });
                }
                serde_json::to_string_pretty(&value).unwrap_or_default()
            }
            Format::CodexToml => {
                let mut doc = toml_edit::DocumentMut::new();
                set_codex(&mut doc, command);
                doc.to_string()
            }
        }
    }
}

/// What installing did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    /// The client.
    pub client: Client,
    /// The file.
    pub path: PathBuf,
    /// The file changed (false: it was already so).
    pub changed: bool,
    /// Where the previous version was saved, when there was one.
    pub backup: Option<PathBuf>,
}

/// A client's state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// The client.
    pub client: Client,
    /// Its name.
    pub name: &'static str,
    /// Its configuration file.
    pub path: PathBuf,
    /// It seems installed on this machine.
    pub detected: bool,
    /// Teitunnel is in its configuration.
    pub connected: bool,
    /// The command it runs for Teitunnel, when connected.
    pub command: Option<String>,
    /// The file couldn't be read.
    pub problem: Option<String>,
}

fn io(path: &Path, source: std::io::Error) -> ClientError {
    ClientError::Io {
        path: path.display().to_string(),
        source,
    }
}

fn read(path: &Path) -> Result<Option<String>, ClientError> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io(path, e)),
    }
}

/// Backs `path` up (if it exists) and replaces it with `text` atomically, keeping its
/// permissions.
fn write(path: &Path, text: &str) -> Result<Option<PathBuf>, ClientError> {
    let parent = path
        .parent()
        .ok_or_else(|| ClientError::Shape(path.display().to_string()))?;
    std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
    let backup = if path.exists() {
        let mut name = path.as_os_str().to_owned();
        name.push(".teitunnel-backup");
        let backup = PathBuf::from(name);
        std::fs::copy(path, &backup).map_err(|e| io(&backup, e))?;
        Some(backup)
    } else {
        None
    };
    let mut temp = path.as_os_str().to_owned();
    temp.push(format!(".teitunnel-{}", std::process::id()));
    let temp = PathBuf::from(temp);
    std::fs::write(&temp, text).map_err(|e| io(&temp, e))?;
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(&temp, meta.permissions());
    }
    std::fs::rename(&temp, path).map_err(|e| {
        let _ = std::fs::remove_file(&temp);
        io(path, e)
    })?;
    Ok(backup)
}

fn to_input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(b) => CstInputValue::Bool(*b),
        Value::Number(n) => CstInputValue::Number(n.to_string()),
        Value::String(s) => CstInputValue::String(s.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(to_input).collect()),
        Value::Object(map) => {
            CstInputValue::Object(map.iter().map(|(k, v)| (k.clone(), to_input(v))).collect())
        }
    }
}

fn parse_json(client: Client, path: &Path, text: &str) -> Result<CstRootNode, ClientError> {
    let text = if text.trim().is_empty() { "{}" } else { text };
    let root = CstRootNode::parse(text, &ParseOptions::default()).map_err(|e| {
        ClientError::Unparseable {
            path: path.display().to_string(),
            format: "JSON",
            reason: e.to_string(),
            client: client.id(),
        }
    })?;
    if root.value().is_some() && root.object_value().is_none() {
        return Err(ClientError::Shape(path.display().to_string()));
    }
    Ok(root)
}

/// The container object, created when `create`.
fn container(root: &CstRootNode, keys: &[&str], create: bool) -> Option<CstObject> {
    let mut object = if create {
        root.object_value_or_set()
    } else {
        root.object_value()?
    };
    for key in keys {
        object = if create {
            object.object_value_or_set(key)
        } else {
            object.object_value(key)?
        };
    }
    Some(object)
}

fn set_codex(doc: &mut toml_edit::DocumentMut, command: &ServerCommand) {
    let servers = doc.entry("mcp_servers").or_insert_with(|| {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        toml_edit::Item::Table(table)
    });
    let mut table = toml_edit::Table::new();
    table.insert("command", toml_edit::value(command.command.clone()));
    let mut args = toml_edit::Array::new();
    for arg in &command.args {
        args.push(arg.clone());
    }
    table.insert("args", toml_edit::value(args));
    if !command.env.is_empty() {
        let mut env = toml_edit::InlineTable::new();
        for (k, v) in &command.env {
            env.insert(k, v.clone().into());
        }
        table.insert("env", toml_edit::value(env));
    }
    if let Some(servers) = servers.as_table_like_mut() {
        servers.insert(SERVER_NAME, toml_edit::Item::Table(table));
    }
}

fn codex_entry(
    doc: &toml_edit::DocumentMut,
) -> Option<(String, Vec<String>, BTreeMap<String, String>)> {
    let entry = doc.get("mcp_servers")?.get(SERVER_NAME)?;
    let command = entry.get("command")?.as_str()?.to_owned();
    let args = entry
        .get("args")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                .collect()
        })
        .unwrap_or_default();
    let env = entry
        .get("env")
        .and_then(|e| e.as_table_like())
        .map(|t| {
            t.iter()
                .filter_map(|(k, v)| Some((k.to_owned(), v.as_str()?.to_owned())))
                .collect()
        })
        .unwrap_or_default();
    Some((command, args, env))
}

fn parse_toml(path: &Path, text: &str) -> Result<toml_edit::DocumentMut, ClientError> {
    text.parse::<toml_edit::DocumentMut>()
        .map_err(|e| ClientError::Unparseable {
            path: path.display().to_string(),
            format: "TOML",
            reason: e.to_string().lines().next().unwrap_or_default().to_owned(),
            client: Client::Codex.id(),
        })
}

/// Adds (or updates) Teitunnel in `client`'s configuration.
///
/// # Errors
/// An unreadable, unparseable or unwritable file.
pub fn install(
    client: Client,
    command: &ServerCommand,
    paths: &Paths,
) -> Result<Change, ClientError> {
    let path = client.config_path(paths);
    let existing = read(&path)?;
    let text = existing.clone().unwrap_or_default();
    let updated = match client.format() {
        Format::Json {
            container: keys, ..
        } => {
            let root = parse_json(client, &path, &text)?;
            let entry = client.entry(command);
            let object = container(&root, keys, true)
                .ok_or_else(|| ClientError::Shape(path.display().to_string()))?;
            match object.get(SERVER_NAME) {
                Some(prop)
                    if prop.value().and_then(|v| v.to_serde_value()).as_ref() == Some(&entry) =>
                {
                    None
                }
                Some(prop) => {
                    prop.set_value(to_input(&entry));
                    Some(root.to_string())
                }
                None => {
                    object.append(SERVER_NAME, to_input(&entry));
                    Some(root.to_string())
                }
            }
        }
        Format::CodexToml => {
            let mut doc = parse_toml(&path, &text)?;
            let wanted = (
                command.command.clone(),
                command.args.clone(),
                command.env.clone(),
            );
            if codex_entry(&doc).as_ref() == Some(&wanted) {
                None
            } else {
                set_codex(&mut doc, command);
                Some(doc.to_string())
            }
        }
    };
    let Some(updated) = updated else {
        return Ok(Change {
            client,
            path,
            changed: false,
            backup: None,
        });
    };
    let mut updated = updated;
    if !updated.ends_with('\n') {
        updated.push('\n');
    }
    let backup = write(&path, &updated)?;
    Ok(Change {
        client,
        path,
        changed: true,
        backup,
    })
}

/// Removes Teitunnel from `client`'s configuration (nothing else).
///
/// # Errors
/// An unreadable, unparseable or unwritable file.
pub fn uninstall(client: Client, paths: &Paths) -> Result<Change, ClientError> {
    let path = client.config_path(paths);
    let unchanged = |path: PathBuf| Change {
        client,
        path,
        changed: false,
        backup: None,
    };
    let Some(text) = read(&path)? else {
        return Ok(unchanged(path));
    };
    let updated = match client.format() {
        Format::Json {
            container: keys, ..
        } => {
            let root = parse_json(client, &path, &text)?;
            match container(&root, keys, false).and_then(|o| o.get(SERVER_NAME)) {
                Some(prop) => {
                    prop.remove();
                    Some(root.to_string())
                }
                None => None,
            }
        }
        Format::CodexToml => {
            let mut doc = parse_toml(&path, &text)?;
            let removed = doc
                .get_mut("mcp_servers")
                .and_then(|s| s.as_table_like_mut())
                .and_then(|s| s.remove(SERVER_NAME))
                .is_some();
            removed.then(|| doc.to_string())
        }
    };
    let Some(updated) = updated else {
        return Ok(unchanged(path));
    };
    let backup = write(&path, &updated)?;
    Ok(Change {
        client,
        path,
        changed: true,
        backup,
    })
}

/// Whether `client` is connected, and how.
pub fn status(client: Client, paths: &Paths) -> Status {
    let path = client.config_path(paths);
    let mut status = Status {
        client,
        name: client.name(),
        detected: client.detected(paths),
        connected: false,
        command: None,
        problem: None,
        path: path.clone(),
    };
    let text = match read(&path) {
        Ok(Some(text)) => text,
        Ok(None) => return status,
        Err(err) => {
            status.problem = Some(err.to_string());
            return status;
        }
    };
    match client.format() {
        Format::Json {
            container: keys, ..
        } => match parse_json(client, &path, &text) {
            Ok(root) => {
                if let Some(entry) = container(&root, keys, false)
                    .and_then(|o| o.get(SERVER_NAME))
                    .and_then(|p| p.value())
                    .and_then(|v| v.to_serde_value())
                {
                    status.connected = true;
                    let command = entry["command"].as_str().unwrap_or_default();
                    let args: Vec<&str> = entry["args"]
                        .as_array()
                        .map(|a| a.iter().filter_map(Value::as_str).collect())
                        .unwrap_or_default();
                    status.command =
                        Some(format!("{command} {}", args.join(" ")).trim().to_owned());
                }
            }
            Err(err) => status.problem = Some(err.to_string()),
        },
        Format::CodexToml => match parse_toml(&path, &text) {
            Ok(doc) => {
                if let Some((command, args, _)) = codex_entry(&doc) {
                    status.connected = true;
                    status.command =
                        Some(format!("{command} {}", args.join(" ")).trim().to_owned());
                }
            }
            Err(err) => status.problem = Some(err.to_string()),
        },
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command() -> ServerCommand {
        ServerCommand {
            command: "/Applications/Teitunnel.app/Contents/MacOS/teitunnel-cli".into(),
            args: vec!["mcp".into()],
            env: BTreeMap::new(),
        }
    }

    #[test]
    fn parses_client_names() {
        for client in Client::ALL {
            assert_eq!(Client::parse(client.id()), Some(client));
        }
        assert_eq!(Client::parse("VS Code"), Some(Client::Vscode));
        assert_eq!(Client::parse("gemini"), Some(Client::GeminiCli));
        assert_eq!(Client::parse("notepad"), None);
    }

    #[test]
    fn knows_where_each_client_keeps_its_configuration() {
        let home = Path::new("/home/me");
        let mac = Paths::under(home, Os::MacOs);
        assert_eq!(
            Client::ClaudeDesktop.config_path(&mac),
            home.join("Library/Application Support/Claude/claude_desktop_config.json")
        );
        assert_eq!(
            Client::Vscode.config_path(&mac),
            home.join("Library/Application Support/Code/User/mcp.json")
        );
        assert_eq!(
            Client::Zed.config_path(&mac),
            home.join(".config/zed/settings.json")
        );
        let windows = Paths::under(home, Os::Windows);
        assert_eq!(
            Client::ClaudeDesktop.config_path(&windows),
            home.join("AppData/Roaming/Claude/claude_desktop_config.json")
        );
        assert_eq!(
            Client::Zed.config_path(&windows),
            home.join("AppData/Roaming/Zed/settings.json")
        );
        let linux = Paths::under(home, Os::Linux);
        assert_eq!(
            Client::Vscode.config_path(&linux),
            home.join(".config/Code/User/mcp.json")
        );
        assert_eq!(
            Client::Cursor.config_path(&linux),
            home.join(".cursor/mcp.json")
        );
        assert_eq!(
            Client::GeminiCli.config_path(&linux),
            home.join(".gemini/settings.json")
        );
        assert_eq!(
            Client::Windsurf.config_path(&linux),
            home.join(".codeium/windsurf/mcp_config.json")
        );
        assert_eq!(
            Client::ClaudeCode.config_path(&linux),
            home.join(".claude.json")
        );
    }

    #[test]
    fn merges_into_existing_json_keeping_everything_else() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::under(home.path(), Os::Linux);
        let path = Client::Cursor.config_path(&paths);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{ "mcpServers": { "other": { "command": "x" } }, "theme": "dark" }"#,
        )
        .unwrap();

        let change = install(Client::Cursor, &command(), &paths).unwrap();
        assert!(change.changed);
        let backup = change.backup.unwrap();
        assert!(
            std::fs::read_to_string(&backup)
                .unwrap()
                .contains("\"other\"")
        );
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(
            written["mcpServers"]["other"]["command"], "x",
            "other servers stay"
        );
        assert_eq!(written["theme"], "dark");
        assert_eq!(written["mcpServers"]["teitunnel"]["type"], "stdio");
        assert_eq!(written["mcpServers"]["teitunnel"]["args"][0], "mcp");

        // Idempotent: the same install writes nothing.
        let again = install(Client::Cursor, &command(), &paths).unwrap();
        assert!(!again.changed && again.backup.is_none());

        let status = status(Client::Cursor, &paths);
        assert!(status.connected && status.detected);
        assert!(status.command.unwrap().ends_with("teitunnel-cli mcp"));

        let removed = uninstall(Client::Cursor, &paths).unwrap();
        assert!(removed.changed);
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(written["mcpServers"].get("teitunnel").is_none());
        assert_eq!(written["mcpServers"]["other"]["command"], "x");
        assert!(!uninstall(Client::Cursor, &paths).unwrap().changed);
    }

    #[test]
    fn keeps_comments_in_jsonc_files() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::under(home.path(), Os::MacOs);
        let path = Client::Zed.config_path(&paths);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "// Zed settings\n{\n  // my font\n  \"buffer_font_size\": 15,\n}\n",
        )
        .unwrap();
        install(Client::Zed, &command(), &paths).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("// Zed settings") && text.contains("// my font"),
            "{text}"
        );
        assert!(text.contains("\"context_servers\""), "{text}");
        assert!(status(Client::Zed, &paths).connected);
    }

    #[test]
    fn creates_files_and_writes_codex_toml() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::under(home.path(), Os::Linux);
        let change = install(Client::ClaudeDesktop, &command(), &paths).unwrap();
        assert!(
            change.changed && change.backup.is_none(),
            "a new file needs no backup"
        );
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&change.path).unwrap()).unwrap();
        assert!(written["mcpServers"]["teitunnel"].get("type").is_none());

        let path = Client::Codex.config_path(&paths);
        if std::env::var_os("CODEX_HOME").is_some() {
            return;
        }
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            "model = \"gpt-5\"\n\n[mcp_servers.other]\ncommand = \"x\"\n",
        )
        .unwrap();
        assert!(install(Client::Codex, &command(), &paths).unwrap().changed);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("model = \"gpt-5\""), "{text}");
        assert!(text.contains("[mcp_servers.teitunnel]"), "{text}");
        assert!(text.contains("[mcp_servers.other]"), "{text}");
        assert!(!install(Client::Codex, &command(), &paths).unwrap().changed);
        assert!(status(Client::Codex, &paths).connected);
        assert!(uninstall(Client::Codex, &paths).unwrap().changed);
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            !text.contains("teitunnel") && text.contains("[mcp_servers.other]"),
            "{text}"
        );
    }

    #[test]
    fn never_touches_a_file_it_cant_parse() {
        let home = tempfile::tempdir().unwrap();
        let paths = Paths::under(home.path(), Os::Linux);
        let path = Client::GeminiCli.config_path(&paths);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "{ \"mcpServers\": ").unwrap();
        assert!(matches!(
            install(Client::GeminiCli, &command(), &paths),
            Err(ClientError::Unparseable { .. })
        ));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ \"mcpServers\": "
        );
        std::fs::write(&path, "[1, 2]").unwrap();
        assert!(matches!(
            install(Client::GeminiCli, &command(), &paths),
            Err(ClientError::Shape(_))
        ));
        assert!(
            status(Client::GeminiCli, &paths).problem.is_none()
                || !status(Client::GeminiCli, &paths).connected
        );
    }

    #[test]
    fn prints_snippets() {
        let json: Value = serde_json::from_str(&Client::Vscode.snippet(&command())).unwrap();
        assert_eq!(json["servers"]["teitunnel"]["type"], "stdio");
        let zed: Value = serde_json::from_str(&Client::Zed.snippet(&command())).unwrap();
        assert!(zed["context_servers"]["teitunnel"]["env"].is_object());
        let toml = Client::Codex.snippet(&command());
        assert!(
            toml.contains("[mcp_servers.teitunnel]") && toml.contains("args = [\"mcp\"]"),
            "{toml}"
        );
    }
}
