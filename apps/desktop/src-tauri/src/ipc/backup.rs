//! Settings ▸ General ▸ Move to Another Computer: an encrypted backup of Teitunnel's
//! setup, and restoring one (the logic is `teitunnel_core::backup`). The passphrase only
//! travels from the UI to here; nothing secret goes back.

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;
use teitunnel_core::{
    Secret,
    backup::{self, BackupSummary, KdfParams},
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

/// A backup read and checked, waiting for the user to restore it.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BackupPreview {
    /// Pass to `backup_restore`.
    pub id: String,
    /// What it holds and what it would replace.
    pub summary: BackupSummary,
}

/// Asks where to save a backup (the system's save panel). `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn backup_choose_save(app: AppHandle) -> Result<Option<String>, AppError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(format!("Teitunnel Setup.{}", backup::EXTENSION))
        .add_filter("Teitunnel backup", &[backup::EXTENSION])
        .save_file(move |file| {
            let _ = tx.send(file.and_then(|f| f.into_path().ok()));
        });
    Ok(rx.await.ok().flatten().map(|p| p.display().to_string()))
}

/// Asks for a backup to restore (the system's open panel). `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn backup_choose_open(app: AppHandle) -> Result<Option<String>, AppError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Teitunnel backup", &[backup::EXTENSION])
        .pick_file(move |file| {
            let _ = tx.send(file.and_then(|f| f.into_path().ok()));
        });
    Ok(rx.await.ok().flatten().map(|p| p.display().to_string()))
}

/// Writes an encrypted backup of this Mac's setup to `path` (no tokens or passwords).
#[tauri::command]
#[specta::specta]
pub async fn backup_create(
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<(), AppError> {
    backup::create_file(
        &state.store,
        &state.machine_name,
        std::path::Path::new(&path),
        Secret::new(passphrase),
        KdfParams::default(),
    )
    .await?;
    Ok(())
}

/// Reads and decrypts a backup and says what restoring it would bring and replace.
/// Nothing changes until `backup_restore`.
#[tauri::command]
#[specta::specta]
pub async fn backup_inspect(
    state: State<'_, AppState>,
    path: String,
    passphrase: String,
) -> Result<BackupPreview, AppError> {
    let contents = backup::read_file(std::path::Path::new(&path), Secret::new(passphrase)).await?;
    let summary = backup::summarize(&state.store, &contents).await?;
    // The backup's creation time and machine identify it well enough: only the one shown
    // can be restored.
    let id = format!("{}-{}", summary.created_at, summary.machine);
    state
        .pending_restore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .replace((id.clone(), contents));
    Ok(BackupPreview { id, summary })
}

/// Restores the backup `backup_inspect` read (the one shown to the user).
#[tauri::command]
#[specta::specta]
pub async fn backup_restore(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), AppError> {
    let pending = state
        .pending_restore
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take_if(|(pending, _)| *pending == id)
        .map(|(_, contents)| contents);
    let contents = pending.ok_or_else(|| {
        AppError::invalid("file", teitunnel_core::text::msg::backup::not_pending())
    })?;
    backup::restore(&state.store, contents).await?;
    for kind in [
        EntityKind::Settings,
        EntityKind::Accounts,
        EntityKind::Routes,
        EntityKind::Snapshots,
        EntityKind::Projects,
    ] {
        let _ = EntityChanged { kind, id: None }.emit(&app);
    }
    Ok(())
}
