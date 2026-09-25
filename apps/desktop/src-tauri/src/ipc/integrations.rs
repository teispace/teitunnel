//! Settings ▸ Integrations: the control connection, links and always-allowed programs.

use tauri::{AppHandle, State};
use tauri_specta::Event;
use teitunnel_core::control::integrations::{self, Integrations, IntegrationsPatch};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn changed(app: &AppHandle) -> Result<(), AppError> {
    EntityChanged {
        kind: EntityKind::Settings,
        id: None,
    }
    .emit(app)?;
    Ok(())
}

/// The Integrations settings.
#[tauri::command]
#[specta::specta]
pub async fn integrations_get(state: State<'_, AppState>) -> Result<Integrations, AppError> {
    Ok(integrations::load(&state.store).await?)
}

/// Turns the control connection or links on or off, or changes the global shortcut.
/// Turning the connection off closes every open connection. A shortcut the system
/// refuses (another app has it) isn't saved: the previous one stays.
#[tauri::command]
#[specta::specta]
pub async fn integrations_set(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: IntegrationsPatch,
) -> Result<Integrations, AppError> {
    let previous = integrations::load(&state.store).await?;
    let shortcut_changed = patch.shortcut.is_some();
    let updated = integrations::update(&state.store, patch).await?;
    if shortcut_changed && let Err(err) = crate::shell::shortcut::apply(&app, &updated.shortcut) {
        let restore = IntegrationsPatch {
            shortcut: Some(previous.shortcut.clone()),
            ..IntegrationsPatch::default()
        };
        integrations::update(&state.store, restore).await?;
        if let Err(again) = crate::shell::shortcut::apply(&app, &previous.shortcut) {
            tracing::warn!(%again, "couldn't restore the previous global shortcut");
        }
        return Err(err.into());
    }
    if updated.control_enabled {
        state.control.start();
    } else {
        state.control.stop();
    }
    changed(&app)?;
    Ok(updated)
}

/// Stops always allowing a program: its next change is asked about again.
#[tauri::command]
#[specta::specta]
pub async fn integrations_revoke(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<Integrations, AppError> {
    let updated = integrations::revoke(&state.store, &name).await?;
    changed(&app)?;
    Ok(updated)
}
