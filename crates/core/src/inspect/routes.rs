//! Inspecting a route (M12-02, decision 2): a temporary change through plan → apply that
//! points the route's service at a tap of this process's Lens, and remembers the
//! original in `inspected_routes`. It's reverted when inspection is turned off, when
//! the process that runs the tap stops (on quit; and swept at the next start after a
//! crash, like shares on your domain, D-068), and the Doctor's `inspect.orphan` check
//! restores a route left pointing at an inspector nobody runs.

use std::collections::HashMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{InspectError, Inspector, OriginTls, TapScope, TapSpec};
use crate::{
    accounts::Accounts,
    domain_shares::{APP_OWNER, now_ms},
    engine::{
        AccessRule, Approval, Change, CloudApi, Connectors, Context, Engine, EngineError, Outcome,
        PlanError, PlanView, Progress, RouteInput, RouteView,
    },
    runtime,
    store::{Store, StoreError},
    text::{Text, UserText},
};

/// A route pointed at an inspector, as remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InspectedRoute {
    /// Account id.
    pub account_id: String,
    /// Public hostname.
    pub hostname: String,
    /// Path rule.
    pub path: Option<String>,
    /// This machine's tunnel carrying it (`None`: the default one).
    pub tunnel_id: Option<String>,
    /// The service it had before, restored when inspection ends.
    pub original_origin: String,
    /// Its login when inspection started (kept as it is).
    pub access: Option<AccessRule>,
    /// The inspector's address the route points at.
    pub lens_url: String,
    /// [`APP_OWNER`], or the CLI process running the inspector.
    pub owner: String,
    /// When inspection started (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: u64,
}

impl InspectedRoute {
    /// Whether it should be reverted: its CLI owner exited.
    pub fn is_over(&self) -> bool {
        self.owner != APP_OWNER && !runtime::is_running(&self.owner)
    }

    /// The change that points the route back at its own service.
    pub fn restore_change(&self, access: Option<AccessRule>) -> Change {
        Change::UpdateRoute {
            hostname: self.hostname.clone(),
            path: self.path.clone(),
            route: RouteInput {
                hostname: self.hostname.clone(),
                path: self.path.clone(),
                origin: self.original_origin.clone(),
                access,
                options: None,
            },
        }
    }
}

/// A change to review before inspection starts or ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct InspectPlan {
    /// The change (an edit of the route's service).
    pub change: Change,
    /// The tunnel carrying the route.
    pub tunnel_id: Option<String>,
    /// The plan, as for any change.
    pub plan: PlanView,
}

/// Inspected routes, in `account` or everywhere, oldest first.
///
/// # Errors
/// The database can't be read.
pub async fn list(store: &Store, account: Option<&str>) -> Result<Vec<InspectedRoute>, StoreError> {
    let account = account.map(str::to_owned);
    store
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT account_id, hostname, path, tunnel_id, original_origin, access, lens_url,
                        owner, created_at
                 FROM inspected_routes WHERE ?1 IS NULL OR account_id = ?1
                 ORDER BY created_at, hostname",
            )?;
            let rows = stmt.query_map(params![account], |row| {
                let path: String = row.get(2)?;
                let access: Option<String> = row.get(5)?;
                Ok(InspectedRoute {
                    account_id: row.get(0)?,
                    hostname: row.get(1)?,
                    path: (!path.is_empty()).then_some(path),
                    tunnel_id: row.get(3)?,
                    original_origin: row.get(4)?,
                    access: access.and_then(|a| serde_json::from_str(&a).ok()),
                    lens_url: row.get(6)?,
                    owner: row.get(7)?,
                    created_at: u64::try_from(row.get::<_, i64>(8)?).unwrap_or_default(),
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

async fn record(store: &Store, route: &InspectedRoute) -> Result<(), StoreError> {
    let route = route.clone();
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO inspected_routes (account_id, hostname, path, tunnel_id,
                    original_origin, access, lens_url, owner, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT (account_id, hostname, path) DO UPDATE SET tunnel_id = ?4,
                    original_origin = ?5, access = ?6, lens_url = ?7, owner = ?8,
                    created_at = ?9",
                params![
                    route.account_id,
                    route.hostname,
                    route.path.unwrap_or_default(),
                    route.tunnel_id,
                    route.original_origin,
                    route
                        .access
                        .as_ref()
                        .and_then(|a| serde_json::to_string(a).ok()),
                    route.lens_url,
                    route.owner,
                    i64::try_from(route.created_at).unwrap_or(i64::MAX),
                ],
            )?;
            Ok(())
        })
        .await
}

/// Forgets an inspected route (without changing it).
///
/// # Errors
/// The database can't be written.
pub async fn forget(
    store: &Store,
    account: &str,
    hostname: &str,
    path: Option<&str>,
) -> Result<(), StoreError> {
    let (account, hostname, path) = (
        account.to_owned(),
        hostname.to_ascii_lowercase(),
        path.unwrap_or_default().to_owned(),
    );
    store
        .call(move |conn| {
            conn.execute(
                "DELETE FROM inspected_routes WHERE account_id = ?1 AND hostname = ?2 AND path = ?3",
                params![account, hostname, path],
            )?;
            Ok(())
        })
        .await
}

async fn find(
    store: &Store,
    account: &str,
    hostname: &str,
    path: Option<&str>,
) -> Result<Option<InspectedRoute>, StoreError> {
    let hostname = hostname.trim().to_ascii_lowercase();
    Ok(list(store, Some(account))
        .await?
        .into_iter()
        .find(|r| r.hostname == hostname && r.path.as_deref() == path.filter(|p| !p.is_empty())))
}

/// This machine's route at `hostname`/`path`.
async fn route_view<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    hostname: &str,
    path: Option<&str>,
) -> Result<Option<RouteView>, EngineError> {
    let hostname = hostname.trim().to_ascii_lowercase();
    let path = path.map(str::trim).filter(|p| !p.is_empty());
    let overview = engine.overview(api, connectors, ctx).await?;
    Ok(overview
        .routes
        .into_iter()
        .find(|r| r.hostname.eq_ignore_ascii_case(&hostname) && r.path.as_deref() == path))
}

fn update(route: &RouteView, origin: &str) -> Change {
    Change::UpdateRoute {
        hostname: route.hostname.clone(),
        path: route.path.clone(),
        route: RouteInput {
            hostname: route.hostname.clone(),
            path: route.path.clone(),
            origin: origin.to_owned(),
            access: route.access.clone(),
            options: None,
        },
    }
}

async fn preview<C: CloudApi>(
    engine: &Engine,
    api: &C,
    ctx: Context<'_>,
    change: &Change,
) -> Result<PlanView, EngineError> {
    let intent = engine.intent_for(api, ctx, change).await?;
    Ok(engine.preview(api, ctx, &intent).await?.view(ctx.account))
}

/// A running tap for the route (reused if there's one), and the route.
async fn prepare_on<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    inspector: &Inspector,
    hostname: &str,
    path: Option<&str>,
) -> Result<(RouteView, String), InspectError> {
    let base = Context {
        tunnel: None,
        ..ctx
    };
    let route = route_view(engine, api, connectors, base, hostname, path)
        .await?
        .ok_or_else(|| InspectError::NotRoute(hostname.to_owned()))?;
    if let Some(store) = inspector.store()
        && let Some(existing) = find(store, ctx.account, hostname, path).await?
        && existing.lens_url == route.origin
        && inspector.tap_for(&scope(ctx.account, &route)).is_some()
    {
        return Err(InspectError::Invalid(format!(
            "{} is already being inspected",
            route.hostname
        )));
    }
    if !(route.origin.starts_with("http://") || route.origin.starts_with("https://")) {
        return Err(InspectError::NotWeb);
    }
    let scope = scope(ctx.account, &route);
    let tap = match inspector.tap_for(&scope) {
        Some(tap) => tap,
        None => {
            let mut spec = TapSpec::new(scope, &route.hostname, &route.origin);
            spec.tls = OriginTls {
                verify: !route.options.no_tls_verify,
                server_name: route.options.origin_server_name.clone(),
                http2: false,
            };
            spec.public_url = Some(format!("https://{}", route.hostname));
            inspector.start(spec).await?.id
        }
    };
    let url = inspector.tap_url(&tap).ok_or(InspectError::UnknownTap)?;
    Ok((route, url))
}

fn scope(account: &str, route: &RouteView) -> TapScope {
    TapScope::route(account, &route.hostname, route.path.as_deref())
}

/// Starts a tap for the route and plans pointing the route at it, for review.
///
/// # Errors
/// Not a route of this machine, not a web service, already inspected, or engine errors.
pub async fn plan_on<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    inspector: &Inspector,
    hostname: &str,
    path: Option<&str>,
) -> Result<InspectPlan, InspectError> {
    let (route, url) = prepare_on(engine, api, connectors, ctx, inspector, hostname, path).await?;
    let ctx = Context {
        tunnel: route.tunnel_id.as_deref(),
        ..ctx
    };
    let change = update(&route, &url);
    let plan = preview(engine, api, ctx, &change).await?;
    Ok(InspectPlan {
        change,
        tunnel_id: route.tunnel_id,
        plan,
    })
}

/// Points the route at the inspector, as reviewed (`fingerprint` of [`plan_on`]'s
/// plan). The route is remembered first, so a crash can't leave it unnoticed.
///
/// # Errors
/// As [`plan_on`]; `Stale` if anything changed since the review.
#[allow(clippy::too_many_arguments)]
pub async fn apply_on<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    inspector: &Inspector,
    hostname: &str,
    path: Option<&str>,
    approval: Approval<'_>,
    progress: impl FnMut(Progress) + Send,
) -> Result<Outcome, InspectError> {
    let (route, url) = prepare_on(engine, api, connectors, ctx, inspector, hostname, path).await?;
    let tunnel = route.tunnel_id.clone();
    let ctx = Context {
        tunnel: tunnel.as_deref(),
        ..ctx
    };
    let change = update(&route, &url);
    let remembered = InspectedRoute {
        account_id: ctx.account.to_owned(),
        hostname: route.hostname.to_ascii_lowercase(),
        path: route.path.clone(),
        tunnel_id: route.tunnel_id.clone(),
        original_origin: route.origin.clone(),
        access: route.access.clone(),
        lens_url: url,
        owner: inspector.owner().to_owned(),
        created_at: now_ms(),
    };
    let store = engine.local().store().clone();
    record(&store, &remembered).await?;
    let intent = engine.intent_for(api, ctx, &change).await;
    let outcome = match intent {
        Ok(intent) => {
            engine
                .apply(api, connectors, ctx, &intent, approval, progress)
                .await
        }
        Err(err) => Err(err),
    };
    if !matches!(outcome, Ok(Outcome::Applied { .. })) {
        let _ = forget(&store, ctx.account, hostname, path).await;
        if let Some(tap) = inspector.tap_for(&scope(ctx.account, &route)) {
            inspector.stop(&tap).await;
        }
    }
    Ok(outcome?)
}

/// Plans pointing an inspected route back at its own service, for review. `None` when
/// there's nothing to do (the route is gone or was changed since; it's forgotten).
///
/// # Errors
/// [`InspectError::NotInspected`], or engine errors.
pub async fn plan_off<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    hostname: &str,
    path: Option<&str>,
) -> Result<Option<InspectPlan>, InspectError> {
    let store = engine.local().store().clone();
    let Some(row) = find(&store, ctx.account, hostname, path).await? else {
        return Err(InspectError::NotInspected(hostname.to_owned()));
    };
    let base = Context {
        tunnel: None,
        ..ctx
    };
    let route = route_view(engine, api, connectors, base, hostname, path).await?;
    let Some(route) = route.filter(|r| r.origin == row.lens_url) else {
        forget(&store, ctx.account, hostname, path).await?;
        return Ok(None);
    };
    let ctx = Context {
        tunnel: route.tunnel_id.as_deref(),
        ..ctx
    };
    let change = row.restore_change(route.access.clone());
    let plan = preview(engine, api, ctx, &change).await?;
    Ok(Some(InspectPlan {
        change,
        tunnel_id: route.tunnel_id,
        plan,
    }))
}

/// Points an inspected route back at its own service, as reviewed, and stops its tap.
/// `Ok(None)`: nothing to do (see [`plan_off`]).
///
/// # Errors
/// As [`plan_off`]; `Stale` if anything changed since the review.
#[allow(clippy::too_many_arguments)]
pub async fn apply_off<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    inspector: Option<&Inspector>,
    hostname: &str,
    path: Option<&str>,
    approval: Approval<'_>,
    progress: impl FnMut(Progress) + Send,
) -> Result<Option<Outcome>, InspectError> {
    let Some(plan) = plan_off(engine, api, connectors, ctx, hostname, path).await? else {
        stop_tap(inspector, ctx.account, hostname, path).await;
        return Ok(None);
    };
    let ctx = Context {
        tunnel: plan.tunnel_id.as_deref(),
        ..ctx
    };
    let intent = engine.intent_for(api, ctx, &plan.change).await?;
    let outcome = engine
        .apply(api, connectors, ctx, &intent, approval, progress)
        .await?;
    if matches!(outcome, Outcome::Applied { .. }) {
        forget(engine.local().store(), ctx.account, hostname, path).await?;
        stop_tap(inspector, ctx.account, hostname, path).await;
    }
    Ok(Some(outcome))
}

async fn stop_tap(
    inspector: Option<&Inspector>,
    account: &str,
    hostname: &str,
    path: Option<&str>,
) {
    if let Some(inspector) = inspector
        && let Some(tap) = inspector.tap_for(&TapScope::route(account, hostname, path))
    {
        inspector.stop(&tap).await;
    }
}

/// Reverts an inspected route without review (quitting, sweeping after a crash): the
/// same change the person approved when inspection started, undone.
///
/// # Errors
/// A message; the route stays remembered so a later sweep (or the Doctor) can try
/// again.
pub async fn revert<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    machine_name: &str,
    inspector: Option<&Inspector>,
    route: &InspectedRoute,
) -> Result<(), Text> {
    let ctx = Context {
        account: &route.account_id,
        machine_name,
        tunnel: None,
    };
    let path = route.path.as_deref();
    let reverted = async {
        let Some(plan) = plan_off(engine, api, connectors, ctx, &route.hostname, path).await?
        else {
            return Ok(());
        };
        let ctx = Context {
            tunnel: plan.tunnel_id.as_deref(),
            ..ctx
        };
        let intent = engine.intent_for(api, ctx, &plan.change).await?;
        let approval = Approval {
            fingerprint: &plan.plan.fingerprint,
            confirmed: false,
        };
        match engine
            .apply(api, connectors, ctx, &intent, approval, |_| {})
            .await?
        {
            Outcome::Applied { .. } => {
                forget(engine.local().store(), ctx.account, &route.hostname, path).await?;
                Ok(())
            }
            Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } => {
                Err(InspectError::Invalid(error.english()))
            }
        }
    }
    .await;
    stop_tap(inspector, &route.account_id, &route.hostname, path).await;
    match reverted {
        Ok(()) => Ok(()),
        // The route is gone: nothing to restore.
        Err(InspectError::Engine(EngineError::Plan(
            PlanError::NoSuchRoute(_) | PlanError::NoTunnel,
        ))) => {
            let _ = forget(
                engine.local().store(),
                &route.account_id,
                &route.hostname,
                path,
            )
            .await;
            Ok(())
        }
        Err(err) => Err(err.text()),
    }
}

/// The port of an inspector address (`http://127.0.0.1:PORT`).
fn lens_port(url: &str) -> Option<u16> {
    url.rsplit(':').next()?.trim_end_matches('/').parse().ok()
}

/// Inspected routes whose rule (`(hostname, path, service)`) still points at the
/// inspector's address while nothing listens there (`listening`: port → listening).
pub fn orphans(
    remembered: &[InspectedRoute],
    rules: &[(String, Option<String>, String)],
    listening: &HashMap<u16, bool>,
) -> Vec<InspectedRoute> {
    remembered
        .iter()
        .filter(|route| {
            rules.iter().any(|(hostname, path, service)| {
                hostname.eq_ignore_ascii_case(&route.hostname)
                    && *path == route.path
                    && *service == route.lens_url
            }) && lens_port(&route.lens_url)
                .is_some_and(|port| listening.get(&port) == Some(&false))
        })
        .cloned()
        .collect()
}

/// Reverts every inspected route `over` says should end, in every account. Returns what
/// couldn't be reverted (tried again next time).
pub async fn sweep<K: Connectors>(
    accounts: &Accounts,
    engine: &Engine,
    connectors: &K,
    machine_name: &str,
    inspector: Option<&Inspector>,
    over: impl Fn(&InspectedRoute) -> bool,
) -> Vec<Text> {
    let mut failures = Vec::new();
    let routes = list(engine.local().store(), None).await.unwrap_or_default();
    for route in routes.iter().filter(|r| over(r)) {
        let Ok(api) = accounts.client(&route.account_id).await else {
            continue;
        };
        if let Err(message) = revert(engine, &api, connectors, machine_name, inspector, route).await
        {
            tracing::warn!(
                hostname = %route.hostname,
                "couldn't end the inspection of a route: {}",
                message.english()
            );
            failures.push(message);
        }
    }
    failures
}
