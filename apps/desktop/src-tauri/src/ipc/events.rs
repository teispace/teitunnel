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
