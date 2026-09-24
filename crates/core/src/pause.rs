//! Pausing a route or a share on your domain (M12-06): the hostname stays (the route
//! and its DNS record are kept) and visitors get a friendly "paused" page from this
//! computer's inspector (Lens) instead of the service; resuming serves the service again
//! at the same address.
//!
//! The paused page needs a tap in front of the service, run by the process that serves
//! the route: the app, `teitunnel up`/`serve`, or the terminal running `share --on`. A
//! pause is a row in `paused_routes` naming that process ([`request`]); the process
//! applies its rows to its taps ([`Enforcer`]), so any process (the CLI, an agent, the
//! app) can ask. A route that isn't inspected is pointed at a tap first, through a
//! reviewed-style plan like "Inspect this route" (D-110), and pointed back on resume.
//! If the process serving it stops, the pause ends with it (a share on your domain ends
//! then anyway; an inspected route is pointed back by the existing sweeps).

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use rusqlite::{OptionalExtension as _, params};
use serde::Serialize;

use crate::{
    accounts::Accounts,
    domain_shares::{APP_OWNER, now_ms},
    engine::{Approval, CloudApi, Connectors, Context, Engine, EngineError, Outcome, PlanError},
    inspect::{InspectError, Inspector, TapPatch, TapScope, lens::PausedPage, routes},
    runtime,
    store::{Store, StoreError},
    text::{Text, UserText, english_display, msg},
};

/// A paused route, as remembered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct PausedRoute {
    /// Account id.
    pub account_id: String,
    /// The route's hostname.
    pub hostname: String,
    /// The process serving the paused page: [`APP_OWNER`], or a CLI process.
    pub owner: String,
    /// The route was pointed at the inspector only for the pause (pointed back on
    /// resume).
    pub via_inspect: bool,
    /// Paused by its schedule (resumed when the schedule's window begins).
    pub by_schedule: bool,
    /// When (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub paused_at: u64,
}

/// Why a route can't be paused or resumed.
#[derive(Debug, thiserror::Error)]
pub enum PauseError {
    /// The terminal that ran this share has stopped.
    OwnerGone(String),
    /// The share runs in a terminal without the inspector (`--no-inspect`), which is
    /// what serves the paused page.
    NotInspected(String),
    /// Nothing on this computer can serve the paused page for a route: the app,
    /// `teitunnel up` or `teitunnel serve` must be running.
    NoHost(String),
    /// The database failed.
    Store(#[from] StoreError),
    /// The inspector or the change to the route failed.
    Inspect(#[from] InspectError),
}

impl UserText for PauseError {
    fn text(&self) -> Text {
        use msg::error::pause as m;
        match self {
            Self::OwnerGone(hostname) => m::owner_gone(hostname),
            Self::NotInspected(hostname) => m::not_inspected(hostname),
            Self::NoHost(hostname) => m::no_host(hostname),
            Self::Store(err) => err.text(),
            Self::Inspect(err) => err.text(),
        }
    }
}

english_display!(PauseError);

impl From<EngineError> for PauseError {
    fn from(err: EngineError) -> Self {
        Self::Inspect(InspectError::Engine(err))
    }
}

/// The settings key of the lease naming the process that serves paused pages and
/// schedules for this machine's routes (the app, `up` or `serve`).
const HOST_LEASE: &str = "routeHost";
/// How long a claim lasts; holders renew it every 30 seconds.
pub const HOST_LEASE_MS: u64 = 90_000;

/// Takes (or renews) the right to serve paused pages and run schedules for this
/// machine's routes, unless another live process holds it. The app always takes it
/// (it's where people pause things); `up` and `serve` take it when it's free.
///
/// # Errors
/// Database errors.
pub async fn claim_host(store: &Store, owner: &str, force: bool) -> Result<bool, StoreError> {
    let owner = owner.to_owned();
    let now = i64::try_from(now_ms()).unwrap_or(i64::MAX);
    let ttl = i64::try_from(HOST_LEASE_MS).unwrap_or(i64::MAX);
    store
        .call(move |conn| {
            let tx = conn.transaction()?;
            let current: Option<String> = tx
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![HOST_LEASE],
                    |row| row.get(0),
                )
                .optional()?;
            let held_by_other = current
                .and_then(|raw| serde_json::from_str::<(String, i64)>(&raw).ok())
                .is_some_and(|(holder, until)| {
                    holder != owner
                        && until > now
                        && (holder == APP_OWNER || runtime::is_running(&holder))
                });
            if held_by_other && !force {
                return Ok(false);
            }
            tx.execute(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![HOST_LEASE, serde_json::to_string(&(owner, now + ttl))?],
            )?;
            tx.commit()?;
            Ok(true)
        })
        .await
}

/// Gives the lease up (the process is stopping).
///
/// # Errors
/// Database errors.
pub async fn release_host(store: &Store, owner: &str) -> Result<(), StoreError> {
    if host(store).await?.as_deref() == Some(owner) {
        let key = HOST_LEASE;
        store
            .call(move |conn| {
                conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
                Ok(())
            })
            .await?;
    }
    Ok(())
}

/// The process holding the lease, if it's alive.
///
/// # Errors
/// Database errors.
pub async fn host(store: &Store) -> Result<Option<String>, StoreError> {
    let now = i64::try_from(now_ms()).unwrap_or(i64::MAX);
    let raw: Option<String> = store
        .call(|conn| {
            Ok(conn
                .query_row(
                    "SELECT value FROM settings WHERE key = ?1",
                    params![HOST_LEASE],
                    |row| row.get(0),
                )
                .optional()?)
        })
        .await?;
    Ok(raw
        .and_then(|raw| serde_json::from_str::<(String, i64)>(&raw).ok())
        .filter(|(holder, until)| {
            *until > now && (holder == APP_OWNER || runtime::is_running(holder))
        })
        .map(|(holder, _)| holder))
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PausedRoute> {
    Ok(PausedRoute {
        account_id: row.get(0)?,
        hostname: row.get(1)?,
        owner: row.get(2)?,
        via_inspect: row.get::<_, i64>(3)? != 0,
        by_schedule: row.get::<_, i64>(4)? != 0,
        paused_at: u64::try_from(row.get::<_, i64>(5)?).unwrap_or_default(),
    })
}

const COLUMNS: &str = "account_id, hostname, owner, via_inspect, by_schedule, paused_at";

/// Paused routes, in `account` or everywhere, by hostname.
///
/// # Errors
/// The database can't be read.
pub async fn list(store: &Store, account: Option<&str>) -> Result<Vec<PausedRoute>, StoreError> {
    let account = account.map(str::to_owned);
    store
        .call(move |conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM paused_routes WHERE ?1 IS NULL OR account_id = ?1
                 ORDER BY hostname"
            ))?;
            let rows = stmt.query_map(params![account], from_row)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// One paused route.
///
/// # Errors
/// The database can't be read.
pub async fn find(
    store: &Store,
    account: &str,
    hostname: &str,
) -> Result<Option<PausedRoute>, StoreError> {
    let (account, hostname) = (account.to_owned(), normal(hostname));
    store
        .call(move |conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {COLUMNS} FROM paused_routes WHERE account_id = ?1 AND hostname = ?2"
                    ),
                    params![account, hostname],
                    from_row,
                )
                .optional()?)
        })
        .await
}

async fn save(store: &Store, route: &PausedRoute) -> Result<(), StoreError> {
    let route = route.clone();
    store
        .call(move |conn| {
            conn.execute(
                "INSERT INTO paused_routes (account_id, hostname, owner, via_inspect, by_schedule, paused_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT (account_id, hostname) DO UPDATE SET owner = ?3, via_inspect = ?4,
                    by_schedule = ?5, paused_at = ?6",
                params![
                    route.account_id,
                    route.hostname,
                    route.owner,
                    i64::from(route.via_inspect),
                    i64::from(route.by_schedule),
                    i64::try_from(route.paused_at).unwrap_or(i64::MAX),
                ],
            )?;
            Ok(())
        })
        .await
}

/// Forgets a pause without resuming anything (the route went away).
///
/// # Errors
/// The database can't be written.
pub async fn forget(store: &Store, account: &str, hostname: &str) -> Result<(), StoreError> {
    let (account, hostname) = (account.to_owned(), normal(hostname));
    store
        .call(move |conn| {
            conn.execute(
                "DELETE FROM paused_routes WHERE account_id = ?1 AND hostname = ?2",
                params![account, hostname],
            )?;
            Ok(())
        })
        .await
}

fn normal(hostname: &str) -> String {
    hostname.trim().trim_end_matches('.').to_ascii_lowercase()
}

/// Whether `owner` runs a tap for `scope` (from the taps table other processes write).
async fn runs_tap(store: &Store, owner: &str, scope: &TapScope) -> Result<bool, StoreError> {
    let (owner, scope) = (owner.to_owned(), serde_json::to_string(scope)?);
    store
        .call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT 1 FROM lens_taps WHERE owner = ?1 AND scope = ?2 AND stopped_at IS NULL",
                    params![owner, scope],
                    |_| Ok(()),
                )
                .optional()?
                .is_some())
        })
        .await
}

/// Asks for `hostname` to be paused. The process serving it applies it: the share's
/// owner for a share on your domain, else the process holding the host lease (the app,
/// `up` or `serve`). A pause by hand replaces one by the schedule.
///
/// # Errors
/// Nothing on this computer can serve the paused page ([`PauseError`]).
pub async fn request(
    store: &Store,
    account: &str,
    hostname: &str,
    by_schedule: bool,
) -> Result<PausedRoute, PauseError> {
    let hostname = normal(hostname);
    if let Some(mut existing) = find(store, account, &hostname).await? {
        if existing.by_schedule && !by_schedule {
            existing.by_schedule = false;
            save(store, &existing).await?;
        }
        return Ok(existing);
    }
    let share = crate::engine::Local::new(store.clone())
        .shares(Some(account))
        .await?
        .into_iter()
        .find(|s| s.hostname == hostname);
    let owner = match share {
        Some(share) if share.owner == APP_OWNER => share.owner,
        Some(share) => {
            if !runtime::is_running(&share.owner) {
                return Err(PauseError::OwnerGone(hostname));
            }
            if !runs_tap(
                store,
                &share.owner,
                &TapScope::route(account, &hostname, None),
            )
            .await?
            {
                return Err(PauseError::NotInspected(hostname));
            }
            share.owner
        }
        None => host(store)
            .await?
            .ok_or_else(|| PauseError::NoHost(hostname.clone()))?,
    };
    let route = PausedRoute {
        account_id: account.to_owned(),
        hostname,
        owner,
        via_inspect: false,
        by_schedule,
        paused_at: now_ms(),
    };
    save(store, &route).await?;
    Ok(route)
}

/// Asks for `hostname` to be served again (its owner applies it). `false`: it wasn't
/// paused.
///
/// # Errors
/// The database can't be written.
pub async fn request_resume(
    store: &Store,
    account: &str,
    hostname: &str,
) -> Result<bool, StoreError> {
    let paused = find(store, account, hostname).await?.is_some();
    forget(store, account, hostname).await?;
    Ok(paused)
}

/// The page visitors see while a route is paused.
pub fn page() -> PausedPage {
    PausedPage::default()
}

fn paused_patch(on: bool) -> TapPatch {
    TapPatch {
        paused: Some(on),
        paused_page: on.then(page),
        ..TapPatch::default()
    }
}

/// Points an uninspected route at a tap of `inspector` (a plan through the engine, like
/// "Inspect this route") and pauses the tap. Records `via_inspect` so resuming points it
/// back.
///
/// # Errors
/// Not a route of this machine, not a web service, or engine errors.
pub async fn pause_here<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: &C,
    connectors: &K,
    ctx: Context<'_>,
    inspector: &Inspector,
    route: &PausedRoute,
) -> Result<(), PauseError> {
    let scope = TapScope::route(&route.account_id, &route.hostname, None);
    if inspector.tap_for(&scope).is_none() {
        // Still pointing at an inspector from before a restart: wait until the sweep has
        // pointed it back, rather than put a tap in front of a dead one.
        let left = routes::list(engine.local().store(), Some(&route.account_id)).await?;
        if left
            .iter()
            .any(|r| r.hostname == route.hostname && r.path.is_none())
        {
            return Err(InspectError::TapGone.into());
        }
        let plan = routes::plan_on(
            engine,
            api,
            connectors,
            ctx,
            inspector,
            &route.hostname,
            None,
        )
        .await?;
        let approval = Approval {
            fingerprint: &plan.plan.fingerprint,
            confirmed: false,
        };
        let outcome = routes::apply_on(
            engine,
            api,
            connectors,
            ctx,
            inspector,
            &route.hostname,
            None,
            approval,
            |_| {},
        )
        .await?;
        if let Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. } = outcome
        {
            return Err(InspectError::Invalid(error.english()).into());
        }
        let store = engine.local().store();
        if let Some(mut row) = find(store, &route.account_id, &route.hostname).await? {
            row.via_inspect = true;
            save(store, &row).await?;
        }
    }
    let tap = inspector.tap_for(&scope).ok_or(InspectError::UnknownTap)?;
    inspector.configure(&tap, &paused_patch(true))?;
    Ok(())
}

/// Serves the route again: the tap stops showing the paused page and, when the route
/// was pointed at the inspector only for the pause, it's pointed back at its service.
///
/// # Errors
/// Engine errors pointing it back (the tap is resumed anyway).
pub async fn resume_here<C: CloudApi, K: Connectors>(
    engine: &Engine,
    api: Option<&C>,
    connectors: &K,
    ctx: Context<'_>,
    inspector: &Inspector,
    hostname: &str,
    via_inspect: bool,
) -> Result<(), PauseError> {
    let scope = TapScope::route(ctx.account, hostname, None);
    if let Some(tap) = inspector.tap_for(&scope) {
        let _ = inspector.configure(&tap, &paused_patch(false));
    }
    let (true, Some(api)) = (via_inspect, api) else {
        return Ok(());
    };
    let Some(plan) = routes::plan_off(engine, api, connectors, ctx, hostname, None).await? else {
        return Ok(());
    };
    let ctx = Context {
        tunnel: plan.tunnel_id.as_deref(),
        ..ctx
    };
    let outcome = routes::apply_off(
        engine,
        api,
        connectors,
        ctx,
        Some(inspector),
        hostname,
        None,
        Approval {
            fingerprint: &plan.plan.fingerprint,
            confirmed: false,
        },
        |_| {},
    )
    .await?;
    match outcome {
        Some(Outcome::RolledBack { error, .. } | Outcome::PartiallyApplied { error, .. }) => {
            Err(InspectError::Invalid(error.english()).into())
        }
        _ => Ok(()),
    }
}

/// Work for the Cloudflare side after [`Enforcer::sync_taps`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Needs {
    /// Point the route at a tap first ([`pause_here`]).
    Inspect(PausedRoute),
    /// The pause ended; point the route back at its service ([`resume_here`]).
    Revert {
        /// Account id.
        account_id: String,
        /// Hostname.
        hostname: String,
    },
}

/// Applies this process's pauses to its taps, and remembers what it applied so a pause
/// that ends (its row removed by any process) is undone.
#[derive(Debug, Default)]
pub struct Enforcer {
    /// Applied pauses → whether the route was pointed at the inspector for it.
    applied: Mutex<HashMap<(String, String), bool>>,
}

impl Enforcer {
    /// An enforcer that hasn't applied anything yet.
    pub fn new() -> Self {
        Self::default()
    }

    fn applied(&self) -> std::sync::MutexGuard<'_, HashMap<(String, String), bool>> {
        self.applied.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Pauses and resumes `inspector`'s taps to match the rows this process owns. What
    /// needs a change in Cloudflare is returned instead (for [`Self::sync`] or a test).
    ///
    /// # Errors
    /// The database can't be read.
    pub async fn sync_taps(
        &self,
        store: &Store,
        inspector: &Inspector,
    ) -> Result<Vec<Needs>, StoreError> {
        let rows: Vec<PausedRoute> = list(store, None)
            .await?
            .into_iter()
            .filter(|r| r.owner == inspector.owner())
            .collect();
        let mut needs = Vec::new();
        for row in &rows {
            let scope = TapScope::route(&row.account_id, &row.hostname, None);
            let key = (row.account_id.clone(), row.hostname.clone());
            match inspector.tap_for(&scope) {
                Some(tap) => {
                    let paused = inspector.view(&tap).is_ok_and(|v| v.paused.is_some());
                    if paused || inspector.configure(&tap, &paused_patch(true)).is_ok() {
                        self.applied().insert(key, row.via_inspect);
                    }
                }
                None => needs.push(Needs::Inspect(row.clone())),
            }
        }
        let ended: Vec<((String, String), bool)> = self
            .applied()
            .iter()
            .filter(|(key, _)| {
                !rows
                    .iter()
                    .any(|r| (&r.account_id, &r.hostname) == (&key.0, &key.1))
            })
            .map(|(key, via)| (key.clone(), *via))
            .collect();
        for ((account_id, hostname), via_inspect) in ended {
            self.applied()
                .remove(&(account_id.clone(), hostname.clone()));
            let scope = TapScope::route(&account_id, &hostname, None);
            if let Some(tap) = inspector.tap_for(&scope) {
                let _ = inspector.configure(&tap, &paused_patch(false));
            }
            if via_inspect {
                needs.push(Needs::Revert {
                    account_id,
                    hostname,
                });
            }
        }
        Ok(needs)
    }

    /// Notes that `route` was paused through a new inspection ([`pause_here`]).
    pub fn applied_via_inspect(&self, route: &PausedRoute) {
        self.applied()
            .insert((route.account_id.clone(), route.hostname.clone()), true);
    }

    /// Everything [`Self::sync_taps`] does, and the Cloudflare side with `accounts`'
    /// clients. A route that's gone forgets its pause. Returns what failed (tried again
    /// next time).
    pub async fn sync<K: Connectors>(
        &self,
        accounts: &Accounts,
        engine: &Engine,
        connectors: &K,
        machine_name: &str,
        inspector: &Inspector,
    ) -> Vec<Text> {
        let store = engine.local().store();
        let needs = match self.sync_taps(store, inspector).await {
            Ok(needs) => needs,
            Err(err) => return vec![err.text()],
        };
        let mut failures = Vec::new();
        for need in needs {
            let account = match &need {
                Needs::Inspect(route) => route.account_id.clone(),
                Needs::Revert { account_id, .. } => account_id.clone(),
            };
            let Ok(api) = accounts.client(&account).await else {
                continue;
            };
            let ctx = Context {
                account: &account,
                machine_name,
                tunnel: None,
            };
            let result = match &need {
                Needs::Inspect(route) => {
                    let done = pause_here(engine, &api, connectors, ctx, inspector, route).await;
                    if done.is_ok() {
                        self.applied_via_inspect(route);
                    }
                    done
                }
                Needs::Revert { hostname, .. } => {
                    resume_here(
                        engine,
                        Some(&api),
                        connectors,
                        ctx,
                        inspector,
                        hostname,
                        true,
                    )
                    .await
                }
            };
            match result {
                Ok(()) => {}
                // The route is gone, or can't have a paused page (not a web service):
                // the pause is dropped rather than retried forever.
                Err(
                    err @ PauseError::Inspect(
                        InspectError::NotRoute(_)
                        | InspectError::NotWeb
                        | InspectError::Engine(EngineError::Plan(
                            PlanError::NoSuchRoute(_) | PlanError::NoTunnel,
                        )),
                    ),
                ) => {
                    if let Needs::Inspect(route) = &need {
                        let _ = forget(store, &route.account_id, &route.hostname).await;
                        failures.push(err.text());
                    }
                }
                Err(err) => {
                    tracing::warn!("couldn't apply a pause: {}", err.text().english());
                    failures.push(err.text());
                }
            }
        }
        failures
    }
}

/// What's needed to pause from a process that serves routes (the app, `up`, `serve`, an
/// MCP server): its services and its inspector.
#[derive(Debug, Clone, Copy)]
pub struct Here<'a, K> {
    /// Connected accounts.
    pub accounts: &'a Accounts,
    /// The engine.
    pub engine: &'a Engine,
    /// This machine's connectors.
    pub connectors: &'a K,
    /// This machine's name.
    pub machine_name: &'a str,
    /// This process's inspector.
    pub inspector: &'a Inspector,
    /// This process's enforcer.
    pub enforcer: &'a Enforcer,
}

/// Pauses (`paused`) or resumes `hostname` in `account` and, when this process serves
/// it, applies that at once (another process applies it within seconds).
///
/// # Errors
/// Why it can't be paused ([`PauseError`]), or why applying it failed.
pub async fn set_paused<K: Connectors>(
    here: Here<'_, K>,
    account: &str,
    hostname: &str,
    paused: bool,
) -> Result<(), Text> {
    let store = here.engine.local().store();
    if paused {
        request(store, account, hostname, false)
            .await
            .map_err(|e| e.text())?;
    } else {
        request_resume(store, account, hostname)
            .await
            .map_err(|e| e.text())?;
    }
    let failures = here
        .enforcer
        .sync(
            here.accounts,
            here.engine,
            here.connectors,
            here.machine_name,
            here.inspector,
        )
        .await;
    if !paused {
        // A route that couldn't be pointed back still works: its tap forwards again.
        return Ok(());
    }
    let row = find(store, account, hostname).await.map_err(|e| e.text())?;
    if let Some(row) = row
        && row.owner == here.inspector.owner()
    {
        let scope = TapScope::route(account, &row.hostname, None);
        let applied = here
            .inspector
            .tap_for(&scope)
            .and_then(|tap| here.inspector.view(&tap).ok())
            .is_some_and(|view| view.paused.is_some());
        if !applied {
            // Nothing is left half-done: a pause that couldn't be applied is dropped.
            let _ = forget(store, account, hostname).await;
            return Err(failures
                .into_iter()
                .next()
                .unwrap_or_else(|| InspectError::UnknownTap.text()));
        }
    }
    Ok(())
}

/// The account of a share on your domain at `hostname`, if there is one.
///
/// # Errors
/// The database can't be read.
pub async fn share_account(store: &Store, hostname: &str) -> Result<Option<String>, StoreError> {
    let hostname = normal(hostname);
    Ok(crate::engine::Local::new(store.clone())
        .shares(None)
        .await?
        .into_iter()
        .find(|s| s.hostname == hostname)
        .map(|s| s.account_id))
}

/// A hostname from what a person typed: a hostname or a URL.
pub fn hostname_of(id: &str) -> String {
    normal(
        id.trim()
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests;
