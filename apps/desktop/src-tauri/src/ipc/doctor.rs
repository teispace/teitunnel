//! Doctor: run every check. Fixes go through the routes commands (plan → apply) or the
//! existing binary/connector commands, so nothing here changes anything.

use tauri::State;
use teitunnel_core::doctor::{self, Issue};

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
