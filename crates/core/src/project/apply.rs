//! Applying a reviewed project plan, the same way for the app and the CLI: its routes
//! one by one (each re-planned and checked against what was shown), then its Snapshots.
//! Shares are started by the caller, which owns them (the app, or a terminal).

use std::path::Path;

use serde::Serialize;

use super::{
    ProjectError, ProjectPlan, SnapshotAction, SnapshotDecl, SnapshotSourceDecl, apply_route,
    registry::{self, CreatedRoute},
    resolve_secret, snapshot_change,
};
use crate::{
    engine::{Approval, Change, CloudApi, Connectors, Context, Engine, Outcome, Progress},
    secrets::SecretStore,
    snapshot::{self, Preparations, SnapshotError, build},
    store::Store,
    text::Text,
};

/// A route change that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteFailure {
    /// The route.
    pub hostname: String,
    /// Why.
    pub error: Text,
    /// What couldn't be undone (empty: everything was).
    pub leftovers: Vec<Text>,
}

/// The routes applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RoutesApplied {
    /// Routes the project created (recorded, for `down --remove-routes`).
    pub created: Vec<CreatedRoute>,
    /// Connectors that couldn't be started (the routes are configured).
    pub notes: Vec<Text>,
    /// The change that failed (the ones after it weren't applied).
    pub failure: Option<RouteFailure>,
}

/// Applies the plan's route changes in order, stopping at the first failure, and records
/// the routes the project created. `progress` gets the route's index and the engine's
/// progress for its steps.
///
/// # Errors
/// [`ProjectError::Changed`] when a route's plan no longer matches the one shown;
/// engine and database errors.
#[allow(clippy::too_many_arguments)]
pub async fn apply_routes<C, K, P>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    store: &Store,
    plan: &ProjectPlan,
    confirmed: bool,
    mut progress: P,
) -> Result<RoutesApplied, ProjectError>
where
    C: CloudApi,
    K: Connectors,
    P: FnMut(usize, Progress) + Send,
{
    let mut applied = RoutesApplied {
        created: Vec::new(),
        notes: Vec::new(),
        failure: None,
    };
    for (index, action) in plan.routes.iter().enumerate() {
        let outcome = apply_route(engine, api, connectors, ctx, action, confirmed, |p| {
            progress(index, p);
        })
        .await?;
        match outcome {
            Outcome::Applied {
                connector_error, ..
            } => {
                applied.notes.extend(connector_error);
                if matches!(action.change, Change::AddRoute { .. }) {
                    applied.created.push(CreatedRoute {
                        account_id: ctx.account.to_owned(),
                        hostname: action.hostname.clone(),
                        path: action.path.clone(),
                    });
                }
            }
            Outcome::RolledBack { error, .. } => {
                applied.failure = Some(RouteFailure {
                    hostname: action.hostname.clone(),
                    error,
                    leftovers: Vec::new(),
                });
                break;
            }
            Outcome::PartiallyApplied {
                error, leftovers, ..
            } => {
                applied.failure = Some(RouteFailure {
                    hostname: action.hostname.clone(),
                    error,
                    leftovers,
                });
                break;
            }
        }
    }
    registry::applied(store, &plan.path, &plan.name, applied.created.clone()).await?;
    Ok(applied)
}

/// What publishing a declared Snapshot did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "result", rename_all = "camelCase")]
pub enum SnapshotResult {
    /// Published (a new Snapshot or a new version).
    Published,
    /// Its files and settings are the live version's already.
    UpToDate,
    /// It would replace a DNS record Teitunnel didn't create; not done.
    NeedsConfirmation,
    /// It failed (and was undone).
    Failed {
        /// Why.
        error: Text,
    },
}

/// Collects a declared Snapshot's files (building the project first for a `build`
/// source: the plan showed the command), then publishes them if anything changed.
///
/// # Errors
/// The files couldn't be collected or built, the password reference can't be read, or
/// engine errors.
#[allow(clippy::too_many_arguments)]
pub async fn publish_snapshot<C, K>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    keychain: &dyn SecretStore,
    action: &SnapshotAction,
    decl: &SnapshotDecl,
    confirmed: bool,
    build_output: impl FnMut(&str) + Send,
) -> Result<SnapshotResult, ProjectError>
where
    C: CloudApi,
    K: Connectors,
{
    let preparations = Preparations::default();
    let prepared = match &action.source {
        SnapshotSourceDecl::Folder(path) => preparations.folder(Path::new(path)).await?,
        SnapshotSourceDecl::Build(path) => {
            let project = build::detect(Path::new(path))?;
            preparations.build(&project, build_output).await?
        }
    };
    let existing = engine
        .local()
        .sites(Some(ctx.account))
        .await?
        .into_iter()
        .find(|s| s.name.eq_ignore_ascii_case(&action.name));
    let needs_password = existing.as_ref().is_none_or(|row| !row.password);
    let password = match &decl.password {
        Some(reference) if needs_password => Some(resolve_secret(reference, keychain)?),
        _ => None,
    };
    let change = snapshot_change(
        decl,
        action.hostname.as_deref(),
        prepared.id,
        existing.as_ref(),
        password,
    );
    let plan = match snapshot::preview(engine, api, &preparations, ctx, &change).await {
        Err(SnapshotError::Unchanged) => return Ok(SnapshotResult::UpToDate),
        other => other?,
    };
    if plan.requires_confirmation && !confirmed {
        return Ok(SnapshotResult::NeedsConfirmation);
    }
    let outcome = snapshot::apply(
        engine,
        api,
        connectors,
        &preparations,
        ctx,
        "project",
        &change,
        Approval {
            fingerprint: &plan.fingerprint,
            confirmed,
        },
        |_| {},
    )
    .await?;
    Ok(match outcome {
        Outcome::Applied { .. } => SnapshotResult::Published,
        Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
            SnapshotResult::Failed { error }
        }
    })
}

/// The build command a Snapshot's project runs (for the plan), if it needs building.
pub fn build_command(dir: &str) -> Option<String> {
    let project = build::detect(Path::new(dir)).ok()?;
    build::BuildCommand::for_project(&project)
        .ok()
        .flatten()
        .map(|c| c.display())
}

/// A failure's message for logs and the CLI.
pub fn failure_text(failure: &RouteFailure) -> String {
    format!("{}: {}", failure.hostname, failure.error.english())
}
