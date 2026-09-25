//! Projects: a `teitunnel.yml` in a folder, opened in the app (the logic is
//! `teitunnel_core::project`). Applying goes through the engine's plan → apply, after the
//! combined plan was shown; its shares belong to the app (they stop when it quits).

use std::time::Duration;

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Runtime, State};
use tauri_plugin_dialog::DialogExt;
use tauri_specta::Event;
use teitunnel_core::{
    accounts::Account,
    domain::OriginUrl,
    domain_shares::{self, APP_OWNER, ShareRequest},
    engine::{Context, Outcome},
    project::{
        self, Diagnostic, HostHeaderDecl, ProjectError, ProjectPlan, RoutesApplied, SnapshotResult,
        registry::{self, ProjectEntry},
    },
    quick_share::HostHeaderChoice,
    text::{Text, UserText as _},
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn changed<R: Runtime>(app: &AppHandle<R>) {
    for kind in [
        EntityKind::Projects,
        EntityKind::Routes,
        EntityKind::QuickShares,
        EntityKind::Snapshots,
    ] {
        let _ = EntityChanged { kind, id: None }.emit(app);
    }
    crate::bootstrap::refresh_tray_routes(app);
}

fn context<'a>(state: &'a AppState, account_id: &'a str) -> Context<'a> {
    Context {
        account: account_id,
        machine_name: &state.machine_name,
        tunnel: None,
    }
}

/// A project as the app shows it: its file's problems, and its plan when it has none.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectStatus {
    /// The project file.
    pub path: String,
    /// The project's name.
    pub name: String,
    /// Errors and warnings in the file, with their lines.
    pub diagnostics: Vec<Diagnostic>,
    /// What applying would do (with each item's state), when the file has no errors.
    pub plan: Option<ProjectPlan>,
    /// Why there's no plan (an account to choose, a placeholder without a value…).
    pub problem: Option<Text>,
    /// When the file last changed (ms since the epoch), to notice edits.
    pub modified_at: Option<f64>,
}

/// What applying did.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ProjectApplied {
    /// The routes.
    pub routes: RoutesApplied,
    /// Each Snapshot by name, and what happened.
    pub snapshots: Vec<(String, SnapshotResult)>,
    /// Shares started (their addresses).
    pub shares: Vec<String>,
    /// Shares that couldn't start, and why.
    pub share_errors: Vec<Text>,
}

fn modified_at(path: &str) -> Option<f64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let ms = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    #[allow(clippy::cast_precision_loss)] // milliseconds fit an f64 exactly until 2255
    Some(ms as f64)
}

/// The account a project applies to: the file's `account` (name or id), else the only
/// connected one.
async fn account_for(state: &AppState, wanted: Option<&str>) -> Result<Account, Text> {
    let accounts = state.accounts.list().await.map_err(|e| e.text())?;
    match wanted {
        Some(wanted) => accounts
            .into_iter()
            .find(|a| a.id == wanted || a.name.eq_ignore_ascii_case(wanted))
            .ok_or_else(|| teitunnel_core::text::msg::app::project_account(wanted)),
        None if accounts.len() == 1 => accounts
            .into_iter()
            .next()
            .ok_or_else(teitunnel_core::text::msg::app::project_no_account),
        None if accounts.is_empty() => Err(teitunnel_core::text::msg::app::project_no_account()),
        None => Err(teitunnel_core::text::msg::app::project_choose_account()),
    }
}

/// Projects this Mac knows, by name.
#[tauri::command]
#[specta::specta]
pub async fn projects_list(state: State<'_, AppState>) -> Result<Vec<ProjectEntry>, AppError> {
    Ok(registry::list(&state.store).await?)
}

/// Asks for a project's folder with the system's open panel. `None` when cancelled.
#[tauri::command]
#[specta::specta]
pub async fn projects_choose_folder(app: AppHandle) -> Result<Option<String>, AppError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder.and_then(|f| f.into_path().ok()));
    });
    Ok(rx
        .await
        .ok()
        .flatten()
        .map(|path| path.display().to_string()))
}

/// Opens a project (a folder with a `teitunnel.yml`, or the file) and remembers it.
#[tauri::command]
#[specta::specta]
pub async fn projects_add(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ProjectEntry, AppError> {
    let loaded = project::load(std::path::Path::new(&path))?;
    let file = loaded.path.display().to_string();
    registry::remember(&state.store, &file, &loaded.name).await?;
    changed(&app);
    registry::list(&state.store)
        .await?
        .into_iter()
        .find(|p| p.path == file)
        .ok_or_else(|| AppError::from(ProjectError::NotFound(file)))
}

/// Forgets a project (nothing it applied is changed).
#[tauri::command]
#[specta::specta]
pub async fn projects_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<(), AppError> {
    registry::forget(&state.store, &path).await?;
    changed(&app);
    Ok(())
}

/// When the project file last changed (a cheap check, polled to notice edits).
#[tauri::command]
#[specta::specta]
pub async fn projects_modified(path: String) -> Result<Option<f64>, AppError> {
    Ok(modified_at(&path))
}

/// Reads a project file and plans it: every declared item's state and what applying
/// would change. Nothing is changed.
#[tauri::command]
#[specta::specta]
pub async fn projects_status(
    state: State<'_, AppState>,
    path: String,
) -> Result<ProjectStatus, AppError> {
    let loaded = project::load(std::path::Path::new(&path))?;
    let mut status = ProjectStatus {
        path: loaded.path.display().to_string(),
        name: loaded.name.clone(),
        diagnostics: loaded.parsed.diagnostics.clone(),
        plan: None,
        problem: None,
        modified_at: modified_at(&path),
    };
    let Some(file) = loaded.parsed.file.as_ref() else {
        return Ok(status);
    };
    let account = match account_for(&state, file.account.as_deref()).await {
        Ok(account) => account,
        Err(problem) => {
            status.problem = Some(problem);
            return Ok(status);
        }
    };
    let api = state.accounts.client(&account.id).await?;
    let quick: Vec<String> = state
        .quick_shares
        .list()
        .into_iter()
        .map(|s| s.origin.to_string())
        .collect();
    match project::plan(
        &state.engine,
        &api,
        &state.machine,
        context(&state, &account.id),
        &loaded,
        &quick,
    )
    .await
    {
        Ok(plan) => status.plan = Some(plan),
        Err(err) => status.problem = Some(err.text()),
    }
    Ok(status)
}

fn host_header(decl: &HostHeaderDecl) -> HostHeaderChoice {
    match decl {
        HostHeaderDecl::Auto => HostHeaderChoice::Auto,
        HostHeaderDecl::Off => HostHeaderChoice::Off,
        HostHeaderDecl::Set(value) => HostHeaderChoice::Set {
            value: value.clone(),
        },
    }
}

/// Applies a reviewed project plan (by fingerprint: a plan that changed since is
/// refused): its routes, its Snapshots, then its shares, which run until stopped or
/// Teitunnel quits. `confirmed` allows replacing DNS records Teitunnel didn't create.
#[tauri::command]
#[specta::specta]
pub async fn projects_apply(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    fingerprint: String,
    confirmed: bool,
) -> Result<ProjectApplied, AppError> {
    let loaded = project::load(std::path::Path::new(&path))?;
    let file = loaded.file()?.clone();
    let account = account_for(&state, file.account.as_deref())
        .await
        .map_err(|t| AppError::invalid("account", t))?;
    let api = state.accounts.client(&account.id).await?;
    let ctx = context(&state, &account.id);
    let quick: Vec<String> = state
        .quick_shares
        .list()
        .into_iter()
        .map(|s| s.origin.to_string())
        .collect();
    let plan = project::plan(&state.engine, &api, &state.machine, ctx, &loaded, &quick).await?;
    if plan.fingerprint != fingerprint {
        return Err(ProjectError::Changed.into());
    }
    let routes = project::apply_routes(
        &state.engine,
        &api,
        &state.machine,
        ctx,
        &state.store,
        &plan,
        confirmed,
        |_, _| {},
    )
    .await;
    changed(&app);
    let routes = routes?;
    let mut applied = ProjectApplied {
        routes,
        snapshots: Vec::new(),
        shares: Vec::new(),
        share_errors: Vec::new(),
    };
    if applied.routes.failure.is_some() {
        return Ok(applied);
    }
    let resolved = project::resolve(&file, &loaded.vars)?;
    for (action, (decl, _)) in plan.snapshots.iter().zip(&resolved.snapshots) {
        let result = project::publish_snapshot(
            &state.engine,
            &api,
            &state.machine,
            ctx,
            state.secrets.as_ref(),
            action,
            decl,
            confirmed,
            |_| {},
        )
        .await
        .unwrap_or_else(|e| SnapshotResult::Failed { error: e.text() });
        applied.snapshots.push((action.name.clone(), result));
    }
    for share in &plan.shares {
        let started = match &share.hostname {
            None => match OriginUrl::parse(&share.origin) {
                Ok(origin) => state
                    .quick_shares
                    .start_with(
                        origin,
                        share
                            .expires_after
                            .map(|s| Duration::from_secs(u64::from(s))),
                        &host_header(&share.host_header),
                        Some(share.inspect),
                    )
                    .await
                    .map(|_| share.origin.clone())
                    .map_err(|e| e.text()),
                Err(err) => Err(err.text()),
            },
            Some(hostname) => {
                let resolved = host_header(&share.host_header)
                    .resolve(&share.origin)
                    .await
                    .ok()
                    .flatten()
                    .map(|h| h.value);
                let outcome = domain_shares::start(
                    &state.engine,
                    &api,
                    &state.machine,
                    ctx,
                    ShareRequest {
                        hostname,
                        origin: &share.origin,
                        access: share.login.clone(),
                        expires_at: share
                            .expires_after
                            .map(|s| domain_shares::now_ms() + u64::from(s) * 1000),
                        owner: APP_OWNER,
                        host_header: resolved,
                        source: None,
                        folder: false,
                    },
                )
                .await;
                match outcome {
                    Ok(Outcome::Applied { .. }) => Ok(format!("https://{hostname}")),
                    Ok(
                        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. },
                    ) => Err(error),
                    Err(err) => Err(teitunnel_core::Error::from(err).text()),
                }
            }
        };
        match started {
            Ok(address) => applied.shares.push(address),
            Err(error) => applied.share_errors.push(error),
        }
    }
    // Local domains: this computer only, served at once.
    if !plan.local_domains.is_empty() {
        project::apply_local_domains(&state.store, &plan).await?;
        if let Err(err) = state.local_domains.sync().await {
            tracing::warn!(%err, "a project's local domains couldn't be served");
        }
        let _ = crate::ipc::EntityChanged {
            kind: crate::ipc::EntityKind::LocalDomains,
            id: None,
        }
        .emit(&app);
    }
    changed(&app);
    Ok(applied)
}
