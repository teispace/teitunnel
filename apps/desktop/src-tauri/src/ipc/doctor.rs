//! Doctor: run every check. Fixes go through the routes commands (plan → apply) or the
//! existing binary/connector commands, so nothing here changes anything.

use tauri::State;
use teitunnel_core::doctor::{self, FixReport, Issue};
use teitunnel_core::text::msg::app as m;

use crate::{error::AppError, state::AppState};

/// Checks cloudflared and every connected account; issues sorted by severity.
#[tauri::command]
#[specta::specta]
pub async fn doctor_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<Issue>, AppError> {
    let issues = doctor::run(
        &state.accounts,
        &state.engine,
        &state.machine,
        &state.binary,
        &state.machine_name,
    )
    .await;
    crate::bootstrap::doctor_ran(&app, &state, &issues).await;
    Ok(issues)
}

/// Ignores (or stops ignoring) Doctor issues by id. Ignored issues are hidden and never
/// notify. Every window is told the settings changed.
#[tauri::command]
#[specta::specta]
pub async fn doctor_set_ignored(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    ids: Vec<String>,
    ignored: bool,
) -> Result<teitunnel_core::settings::Settings, AppError> {
    use tauri_specta::Event;
    let updated = teitunnel_core::settings::set_ignored(&state.store, ids, ignored).await?;
    crate::ipc::events::EntityChanged {
        kind: crate::ipc::events::EntityKind::Settings,
        id: None,
    }
    .emit(&app)?;
    Ok(updated)
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

/// Everything the diagnostics bundle contains, redacted.
async fn bundle(
    app: &tauri::AppHandle,
    state: &AppState,
) -> Result<Vec<teitunnel_core::diagnostics::BundleFile>, AppError> {
    use tauri::Manager;
    use teitunnel_core::diagnostics::{Inputs, build};
    let binary = match state.binary.current().await {
        Ok(status) => format!(
            "cloudflared {} ({:?}) at {}",
            status
                .version
                .map_or_else(|| "unknown version".to_owned(), |v| v.to_string()),
            status.source,
            status.path.display()
        ),
        Err(_) => "cloudflared not installed".to_owned(),
    };
    let accounts = state.accounts.list().await.unwrap_or_default();
    let mut summary = vec![
        format!("Teitunnel {}", env!("CARGO_PKG_VERSION")),
        teitunnel_core::platform::os_description(),
        binary,
        format!("Accounts: {}", accounts.len()),
    ];
    for account in &accounts {
        let tunnels = state
            .engine
            .local()
            .tunnels(&account.id)
            .await
            .unwrap_or_default();
        summary.push(format!(
            "  {:?} account, tunnels on this machine: {}",
            account.credential,
            if tunnels.is_empty() {
                "none".to_owned()
            } else {
                tunnels
                    .iter()
                    .map(|t| t.tunnel_id.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            }
        ));
    }
    let issues = doctor::run(
        &state.accounts,
        &state.engine,
        &state.machine,
        &state.binary,
        &state.machine_name,
    )
    .await;
    let settings = teitunnel_core::settings::load(&state.store).await?;
    let inputs = Inputs {
        summary,
        issues,
        settings: serde_json::to_value(settings).unwrap_or_default(),
        log_dir: app.path().app_log_dir().ok(),
    };
    tauri::async_runtime::spawn_blocking(move || build(&inputs))
        .await
        .map_err(|_| AppError::internal(m::interrupted()))
}

/// What a diagnostics export would contain (shown before saving).
#[tauri::command]
#[specta::specta]
pub async fn diagnostics_preview(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<teitunnel_core::diagnostics::FileSummary>, AppError> {
    let files = bundle(&app, &state).await?;
    Ok(teitunnel_core::diagnostics::summarize(&files))
}

/// Saves the diagnostics bundle to Downloads and shows it in Finder. Returns its path.
#[tauri::command]
#[specta::specta]
pub async fn diagnostics_export(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<String, AppError> {
    let files = bundle(&app, &state).await?;
    crate::ipc::app::save_to_downloads(
        &app,
        &teitunnel_core::diagnostics::file_name(),
        move |path| teitunnel_core::diagnostics::write(&files, path),
    )
    .await
}
