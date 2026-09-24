//! Teitunnel's MCP (Model Context Protocol) server: AI agents (Claude Code, Cursor, VS
//! Code, Codex, Windsurf, Zed, Claude Desktop, Gemini CLI…) share local services, manage
//! routes through reviewed plans, diagnose problems and inspect traffic, with the same
//! engine and safety rules as the app.
//!
//! - [`McpServer`] implements the protocol (tools, resources, prompts, progress,
//!   logging, completions, elicitation for approvals) over a [`Backend`].
//! - [`CoreBackend`] is the backend over `teitunnel-core`, hosted by `teitunnel mcp`
//!   (stdio) and `teitunnel serve` (Streamable HTTP at `/mcp`, see [`http`]).
//! - Tools come from [`ToolProvider`]s: Teitunnel's own ([`tools::CoreTools`]) plus any
//!   added with [`ServerBuilder::provider`].
//! - Captured traffic comes from a [`TrafficSource`] (the inspector); [`NoTraffic`]
//!   stands in until it runs.
//! - [`clients`] writes the MCP configuration of AI clients.
//!
//! Safety: see [`Mode`]. Secrets never reach agents; every change is recorded in
//! Activity with the agent's name.

pub mod backend;
pub mod clients;
pub mod config;
mod core_backend;
pub mod expose;
pub mod http;
mod inspector_traffic;
pub mod limits;
pub mod plans;
mod prompts;
#[cfg(test)]
mod protocol_tests;
pub mod redaction;
pub mod registry;
pub mod reservations;
mod resources;
mod server;
pub mod tools;
pub mod traffic;

pub use backend::{Backend, BackendError, ChangeEvent, SharedBackend};
pub use config::{Mode, Settings};
pub use core_backend::{ConnectorSource, CoreBackend, CoreParts};
pub use expose::ExposeTools;
pub use inspector_traffic::InspectorTraffic;
pub use registry::{
    Approval, ApprovalRequest, Approver, ToolClass, ToolContext, ToolError, ToolOutput,
    ToolProvider, ToolResult, ToolSpec,
};
pub use server::{HttpIdentity, INSTRUCTIONS, McpServer, ServerBuilder};
pub use traffic::{NoTraffic, TrafficSource};

/// Serves `server` over stdio until the client disconnects (or `stop` is cancelled).
///
/// # Errors
/// The transport failed to start.
pub async fn serve_stdio(
    server: McpServer,
    stop: tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    use rmcp::ServiceExt as _;
    let running = server
        .serve_with_ct(rmcp::transport::stdio(), stop)
        .await
        .map_err(|e| format!("Couldn't start the MCP server: {e}"))?;
    running
        .waiting()
        .await
        .map_err(|e| format!("The MCP server stopped: {e}"))?;
    Ok(())
}
