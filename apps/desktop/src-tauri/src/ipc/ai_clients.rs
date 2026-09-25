//! Settings ▸ AI tools: connecting AI clients (Claude Code, Cursor, VS Code…) to
//! Teitunnel's MCP server. The work is in `teitunnel_mcp::clients`; the command a client
//! runs is the `teitunnel` that ships with the app.

use std::path::PathBuf;

use serde::Serialize;
use teitunnel_core::{
    cli_install::{BUNDLED_NAME, CLI_NAME, Layout, Method},
    text::msg::ai_clients as m,
};
use teitunnel_mcp::clients::{self, Client, Paths, ServerCommand};

use crate::error::AppError;

/// An AI client, as Settings shows it.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiClientView {
    /// Its id, e.g. `claude-code`.
    pub id: String,
    /// Its name, e.g. `Claude Code`.
    pub name: String,
    /// Its MCP configuration file.
    pub path: String,
    /// It seems installed.
    pub detected: bool,
    /// Teitunnel is in its configuration.
    pub connected: bool,
    /// Its configuration couldn't be read (connecting would leave it alone).
    pub problem: Option<String>,
}

/// AI agents connected through `teitunnel mcp` while the app runs, and their changes
/// waiting for the person's answer (asked in a dialog).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiAgentsView {
    /// Connected agents.
    pub agents: Vec<teitunnel_core::control::ConnectedAgent>,
    /// Waiting approvals.
    pub approvals: Vec<teitunnel_core::control::PendingApproval>,
}

/// Agents connected now, and approvals waiting.
#[tauri::command]
#[specta::specta]
pub fn ai_agents(state: tauri::State<'_, crate::state::AppState>) -> AiAgentsView {
    AiAgentsView {
        agents: state.control.host.agents(),
        approvals: state.control.host.pending_approvals(),
    }
}

/// Every client, and whether Teitunnel can connect them (it needs its command line tool).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiClientsView {
    /// The command clients would run, when there is one.
    pub command: Option<String>,
    /// The clients.
    pub clients: Vec<AiClientView>,
}

/// The `teitunnel` clients should run: a copy on the PATH that Teitunnel put there (a
/// stable path, unlike an AppImage's mount), else the one inside the app, else (a
/// development build) the one built next to it.
fn command() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    if let Some(layout) = Layout::detect(&exe) {
        if let Method::Copy(dir) = &layout.method
            && dir.join(CLI_NAME).is_file()
        {
            return Some(dir.join(CLI_NAME));
        }
        return Some(layout.bundled);
    }
    let dev = exe.with_file_name(BUNDLED_NAME);
    let dev = if dev.is_file() {
        dev
    } else {
        exe.with_file_name(if cfg!(windows) {
            "teitunnel-cli.exe"
        } else {
            "teitunnel-cli"
        })
    };
    dev.is_file().then_some(dev)
}

fn view() -> AiClientsView {
    let command = command().map(|p| p.display().to_string());
    let clients = Paths::detect()
        .map(|paths| {
            Client::ALL
                .iter()
                .map(|client| {
                    let status = clients::status(*client, &paths);
                    AiClientView {
                        id: client.id().to_owned(),
                        name: client.name().to_owned(),
                        path: status.path.display().to_string(),
                        detected: status.detected,
                        connected: status.connected,
                        problem: status.problem,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    AiClientsView { command, clients }
}

fn client(id: &str) -> Result<Client, AppError> {
    Client::parse(id).ok_or_else(|| AppError::invalid("client", m::unknown()))
}

fn failed(err: &clients::ClientError) -> AppError {
    tracing::warn!(error = %err, "AI client configuration");
    AppError::internal(m::failed(err.to_string()))
}

/// The AI clients on this computer and whether each is connected.
#[tauri::command]
#[specta::specta]
pub fn ai_clients_status() -> AiClientsView {
    view()
}

/// Connects an AI client: adds Teitunnel to its MCP configuration (merged, with a backup).
#[tauri::command]
#[specta::specta]
pub fn ai_clients_connect(client_id: String) -> Result<AiClientsView, AppError> {
    let client = client(&client_id)?;
    let command = command().ok_or_else(|| AppError::internal(m::no_cli()))?;
    let paths = Paths::detect().map_err(|e| failed(&e))?;
    let server = ServerCommand {
        command: command.display().to_string(),
        args: vec!["mcp".to_owned()],
        env: std::collections::BTreeMap::new(),
    };
    clients::install(client, &server, &paths).map_err(|e| failed(&e))?;
    Ok(view())
}

/// Disconnects an AI client: removes Teitunnel from its MCP configuration.
#[tauri::command]
#[specta::specta]
pub fn ai_clients_disconnect(client_id: String) -> Result<AiClientsView, AppError> {
    let client = client(&client_id)?;
    let paths = Paths::detect().map_err(|e| failed(&e))?;
    clients::uninstall(client, &paths).map_err(|e| failed(&e))?;
    Ok(view())
}
