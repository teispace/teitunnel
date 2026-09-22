//! Doctor: run every check. Fixes go through the routes commands (plan → apply) or the
//! existing binary/connector commands, so nothing here changes anything.

use tauri::State;
use teitunnel_core::doctor::{self, FixReport, Issue};

use crate::{error::AppError, state::AppState};

/// Checks cloudflared and every connected account; issues sorted by severity.
#[tauri::command]
#[specta::specta]
pub async fn doctor_run(state: State<'_, AppState>) -> Result<Vec<Issue>, AppError> {
    Ok(doctor::run(
        &state.accounts,
        &state.engine,
        &state.machine,
        &state.binary,
        &state.machine_name,
    )
    .await)
}

/// Applies every fix that needs no review (owned DNS repairs and orphan cleanup), each
/// through a fresh plan; the rest are left for the user.
#[tauri::command]
#[specta::specta]
pub async fn doctor_fix_safe(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<FixReport, AppError> {
    use tauri_specta::Event;
    let report = doctor::fix_all_safe(
        &state.accounts,
        &state.engine,
        &state.machine,
        &state.binary,
        &state.machine_name,
    )
    .await;
    let _ = crate::ipc::EntityChanged {
        kind: crate::ipc::EntityKind::Routes,
        id: None,
    }
    .emit(&app);
    Ok(report)
}
