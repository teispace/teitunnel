//! Connecting AI clients: writes (and removes) Teitunnel's entry in each client's MCP
//! configuration, for `teitunnel mcp install|uninstall|config|status` and the app's
//! "Connect an AI tool".
//!
//! Every write is a merge: only the `teitunnel` entry changes; other servers, settings
//! and comments (JSONC files such as VS Code's and Zed's) stay as they are. Nothing is
//! written when the entry is already right; otherwise the file is backed up first
//! (`<file>.teitunnel-backup`) and replaced atomically. A file that can't be parsed is
//! never touched. Locations and formats were checked against each client's
//! documentation on 2026-09-24 (Copilot CLI, opencode, Kiro, LM Studio and Junie on
//! 2026-09-27).
//!
//! A client counts as installed only when its app or program is found (an app bundle,
//! a program in a usual install folder, a desktop entry): a configuration folder alone
//! is often left behind after uninstalling, so it proves nothing.

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
    /// GitHub Copilot CLI (`~/.copilot/mcp-config.json`, or under `$COPILOT_HOME`).
    CopilotCli,
    /// opencode (`~/.config/opencode/opencode.json`, `mcp`).
    Opencode,
    /// Kiro, the IDE and `kiro-cli` (`~/.kiro/settings/mcp.json`).
    Kiro,
    /// LM Studio (`~/.lmstudio/mcp.json`).
    LmStudio,
    /// JetBrains Junie (`~/.junie/mcp/mcp.json`).
    Junie,
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

/// Where configuration lives, and where apps are installed, on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The home folder.
    pub home: PathBuf,
    /// Per-user application data: `~/Library/Application Support` (macOS), `%APPDATA%`
    /// (Windows), `$XDG_CONFIG_HOME` or `~/.config` (Linux).
    pub app_data: PathBuf,
    /// `$XDG_CONFIG_HOME` or `~/.config` (Zed on macOS and Linux).
    pub xdg_config: PathBuf,
    /// `%LOCALAPPDATA%` (Windows): per-user installs and Store apps' folders.
    pub local_app_data: PathBuf,
    /// Folders programs are installed in: `$PATH`, then the usual ones an app started
    /// from the Dock or Start menu doesn't have on its `PATH` (Homebrew, npm, `~/.local/bin`…).
    pub programs: Vec<PathBuf>,
    /// Folders app bundles are installed in (macOS).
    pub applications: Vec<PathBuf>,
    /// Folders of desktop entries (Linux, including Flatpak and Snap).
    pub desktop_entries: Vec<PathBuf>,
    /// Program Files folders (Windows).
    pub program_files: Vec<PathBuf>,
    /// `$CODEX_HOME`, `$CLAUDE_CONFIG_DIR` and `$COPILOT_HOME`, when set.
    pub overrides: Overrides,
    /// Which platform's layout to use.
    pub os: Os,
}

/// Folders a client's configuration was moved to with an environment variable.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Overrides {
    /// `$CODEX_HOME`.
    pub codex_home: Option<PathBuf>,
    /// `$CLAUDE_CONFIG_DIR`.
    pub claude_config_dir: Option<PathBuf>,
    /// `$COPILOT_HOME`.
    pub copilot_home: Option<PathBuf>,
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

/// An absolute path from an environment variable.
fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

/// `base` joined with a `/`-separated relative path.
fn under(base: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.to_path_buf(), |p, c| p.join(c))
}

impl Paths {
    /// This machine's.
    ///
    /// # Errors
    /// No home folder.
    pub fn detect() -> Result<Self, ClientError> {
        let home = std::env::home_dir().ok_or(ClientError::NoHome)?;
        let os = if cfg!(target_os = "macos") {
            Os::MacOs
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        };
        let mut paths = Self::under(&home, os);
        if let Some(xdg) = env_path("XDG_CONFIG_HOME") {
            if os == Os::Linux {
                paths.app_data.clone_from(&xdg);
            }
            paths.xdg_config = xdg;
        }
        if os == Os::Windows {
            if let Some(roaming) = env_path("APPDATA") {
                paths.app_data = roaming;
            }
            if let Some(local) = env_path("LOCALAPPDATA") {
                paths.local_app_data = local;
            }
            paths.program_files = ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"]
                .into_iter()
                .filter_map(env_path)
                .collect();
            paths.programs.extend([
                paths.app_data.join("npm"),
                under(&paths.local_app_data, "Microsoft/WinGet/Links"),
                under(&paths.local_app_data, "Programs"),
            ]);
        } else {
            paths.programs.extend(
                [
                    "/opt/homebrew/bin",
                    "/usr/local/bin",
                    "/usr/bin",
                    "/snap/bin",
                    "/var/lib/flatpak/exports/bin",
                ]
                .map(PathBuf::from),
            );
            paths.programs.extend(node_version_bins(&home));
        }
        let from_path: Vec<PathBuf> = std::env::var_os("PATH")
            .map(|p| {
                std::env::split_paths(&p)
                    .filter(|p| p.is_absolute())
                    .collect()
            })
            .unwrap_or_default();
        paths.programs.splice(0..0, from_path);
        paths.programs.dedup();
        paths.overrides = Overrides {
            codex_home: env_path("CODEX_HOME"),
            claude_config_dir: env_path("CLAUDE_CONFIG_DIR"),
            copilot_home: env_path("COPILOT_HOME"),
        };
        Ok(paths)
    }

    /// A layout rooted at `home`: this machine's, and the whole of it in tests.
    pub fn under(home: &Path, os: Os) -> Self {
        let xdg_config = home.join(".config");
        let app_data = match os {
            Os::MacOs => under(home, "Library/Application Support"),
            Os::Windows => under(home, "AppData/Roaming"),
            Os::Linux => xdg_config.clone(),
        };
        let programs = [
            ".local/bin",
            "bin",
            ".npm-global/bin",
            ".volta/bin",
            ".bun/bin",
            "Library/pnpm",
            ".local/share/pnpm",
            ".cargo/bin",
        ]
        .into_iter()
        .map(|p| under(home, p))
        .collect();
        Self {
            home: home.to_path_buf(),
            app_data,
            xdg_config,
            local_app_data: under(home, "AppData/Local"),
            programs,
            applications: vec![PathBuf::from("/Applications"), home.join("Applications")],
            desktop_entries: vec![
                PathBuf::from("/usr/share/applications"),
                PathBuf::from("/usr/local/share/applications"),
                under(home, ".local/share/applications"),
                PathBuf::from("/var/lib/flatpak/exports/share/applications"),
                under(home, ".local/share/flatpak/exports/share/applications"),
                PathBuf::from("/var/lib/snapd/desktop/applications"),
            ],
            program_files: Vec::new(),
            overrides: Overrides::default(),
            os,
        }
    }
}

/// The `bin` folders of Node versions installed with nvm or fnm, where npm puts global
/// commands such as `gemini` and `codex`.
fn node_version_bins(home: &Path) -> Vec<PathBuf> {
    let mut bins = Vec::new();
    for (versions, bin) in [
        (".nvm/versions/node", "bin"),
        (".local/share/fnm/node-versions", "installation/bin"),
        (
            "Library/Application Support/fnm/node-versions",
            "installation/bin",
        ),
    ] {
        if let Ok(entries) = std::fs::read_dir(under(home, versions)) {
            bins.extend(entries.flatten().map(|e| under(&e.path(), bin)));
        }
    }
    bins
}

/// Something that shows a client is installed.
#[derive(Debug, Clone, Copy)]
enum Sign {
    /// An app bundle, `<name>.app`, in an applications folder (macOS).
    MacApp(&'static str),
    /// A program in one of [`Paths::programs`] (on Windows also `.exe` and `.cmd`).
    Program(&'static str),
    /// A file or folder under the home folder (every platform).
    Home(&'static str),
    /// A file or folder under `%LOCALAPPDATA%` (Windows).
    LocalAppData(&'static str),
    /// A file under a Program Files folder (Windows).
    ProgramFiles(&'static str),
    /// A desktop entry (Linux).
    Desktop(&'static str),
}

impl Sign {
    /// Where it was found, if it was.
    fn find(self, paths: &Paths) -> Option<PathBuf> {
        // `exists` follows links, so a link left behind by an uninstall doesn't count.
        let found = |p: PathBuf| p.exists().then_some(p);
        match (self, paths.os) {
            (Self::MacApp(name), Os::MacOs) => paths
                .applications
                .iter()
                .find_map(|dir| found(dir.join(format!("{name}.app")))),
            (Self::Program(name), os) => {
                let names: &[String] = &if os == Os::Windows {
                    vec![format!("{name}.exe"), format!("{name}.cmd")]
                } else {
                    vec![name.to_owned()]
                };
                paths.programs.iter().find_map(|dir| {
                    names
                        .iter()
                        .find_map(|n| found(dir.join(n)).filter(|p| p.is_file()))
                })
            }
            (Self::Home(relative), _) => found(under(&paths.home, relative)),
            (Self::LocalAppData(relative), Os::Windows) => {
                found(under(&paths.local_app_data, relative))
            }
            (Self::ProgramFiles(relative), Os::Windows) => paths
                .program_files
                .iter()
                .find_map(|dir| found(under(dir, relative))),
            (Self::Desktop(name), Os::Linux) => paths
                .desktop_entries
                .iter()
                .find_map(|dir| found(dir.join(name))),
            _ => None,
        }
    }
}

/// Claude Desktop from the Microsoft Store keeps its files in its package folder.
const CLAUDE_STORE_PACKAGE: &str = "Packages/Claude_pzs8sxrjxfjjc";

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
    /// A JSON (or JSONC) object at `container` (nested keys), entries shaped as `entry`.
    Json {
        container: &'static [&'static str],
        entry: Entry,
    },
    /// `[mcp_servers.<name>]` in TOML.
    CodexToml,
}

/// How a JSON client wants its entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entry {
    /// `{ command, args, env? }`.
    Plain,
    /// `{ type: "stdio", command, args, env? }`.
    Stdio,
    /// Copilot CLI: `{ type: "local", command, args, env?, tools: ["*"] }`.
    CopilotLocal,
    /// opencode: `{ type: "local", command: [program, ...args], environment?, enabled }`.
    OpencodeLocal,
}

impl Client {
    /// Every client.
    pub const ALL: [Self; 13] = [
        Self::ClaudeCode,
        Self::ClaudeDesktop,
        Self::Cursor,
        Self::Vscode,
        Self::Codex,
        Self::Windsurf,
        Self::Zed,
        Self::GeminiCli,
        Self::CopilotCli,
        Self::Opencode,
        Self::Kiro,
        Self::LmStudio,
        Self::Junie,
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
            Self::CopilotCli => "copilot-cli",
            Self::Opencode => "opencode",
            Self::Kiro => "kiro",
            Self::LmStudio => "lm-studio",
            Self::Junie => "junie",
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
            Self::CopilotCli => "GitHub Copilot CLI",
            Self::Opencode => "opencode",
            Self::Kiro => "Kiro",
            Self::LmStudio => "LM Studio",
            Self::Junie => "Junie",
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
                "code" | "vs-code" => Some(Self::Vscode),
                "gemini" => Some(Self::GeminiCli),
                "copilot" | "gh-copilot" => Some(Self::CopilotCli),
                "kiro-cli" => Some(Self::Kiro),
                "lmstudio" | "lms" => Some(Self::LmStudio),
                _ => None,
            })
    }

    /// The client an MCP connection comes from, by the name it gives in `clientInfo`
    /// (best effort: clients name themselves freely, e.g. `claude-code`,
    /// `Visual Studio Code`, `cursor-vscode`, `codex-mcp-client`).
    pub fn from_client_info(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        let has = |part: &str| name.contains(part);
        Some(if has("claude-code") || has("claude code") {
            Self::ClaudeCode
        } else if has("claude") {
            Self::ClaudeDesktop
        } else if has("cursor") {
            Self::Cursor
        } else if has("copilot") && !has("visual studio") {
            Self::CopilotCli
        } else if has("visual studio code") || has("vscode") || name == "code" {
            Self::Vscode
        } else if has("codex") {
            Self::Codex
        } else if has("windsurf") || has("codeium") {
            Self::Windsurf
        } else if has("zed") {
            Self::Zed
        } else if has("gemini") {
            Self::GeminiCli
        } else if has("opencode") {
            Self::Opencode
        } else if has("kiro") {
            Self::Kiro
        } else if has("lm studio") || has("lmstudio") || has("lm-studio") {
            Self::LmStudio
        } else if has("junie") {
            Self::Junie
        } else {
            return None;
        })
    }

    fn format(self) -> Format {
        let json = |container, entry| Format::Json { container, entry };
        match self {
            Self::ClaudeCode | Self::Cursor => json(&["mcpServers"], Entry::Stdio),
            Self::Vscode => json(&["servers"], Entry::Stdio),
            Self::ClaudeDesktop
            | Self::Windsurf
            | Self::GeminiCli
            | Self::Kiro
            | Self::LmStudio
            | Self::Junie => json(&["mcpServers"], Entry::Plain),
            Self::Zed => json(&["context_servers"], Entry::Plain),
            Self::CopilotCli => json(&["mcpServers"], Entry::CopilotLocal),
            Self::Opencode => json(&["mcp"], Entry::OpencodeLocal),
            Self::Codex => Format::CodexToml,
        }
    }

    /// Its configuration file.
    pub fn config_path(self, paths: &Paths) -> PathBuf {
        let home = &paths.home;
        let overrides = &paths.overrides;
        match self {
            Self::ClaudeCode => overrides
                .claude_config_dir
                .as_deref()
                .unwrap_or(home)
                .join(".claude.json"),
            Self::ClaudeDesktop => {
                let store = under(&paths.local_app_data, CLAUDE_STORE_PACKAGE);
                let base = if paths.os == Os::Windows && store.is_dir() {
                    under(&store, "LocalCache/Roaming")
                } else {
                    paths.app_data.clone()
                };
                under(&base, "Claude/claude_desktop_config.json")
            }
            Self::Cursor => under(home, ".cursor/mcp.json"),
            Self::Vscode => under(&paths.app_data, "Code/User/mcp.json"),
            Self::Codex => overrides
                .codex_home
                .clone()
                .unwrap_or_else(|| home.join(".codex"))
                .join("config.toml"),
            Self::Windsurf => under(home, ".codeium/windsurf/mcp_config.json"),
            Self::Zed => match paths.os {
                Os::Windows => under(&paths.app_data, "Zed/settings.json"),
                Os::MacOs | Os::Linux => under(&paths.xdg_config, "zed/settings.json"),
            },
            Self::GeminiCli => under(home, ".gemini/settings.json"),
            Self::CopilotCli => overrides
                .copilot_home
                .clone()
                .unwrap_or_else(|| home.join(".copilot"))
                .join("mcp-config.json"),
            Self::Opencode => {
                // opencode reads `opencode.jsonc` or `opencode.json`; use the one there is.
                let dir = under(home, ".config/opencode");
                let jsonc = dir.join("opencode.jsonc");
                if jsonc.exists() {
                    jsonc
                } else {
                    dir.join("opencode.json")
                }
            }
            Self::Kiro => under(home, ".kiro/settings/mcp.json"),
            Self::LmStudio => under(home, ".lmstudio/mcp.json"),
            Self::Junie => under(home, ".junie/mcp/mcp.json"),
        }
    }

    /// What shows the client is installed.
    fn signs(self) -> &'static [Sign] {
        use Sign::{Desktop, Home, LocalAppData, MacApp, Program, ProgramFiles};
        match self {
            Self::ClaudeCode => &[
                Program("claude"),
                Home(".claude/local/claude"),
                Home(".local/share/claude/versions"),
            ],
            Self::ClaudeDesktop => &[
                MacApp("Claude"),
                LocalAppData("AnthropicClaude/claude.exe"),
                LocalAppData(CLAUDE_STORE_PACKAGE),
                Desktop("claude-desktop.desktop"),
                Program("claude-desktop"),
            ],
            Self::Cursor => &[
                MacApp("Cursor"),
                LocalAppData("Programs/cursor/Cursor.exe"),
                Desktop("cursor.desktop"),
                Program("cursor"),
                Program("cursor-agent"),
            ],
            Self::Vscode => &[
                MacApp("Visual Studio Code"),
                LocalAppData("Programs/Microsoft VS Code/Code.exe"),
                ProgramFiles("Microsoft VS Code/Code.exe"),
                Desktop("code.desktop"),
                Program("code"),
            ],
            Self::Codex => &[MacApp("Codex"), Program("codex")],
            Self::Windsurf => &[
                MacApp("Windsurf"),
                LocalAppData("Programs/Windsurf/Windsurf.exe"),
                Desktop("windsurf.desktop"),
                Program("windsurf"),
            ],
            Self::Zed => &[
                MacApp("Zed"),
                LocalAppData("Programs/Zed/Zed.exe"),
                Desktop("dev.zed.Zed.desktop"),
                Home(".local/zed.app"),
                Program("zed"),
            ],
            Self::GeminiCli => &[Program("gemini")],
            Self::CopilotCli => &[Program("copilot")],
            Self::Opencode => &[Program("opencode"), Home(".opencode/bin/opencode")],
            Self::Kiro => &[
                MacApp("Kiro"),
                LocalAppData("Programs/Kiro/Kiro.exe"),
                Desktop("kiro.desktop"),
                Program("kiro-cli"),
                Program("kiro"),
            ],
            Self::LmStudio => &[
                MacApp("LM Studio"),
                LocalAppData("Programs/LM Studio/LM Studio.exe"),
                Desktop("lm-studio.desktop"),
                Home(".lmstudio/bin/lms"),
                Home(".lmstudio/bin/lms.exe"),
            ],
            Self::Junie => &[Program("junie")],
        }
    }

    /// Where the client's app or program was found, if it's installed.
    pub fn installed(self, paths: &Paths) -> Option<PathBuf> {
        self.signs().iter().find_map(|sign| sign.find(paths))
    }

    /// Whether the client is installed (see [`Client::installed`]).
    pub fn detected(self, paths: &Paths) -> bool {
        self.installed(paths).is_some()
    }

    /// The entry, as this client wants it.
    fn entry(self, command: &ServerCommand) -> Value {
        let style = match self.format() {
            Format::Json { entry, .. } => entry,
            Format::CodexToml => Entry::Plain,
        };
        let env = || {
            Value::Object(
                command
                    .env
                    .iter()
                    .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                    .collect(),
            )
        };
        let mut entry = serde_json::Map::new();
        if style == Entry::OpencodeLocal {
            let mut program = vec![Value::String(command.command.clone())];
            program.extend(command.args.iter().cloned().map(Value::String));
            entry.insert("type".into(), "local".into());
            entry.insert("command".into(), Value::Array(program));
            if !command.env.is_empty() {
                entry.insert("environment".into(), env());
            }
            entry.insert("enabled".into(), true.into());
            return Value::Object(entry);
        }
        match style {
            Entry::Stdio => {
                entry.insert("type".into(), "stdio".into());
            }
            Entry::CopilotLocal => {
                entry.insert("type".into(), "local".into());
            }
            Entry::Plain | Entry::OpencodeLocal => {}
        }
        entry.insert("command".into(), command.command.clone().into());
        entry.insert("args".into(), command.args.clone().into());
        if !command.env.is_empty() || matches!(self, Self::Zed) {
            entry.insert("env".into(), env());
        }
        if style == Entry::CopilotLocal {
            entry.insert("tools".into(), serde_json::json!(["*"]));
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
    /// Where its app or program was found: it's installed.
    pub installed_at: Option<PathBuf>,
    /// It's installed (`installed_at` is set).
    pub detected: bool,
    /// Teitunnel is in its configuration.
    pub connected: bool,
    /// The command it runs for Teitunnel, when connected, as one line.
    pub command: Option<String>,
    /// That command's program and arguments.
    pub program: Option<PathBuf>,
    /// That command's arguments.
    pub args: Vec<String>,
    /// The file couldn't be read.
    pub problem: Option<String>,
}

/// Why a client's Teitunnel entry needs updating.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Stale {
    /// The program it starts isn't there any more (Teitunnel moved or was reinstalled).
    Missing,
    /// It starts another program, or other arguments, than this Teitunnel would write.
    Different,
}

/// Where a client stands, in one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum State {
    /// Its configuration file can't be read.
    Unreadable,
    /// Connected, starting the right program.
    Connected,
    /// Connected, but the entry starts a program that moved or other arguments.
    NeedsUpdate,
    /// Teitunnel is in its configuration, but the client isn't installed (left over).
    Leftover,
    /// Installed, not connected.
    NotConnected,
    /// Not installed.
    NotInstalled,
}

impl Status {
    /// Where the client stands, `expected` being what connecting it would write.
    pub fn state(&self, expected: &ServerCommand) -> State {
        if self.problem.is_some() {
            State::Unreadable
        } else if self.connected && !self.detected {
            State::Leftover
        } else if self.connected && self.stale(expected).is_some() {
            State::NeedsUpdate
        } else if self.connected {
            State::Connected
        } else if self.detected {
            State::NotConnected
        } else {
            State::NotInstalled
        }
    }

    /// Whether the entry needs updating to start `expected`.
    pub fn stale(&self, expected: &ServerCommand) -> Option<Stale> {
        let program = self.program.as_ref().filter(|_| self.connected)?;
        if !program.is_file() {
            Some(Stale::Missing)
        } else if program != Path::new(&expected.command) || self.args != expected.args {
            Some(Stale::Different)
        } else {
            None
        }
    }
}

/// The program and arguments of an entry: `command` + `args`, or opencode's
/// `command: [program, ...args]`.
fn entry_command(entry: &Value) -> Option<(String, Vec<String>)> {
    let strings = |value: &Value| -> Vec<String> {
        value
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    match &entry["command"] {
        Value::String(command) => Some((command.clone(), strings(&entry["args"]))),
        list @ Value::Array(_) => {
            let mut parts = strings(list).into_iter();
            Some((parts.next()?, parts.collect()))
        }
        _ => None,
    }
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

/// Whether `client` is installed and connected, and how.
pub fn status(client: Client, paths: &Paths) -> Status {
    let path = client.config_path(paths);
    let installed_at = client.installed(paths);
    let mut status = Status {
        client,
        name: client.name(),
        detected: installed_at.is_some(),
        installed_at,
        connected: false,
        command: None,
        program: None,
        args: Vec::new(),
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
    let found = match client.format() {
        Format::Json {
            container: keys, ..
        } => match parse_json(client, &path, &text) {
            Ok(root) => container(&root, keys, false)
                .and_then(|o| o.get(SERVER_NAME))
                .and_then(|p| p.value())
                .and_then(|v| v.to_serde_value())
                .map(|entry| entry_command(&entry)),
            Err(err) => {
                status.problem = Some(err.to_string());
                None
            }
        },
        Format::CodexToml => match parse_toml(&path, &text) {
            Ok(doc) => codex_entry(&doc).map(|(command, args, _)| Some((command, args))),
            Err(err) => {
                status.problem = Some(err.to_string());
                None
            }
        },
    };
    if let Some(entry) = found {
        status.connected = true;
        if let Some((program, args)) = entry {
            status.command = Some(format!("{program} {}", args.join(" ")).trim().to_owned());
            status.program = Some(PathBuf::from(program));
            status.args = args;
        }
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
        assert!(status.connected);
        assert!(
            !status.detected,
            "a configuration file alone isn't an install"
        );
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

    /// `Paths::under` without the machine's own app folders.
    fn isolated(home: &Path, os: Os) -> Paths {
        let mut paths = Paths::under(home, os);
        paths.applications = vec![home.join("Applications")];
        paths.desktop_entries = vec![home.join(".local/share/applications")];
        paths.program_files = vec![home.join("Program Files")];
        paths
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn finds_installed_apps_not_leftover_folders() {
        let home = tempfile::tempdir().unwrap();
        let linux = isolated(home.path(), Os::Linux);
        // Configuration folders stay behind after uninstalling: they prove nothing.
        std::fs::create_dir_all(home.path().join(".cursor")).unwrap();
        std::fs::create_dir_all(home.path().join(".gemini")).unwrap();
        assert!(!Client::Cursor.detected(&linux));
        assert!(!Client::GeminiCli.detected(&linux));

        touch(&home.path().join(".local/bin/gemini"));
        assert_eq!(
            Client::GeminiCli.installed(&linux),
            Some(home.path().join(".local/bin/gemini"))
        );
        touch(&home.path().join(".local/share/applications/cursor.desktop"));
        assert!(Client::Cursor.detected(&linux));

        let mac = isolated(home.path(), Os::MacOs);
        assert!(!Client::ClaudeDesktop.detected(&mac));
        std::fs::create_dir_all(home.path().join("Applications/Claude.app")).unwrap();
        assert!(Client::ClaudeDesktop.detected(&mac));
        // A desktop entry means nothing on macOS.
        assert!(!Client::Windsurf.detected(&mac));

        let windows = isolated(home.path(), Os::Windows);
        touch(&home.path().join("AppData/Local/Programs/Zed/Zed.exe"));
        assert!(Client::Zed.detected(&windows));
        touch(&home.path().join(".local/bin/claude.exe"));
        assert!(Client::ClaudeCode.detected(&windows));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_left_by_an_uninstall_isnt_an_install() {
        let home = tempfile::tempdir().unwrap();
        let paths = isolated(home.path(), Os::Linux);
        let bin = home.path().join(".local/bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(home.path().join("gone/code"), bin.join("code")).unwrap();
        assert!(!Client::Vscode.detected(&paths));
    }

    #[test]
    fn follows_configuration_moved_by_environment_or_the_store() {
        let home = tempfile::tempdir().unwrap();
        let mut paths = isolated(home.path(), Os::Windows);
        paths.overrides.claude_config_dir = Some(home.path().join("claude-config"));
        paths.overrides.copilot_home = Some(home.path().join("copilot"));
        assert_eq!(
            Client::ClaudeCode.config_path(&paths),
            home.path().join("claude-config/.claude.json")
        );
        assert_eq!(
            Client::CopilotCli.config_path(&paths),
            home.path().join("copilot/mcp-config.json")
        );
        assert_eq!(
            Client::ClaudeDesktop.config_path(&paths),
            home.path()
                .join("AppData/Roaming/Claude/claude_desktop_config.json")
        );
        // Claude from the Microsoft Store reads the file in its package folder.
        std::fs::create_dir_all(
            home.path()
                .join("AppData/Local/Packages/Claude_pzs8sxrjxfjjc"),
        )
        .unwrap();
        assert!(Client::ClaudeDesktop.detected(&paths));
        assert_eq!(
            Client::ClaudeDesktop.config_path(&paths),
            home.path().join(
                "AppData/Local/Packages/Claude_pzs8sxrjxfjjc/LocalCache/Roaming/Claude/claude_desktop_config.json"
            )
        );
    }

    #[test]
    fn writes_copilot_and_opencode_entries_their_way() {
        let home = tempfile::tempdir().unwrap();
        let paths = isolated(home.path(), Os::Linux);
        let copilot = install(Client::CopilotCli, &command(), &paths).unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&copilot.path).unwrap()).unwrap();
        let entry = &written["mcpServers"]["teitunnel"];
        assert_eq!(entry["type"], "local");
        assert_eq!(entry["tools"][0], "*");

        let opencode = install(Client::Opencode, &command(), &paths).unwrap();
        assert!(opencode.path.ends_with(".config/opencode/opencode.json"));
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(&opencode.path).unwrap()).unwrap();
        let entry = &written["mcp"]["teitunnel"];
        assert_eq!(entry["type"], "local");
        assert_eq!(entry["command"][1], "mcp");
        assert_eq!(entry["enabled"], true);
        let status = status(Client::Opencode, &paths);
        assert!(status.connected);
        assert_eq!(status.args, ["mcp"]);
        assert!(
            !install(Client::Opencode, &command(), &paths)
                .unwrap()
                .changed
        );
    }

    #[test]
    fn says_when_an_entry_needs_updating() {
        let home = tempfile::tempdir().unwrap();
        let paths = isolated(home.path(), Os::Linux);
        let program = home.path().join("Teitunnel/teitunnel-cli");
        touch(&program);
        let current = ServerCommand {
            command: program.display().to_string(),
            args: vec!["mcp".into()],
            env: BTreeMap::new(),
        };
        // Written by a Teitunnel that has since moved.
        let moved = ServerCommand {
            command: home.path().join("Old/teitunnel-cli").display().to_string(),
            ..current.clone()
        };
        install(Client::Cursor, &moved, &paths).unwrap();
        assert_eq!(
            status(Client::Cursor, &paths).stale(&current),
            Some(Stale::Missing)
        );
        install(Client::Cursor, &current, &paths).unwrap();
        assert_eq!(status(Client::Cursor, &paths).stale(&current), None);
        let read_only = ServerCommand {
            args: vec!["mcp".into(), "--mode".into(), "read-only".into()],
            ..current.clone()
        };
        assert_eq!(
            status(Client::Cursor, &paths).stale(&read_only),
            Some(Stale::Different)
        );
        assert_eq!(
            status(Client::Windsurf, &paths).stale(&current),
            None,
            "not connected"
        );
    }

    #[test]
    fn knows_clients_by_the_name_they_give() {
        for (name, client) in [
            ("claude-code", Client::ClaudeCode),
            ("claude-ai", Client::ClaudeDesktop),
            ("cursor-vscode", Client::Cursor),
            ("Visual Studio Code", Client::Vscode),
            ("codex-mcp-client", Client::Codex),
            ("Zed", Client::Zed),
            ("gemini-cli-mcp-client", Client::GeminiCli),
            ("github-copilot-cli", Client::CopilotCli),
            ("opencode", Client::Opencode),
        ] {
            assert_eq!(Client::from_client_info(name), Some(client), "{name}");
        }
        assert_eq!(Client::from_client_info("my-script"), None);
    }
}
