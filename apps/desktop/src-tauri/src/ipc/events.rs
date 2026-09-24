//! Typed events emitted to the webview.

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

/// Kinds of entities the UI caches. Each maps to TanStack Query keys on the frontend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum EntityKind {
    /// App settings.
    Settings,
    /// Quick Shares (list, status, URL).
    QuickShares,
    /// Connected Cloudflare accounts (and their domains).
    Accounts,
    /// Routes and this Mac's tunnel (id: the account).
    Routes,
    /// App updates.
    Updates,
    /// Snapshots (id: the account).
    Snapshots,
}

/// Emitted after anything changes, so the UI can invalidate the affected queries.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct EntityChanged {
    /// What kind of entity changed.
    pub kind: EntityKind,
    /// Which one, when a single entity changed.
    pub id: Option<String>,
}

/// A menu-bar command the webview handles (navigation, panes, palette).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum MenuCommand {
    /// File ▸ New Route (⌘N).
    NewRoute,
    /// File ▸ New Quick Share (⇧⌘N).
    NewQuickShare,
    /// View ▸ Toggle Sidebar (⌥⌘S).
    ToggleSidebar,
    /// View ▸ Toggle Inspector (⌥⌘I).
    ToggleInspector,
    /// View ▸ Refresh (⌘R).
    Refresh,
    /// View ▸ Command Palette (⌘K).
    CommandPalette,
    /// View ▸ Overview (⌘1).
    GoOverview,
    /// View ▸ Routes (⌘2).
    GoRoutes,
    /// View ▸ Quick Share (⌘3).
    GoQuickShare,
    /// View ▸ Snapshots (⌘4).
    GoSnapshots,
    /// View ▸ Domains (⌘5).
    GoDomains,
    /// View ▸ Tunnels (⌘6).
    GoTunnels,
    /// View ▸ Activity (⌘7).
    GoActivity,
    /// View ▸ Doctor (⌘8).
    GoDoctor,
    /// View ▸ Analytics (⌘9).
    GoAnalytics,
    /// Quit was chosen while routes run through the app: ask what to do.
    ConfirmQuit,
    /// Help ▸ Export Diagnostics…
    ExportDiagnostics,
}

/// Emitted when a menu-bar item that the webview handles is chosen.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct MenuAction {
    /// The chosen command.
    pub command: MenuCommand,
}
