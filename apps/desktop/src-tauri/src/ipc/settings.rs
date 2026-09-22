//! Settings commands.

use tauri::{AppHandle, State};
use tauri_specta::Event;
use teitunnel_core::settings::{self, Settings, SettingsPatch};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    shell,
    state::AppState,
};

/// Returns all settings, with defaults applied.
#[tauri::command]
#[specta::specta]
pub async fn settings_get(state: State<'_, AppState>) -> Result<Settings, AppError> {
    Ok(settings::load(&state.store).await?)
}

/// Updates the given settings and returns the result. Every window is notified.
#[tauri::command]
#[specta::specta]
pub async fn settings_set(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<Settings, AppError> {
    let updated = settings::update(&state.store, patch).await?;
    shell::tray::set_visible(&app, updated.show_in_menu_bar);
    EntityChanged {
        kind: EntityKind::Settings,
        id: None,
    }
    .emit(&app)?;
    Ok(updated)
}
