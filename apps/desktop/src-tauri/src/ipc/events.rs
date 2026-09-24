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
    /// Projects (teitunnel.yml files opened in the app).
    Projects,
    /// The inspector's taps and settings (captures stream on `inspect_subscribe`).
    Inspector,
    /// Local HTTPS domains, their listeners and trust.
    LocalDomains,
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

/// Where the main window should go (asked by the control connection or a link).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(
    tag = "view",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ViewTarget {
    /// The Overview.
    Overview,
    /// A route's sheet.
    Route {
        /// Its hostname.
        hostname: String,
    },
    /// Quick Share, optionally one share.
    Share {
        /// The share's id.
        id: Option<String>,
    },
    /// A share's request inspector.
    Inspector {
        /// The share's id.
        share: String,
    },
    /// The Doctor.
    Doctor,
}

/// Emitted when the window should show a view.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
#[serde(rename_all = "camelCase")]
pub struct OpenView {
    /// The view.
    pub target: ViewTarget,
}
