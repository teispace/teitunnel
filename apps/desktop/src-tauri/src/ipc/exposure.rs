//! The exposure check before something goes public (the logic is
//! `teitunnel_core::exposure`): the share and route sheets run it and show what it finds
//! with "Share Anyway".

use tauri::State;
use teitunnel_core::exposure::{self, ExposureReport};

use crate::{error::AppError, state::AppState};

/// Checks a local service for common leaks (a `.env` file, the git folder, debug pages,
/// open admin panels…), directly and within 2 seconds. `None` when the check is off in
/// Settings.
#[tauri::command]
#[specta::specta]
pub async fn exposure_check(
    state: State<'_, AppState>,
    origin: String,
) -> Result<Option<ExposureReport>, AppError> {
    if !teitunnel_core::settings::load(&state.store)
        .await?
        .exposure_check
    {
        return Ok(None);
    }
    Ok(Some(exposure::check(&origin).await))
}
