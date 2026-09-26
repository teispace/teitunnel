//! Settings ▸ AI tools: connecting AI clients (Claude Code, Cursor, VS Code…) to
//! Teitunnel's MCP server. The work is in `teitunnel_mcp::clients`; the command a client
//! runs is the `teitunnel` that ships with the app.

use std::path::PathBuf;

use serde::Serialize;
use teitunnel_core::{
    agents_seen::{self, SeenAgent},
    cli_install::{BUNDLED_NAME, CLI_NAME, Layout, Method},
    text::msg::ai_clients as m,
};
use teitunnel_mcp::{
    check,
    clients::{self, Client, Paths, ServerCommand},
};

use crate::error::AppError;

/// Where an AI client stands (see `teitunnel_mcp::clients::State`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub enum AiClientState {
    /// Its configuration file can't be read (connecting would leave it alone).
    Unreadable,
    /// Connected, starting this Teitunnel.
    Connected,
    /// Connected to a Teitunnel that moved, or with other arguments: update it.
    NeedsUpdate,
    /// Teitunnel is in its configuration, but the client isn't installed any more.
    Leftover,
    /// Installed, not connected.
    NotConnected,
    /// Not installed on this computer.
    NotInstalled,
}

impl From<clients::State> for AiClientState {
    fn from(state: clients::State) -> Self {
        match state {
            clients::State::Unreadable => Self::Unreadable,
            clients::State::Connected => Self::Connected,
            clients::State::NeedsUpdate => Self::NeedsUpdate,
            clients::State::Leftover => Self::Leftover,
            clients::State::NotConnected => Self::NotConnected,
            clients::State::NotInstalled => Self::NotInstalled,
        }
    }
}

/// An AI client, as AI & Integrations shows it.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiClientView {
    /// Its id, e.g. `claude-code`.
    pub id: String,
    /// Its name, e.g. `Claude Code`.
    pub name: String,
    /// Where it stands.
    pub state: AiClientState,
    /// Its MCP configuration file.
    pub path: String,
    /// Where its app or program was found.
    pub installed_at: Option<String>,
    /// The command its configuration runs for Teitunnel, when connected.
    pub command: Option<String>,
    /// When an agent of this client last used Teitunnel (milliseconds since the epoch).
    #[specta(type = Option<f64>)]
    pub last_used_at: Option<u64>,
    /// The entry to add by hand, in its configuration's format.
    pub snippet: Option<String>,
    /// Why its configuration couldn't be read.
    pub problem: Option<String>,
}

/// A working server's answer to "Test connection".
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiClientCheck {
    /// The server's name and version, e.g. `teitunnel 0.4.1`.
    pub server: String,
    /// The protocol version it chose.
    pub protocol: String,
    /// How many tools it offers.
    pub tools: u32,
    /// How long it took, in milliseconds.
    pub millis: u32,
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

/// Clients connected with OAuth to MCP servers shared from this computer, newest first.
#[tauri::command]
#[specta::specta]
pub async fn mcp_connections(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<Vec<teitunnel_core::mcp_auth::McpConnection>, AppError> {
    teitunnel_core::mcp_auth::connections(&state.store, None)
        .await
        .map_err(|e| teitunnel_core::Error::from(e).into())
}

/// Disconnects a client from a shared MCP server: its tokens stop working at once in
/// the processes sharing it (they hear it on the control connection; one that can't
/// notices within seconds).
#[tauri::command]
#[specta::specta]
pub async fn mcp_disconnect(
    state: tauri::State<'_, crate::state::AppState>,
    id: String,
) -> Result<(), AppError> {
    teitunnel_core::mcp_auth::disconnect(&state.store, &id)
        .await
        .map_err(teitunnel_core::Error::from)?;
    state
        .control
        .host
        .publish(teitunnel_core::mcp_auth::disconnected_event(&id));
    Ok(())
}

/// Every client, and whether Teitunnel can connect them (it needs its command line tool).
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct AiClientsView {
    /// The command clients would run, when there is one.
    pub command: Option<String>,
    /// The clients: installed or configured ones first.
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

/// What connecting a client writes: this Teitunnel's `mcp` command.
fn server() -> Option<ServerCommand> {
    command().map(|command| ServerCommand {
        command: command.display().to_string(),
        args: vec!["mcp".to_owned()],
        env: std::collections::BTreeMap::new(),
    })
}

fn view(seen: &[SeenAgent]) -> AiClientsView {
    let server = server();
    // What an entry is compared with when there's no command line tool to connect.
    let expected = server.clone().unwrap_or_else(|| ServerCommand {
        command: String::new(),
        args: vec!["mcp".to_owned()],
        env: std::collections::BTreeMap::new(),
    });
    let mut clients: Vec<AiClientView> = Paths::detect()
        .map(|paths| {
            Client::ALL
                .iter()
                .map(|client| {
                    let status = clients::status(*client, &paths);
                    let last_used_at = seen
                        .iter()
                        .filter(|a| Client::from_client_info(&a.name) == Some(*client))
                        .map(|a| a.last_seen_at)
                        .max();
                    AiClientView {
                        id: client.id().to_owned(),
                        name: client.name().to_owned(),
                        state: status.state(&expected).into(),
                        path: status.path.display().to_string(),
                        installed_at: status.installed_at.map(|p| p.display().to_string()),
                        command: status.command,
                        last_used_at,
                        snippet: server.as_ref().map(|s| client.snippet(s)),
                        problem: status.problem,
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    // Installed or configured first, then the rest, keeping the order otherwise.
    clients.sort_by_key(|c| c.state == AiClientState::NotInstalled);
    AiClientsView {
        command: command().map(|p| p.display().to_string()),
        clients,
    }
}

async fn current(state: &crate::state::AppState) -> Result<AiClientsView, AppError> {
    let seen = agents_seen::list(&state.store).await.unwrap_or_default();
    super::off_main(move || view(&seen)).await
}

fn client(id: &str) -> Result<Client, AppError> {
    Client::parse(id).ok_or_else(|| AppError::invalid("client", m::unknown()))
}

fn failed(err: &clients::ClientError) -> AppError {
    tracing::warn!(error = %err, "AI client configuration");
    AppError::internal(m::failed(err.to_string()))
}

/// The AI clients on this computer: installed, connected and last used.
#[tauri::command]
#[specta::specta]
pub async fn ai_clients_status(
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<AiClientsView, AppError> {
    current(&state).await
}

/// Connects an AI client (or updates its entry): adds Teitunnel to its MCP configuration
/// (merged, with a backup). Only installed clients: connecting one that isn't would
/// create its folders and look like an install.
#[tauri::command]
#[specta::specta]
pub async fn ai_clients_connect(
    state: tauri::State<'_, crate::state::AppState>,
    client_id: String,
) -> Result<AiClientsView, AppError> {
    super::off_main(move || connect(&client_id)).await??;
    current(&state).await
}

fn connect(client_id: &str) -> Result<(), AppError> {
    let client = client(client_id)?;
    let server = server().ok_or_else(|| AppError::internal(m::no_cli()))?;
    let paths = Paths::detect().map_err(|e| failed(&e))?;
    if !client.detected(&paths) {
        return Err(AppError::invalid("client", m::not_installed(client.name())));
    }
    clients::install(client, &server, &paths).map_err(|e| failed(&e))?;
    Ok(())
}

/// Disconnects an AI client: removes Teitunnel from its MCP configuration.
#[tauri::command]
#[specta::specta]
pub async fn ai_clients_disconnect(
    state: tauri::State<'_, crate::state::AppState>,
    client_id: String,
) -> Result<AiClientsView, AppError> {
    super::off_main(move || disconnect(&client_id)).await??;
    current(&state).await
}

fn disconnect(client_id: &str) -> Result<(), AppError> {
    let client = client(client_id)?;
    let paths = Paths::detect().map_err(|e| failed(&e))?;
    clients::uninstall(client, &paths).map_err(|e| failed(&e))?;
    Ok(())
}

/// Starts the MCP server exactly as the client's configuration says (or as connecting
/// it would) and checks it answers: the handshake and its tool list, then it's stopped.
#[tauri::command]
#[specta::specta]
pub async fn ai_clients_test(client_id: String) -> Result<AiClientCheck, AppError> {
    let client = client(&client_id)?;
    let paths = Paths::detect().map_err(|e| failed(&e))?;
    let status = super::off_main(move || clients::status(client, &paths)).await?;
    let (program, args) = match status.program {
        Some(program) => (program, status.args),
        None => {
            let server = server().ok_or_else(|| AppError::internal(m::no_cli()))?;
            (PathBuf::from(server.command), server.args)
        }
    };
    let checked = check::check(
        &program,
        &args,
        &std::collections::BTreeMap::new(),
        check::TIMEOUT,
    )
    .await
    .map_err(|e| AppError::internal(m::check_failed(e.to_string())))?;
    Ok(AiClientCheck {
        server: format!("{} {}", checked.server, checked.version)
            .trim()
            .to_owned(),
        protocol: checked.protocol,
        tools: checked.tools,
        millis: checked.millis,
    })
}

/// Shows the client's MCP configuration file in Finder, Explorer or the file manager (its
/// folder when the file doesn't exist yet). The path is the client's own, never one the
/// page chose.
#[tauri::command]
#[specta::specta]
pub async fn ai_clients_reveal(app: tauri::AppHandle, client_id: String) -> Result<(), AppError> {
    use tauri_plugin_opener::OpenerExt;
    let client = client(&client_id)?;
    let paths = Paths::detect().map_err(|e| failed(&e))?;
    let path = client.config_path(&paths);
    let target = if path.exists() {
        path
    } else {
        path.parent()
            .filter(|p| p.is_dir())
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| AppError::invalid("client", m::no_config(client.name())))?
    };
    app.opener()
        .reveal_item_in_dir(&target)
        .map_err(|e| AppError::internal(m::failed(e.to_string())))
}
