//! Reservations: the account's reserved hostnames, and a hostname's
//! availability while it's typed. Reserving and releasing are route changes
//! (`Change::ReserveHostname`, `Change::ReleaseHostname`) through `routes_preview` and
//! `routes_apply`.

use tauri::State;
use teitunnel_core::reservations::{self, Availability, Reservations};

use crate::{error::AppError, state::AppState};

/// The account's reserved hostnames and who holds them (the cache when offline).
#[tauri::command]
#[specta::specta]
pub async fn reservations_list(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Reservations, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(reservations::list(&state.engine, &api, &account_id).await?)
}

/// Whether `hostname` is free, yours, or held by someone else (one DNS read).
#[tauri::command]
#[specta::specta]
pub async fn reservations_availability(
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
) -> Result<Availability, AppError> {
    let api = state.accounts.client(&account_id).await?;
    Ok(reservations::availability(&state.engine, &api, &account_id, &hostname).await?)
}
