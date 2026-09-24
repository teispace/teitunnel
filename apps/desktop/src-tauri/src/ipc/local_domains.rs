//! Local HTTPS domains (`https://shop.test`): the list and its status, adding and
//! removing, trust, and the administrator steps. The logic is
//! `teitunnel_core::local_domains`; nothing secret crosses here (the CA's key stays in
//! the keychain; only its public certificate is saved on request).

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;
use teitunnel_core::local_domains::{
    AdminTask, LocalDomainFix, LocalDomainInput, LocalDomainView, LocalDomainsStatus, TrustOptions,
    TrustView,
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn changed(app: &AppHandle) {
    let _ = EntityChanged {
        kind: EntityKind::LocalDomains,
        id: None,
    }
    .emit(app);
}

/// The domains, the listeners, `.test` names and the CA (no prompts).
#[tauri::command]
#[specta::specta]
pub async fn local_domains_status(
    state: State<'_, AppState>,
) -> Result<LocalDomainsStatus, AppError> {
    Ok(state.local_domains.status().await)
}

/// Adds a local domain and serves it.
#[tauri::command]
#[specta::specta]
pub async fn local_domains_add(
    app: AppHandle,
    state: State<'_, AppState>,
    input: LocalDomainInput,
) -> Result<LocalDomainView, AppError> {
    let view = state.local_domains.add(&input).await?;
    changed(&app);
    Ok(view)
}

/// Changes a local domain's service, subdomains, HTTPS or inspection.
#[tauri::command]
#[specta::specta]
pub async fn local_domains_update(
    app: AppHandle,
    state: State<'_, AppState>,
    input: LocalDomainInput,
) -> Result<LocalDomainView, AppError> {
    let view = state.local_domains.update(&input).await?;
    changed(&app);
    Ok(view)
}

/// Records a local domain's requests in the inspector, or stops.
#[tauri::command]
#[specta::specta]
pub async fn local_domains_set_inspect(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
    inspect: bool,
) -> Result<LocalDomainView, AppError> {
    let view = state.local_domains.set_inspect(&name, inspect).await?;
    changed(&app);
    for kind in [EntityKind::LocalDomains, EntityKind::Inspector] {
        let _ = EntityChanged { kind, id: None }.emit(&app);
    }
    Ok(view)
}

/// Removes a local domain.
#[tauri::command]
#[specta::specta]
pub async fn local_domains_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<(), AppError> {
    state.local_domains.remove(&name).await?;
    changed(&app);
    Ok(())
}

/// Lets phones and computers on the network open `.local` names, or stops that.
#[tauri::command]
#[specta::specta]
pub async fn local_domains_set_lan(
    app: AppHandle,
    state: State<'_, AppState>,
    lan: bool,
) -> Result<(), AppError> {
    state.local_domains.set_lan(lan).await?;
    changed(&app);
    Ok(())
}

/// Starts serving again (after freeing a port, for example).
#[tauri::command]
#[specta::specta]
pub async fn local_domains_restart(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<LocalDomainsStatus, AppError> {
    // A failure to serve is shown in the status.
    let _ = state.local_domains.restart().await;
    changed(&app);
    Ok(state.local_domains.status().await)
}

/// Where the local CA is trusted (runs the system's tools; no prompts).
#[tauri::command]
#[specta::specta]
pub async fn local_domains_trust_status(state: State<'_, AppState>) -> Result<TrustView, AppError> {
    Ok(state.local_domains.trust_status().await)
}

/// Trusts the local CA. The system asks for a password (macOS) or to confirm (Windows).
#[tauri::command]
#[specta::specta]
pub async fn local_domains_trust(
    app: AppHandle,
    state: State<'_, AppState>,
    options: TrustOptions,
) -> Result<TrustView, AppError> {
    let view = state.local_domains.trust(options).await?;
    changed(&app);
    Ok(view)
}

/// Stops trusting the local CA; with `forget`, deletes it too.
#[tauri::command]
#[specta::specta]
pub async fn local_domains_untrust(
    app: AppHandle,
    state: State<'_, AppState>,
    forget: bool,
) -> Result<TrustView, AppError> {
    let view = state.local_domains.untrust(forget).await?;
    changed(&app);
    Ok(view)
}

/// Runs a step that needs an administrator through the system's dialog (Linux).
#[tauri::command]
#[specta::specta]
pub async fn local_domains_run_as_admin(
    app: AppHandle,
    state: State<'_, AppState>,
    task: AdminTask,
) -> Result<(), AppError> {
    state.local_domains.run_as_admin(task).await?;
    changed(&app);
    Ok(())
}

/// Applies a Doctor fix for local domains.
#[tauri::command]
#[specta::specta]
pub async fn local_domains_fix(
    app: AppHandle,
    state: State<'_, AppState>,
    action: LocalDomainFix,
) -> Result<(), AppError> {
    state.local_domains.fix(action).await?;
    changed(&app);
    Ok(())
}

/// How to save the CA certificate for another device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub enum CaFormat {
    /// An Apple configuration profile (iPhone, iPad).
    AppleProfile,
    /// A PEM certificate (Android, other computers).
    Certificate,
}

/// Saves the local CA's certificate (never its key) where the person chooses, to install
/// on a phone. `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn local_domains_save_ca(
    app: AppHandle,
    state: State<'_, AppState>,
    format: CaFormat,
) -> Result<Option<String>, AppError> {
    let (bytes, name, filter, extension) = match format {
        CaFormat::AppleProfile => (
            state.local_domains.ca_profile().await?,
            "Teitunnel Local CA.mobileconfig",
            "Configuration profile",
            "mobileconfig",
        ),
        CaFormat::Certificate => (
            state.local_domains.ca_certificate().await?.into_bytes(),
            "Teitunnel Local CA.crt",
            "Certificate",
            "crt",
        ),
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(name)
        .add_filter(filter, &[extension])
        .save_file(move |file| {
            let _ = tx.send(file.and_then(|f| f.into_path().ok()));
        });
    let Some(path) = rx.await.ok().flatten() else {
        return Ok(None);
    };
    std::fs::write(&path, bytes).map_err(teitunnel_core::local_domains::LocalDomainError::from)?;
    Ok(Some(path.display().to_string()))
}
