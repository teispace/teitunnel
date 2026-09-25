//! Planning Workers in front of a route (the offline page, webhook inboxes) and the
//! account's D1 database.
//!
//! Order: database → Worker → route, so a route never points at a Worker that isn't
//! there; removing runs the other way (route first, so requests go straight to the
//! tunnel again before the Worker goes). A route pattern someone else's Worker already
//! has is never taken over.

use super::{Builder, PlanError};
use crate::{
    Secret,
    domain::Hostname,
    engine::{
        front::{
            DatabaseRef, FrontConfig, FrontKind, InboxSettings, OfflinePage, clean_inbox_path,
            pattern_for, script_for,
        },
        types::{Step, Warning},
    },
};

/// The account's database, creating it first when there's none.
pub(super) fn database(b: &mut Builder<'_>) -> DatabaseRef {
    match b.snapshot.database.as_ref().and_then(|d| d.id.clone()) {
        Some(id) => DatabaseRef::Existing(id),
        None => {
            if !b
                .steps
                .iter()
                .any(|s| matches!(s, Step::CreateDatabase { .. }))
            {
                b.steps.push(Step::CreateDatabase {
                    name: crate::comments::remote::DATABASE_NAME.to_owned(),
                });
            }
            DatabaseRef::Created
        }
    }
}

/// The hostname must be proxied through Cloudflare (a route's DNS record), or a Worker
/// route would never run.
fn routed(b: &Builder<'_>, hostname: &Hostname) -> Result<String, PlanError> {
    let zone_id = b.zone_id(hostname)?;
    let proxied = b
        .snapshot
        .records_named(hostname.as_str())
        .any(|r| r.record.proxied && matches!(r.record.kind.as_str(), "A" | "AAAA" | "CNAME"));
    if proxied {
        Ok(zone_id)
    } else {
        Err(PlanError::FrontNeedsRoute(hostname.to_string()))
    }
}

/// Makes Teitunnel's Worker of `config`'s kind serve `hostname` with `config`.
fn put(
    b: &mut Builder<'_>,
    hostname: &Hostname,
    config: FrontConfig,
    database: Option<DatabaseRef>,
    secret: Option<Secret<String>>,
) -> Result<(), PlanError> {
    let zone_id = routed(b, hostname)?;
    let state = b
        .snapshot
        .front_of(hostname.as_str())
        .ok_or_else(|| PlanError::NoZone(hostname.to_string()))?;
    let pattern = pattern_for(hostname.as_str(), &config);
    let kind = config.kind();
    let path = config.path().to_owned();
    let existing = state.find(kind, &path).cloned();
    let script = existing.as_ref().map_or_else(
        || script_for(kind, hostname.as_str(), &path),
        |f| f.script.clone(),
    );
    let has_route = existing.as_ref().is_some_and(|f| f.route.is_some());
    if !has_route && let Some(foreign) = state.foreign_on(&pattern) {
        return Err(PlanError::WorkerRouteTaken {
            pattern,
            worker: foreign.script.clone().unwrap_or_default(),
        });
    }
    let unchanged = existing
        .as_ref()
        .is_some_and(|f| f.exists && f.config == config)
        && secret.is_none();
    if !unchanged {
        b.steps.push(Step::PutFrontWorker {
            hostname: hostname.to_string(),
            zone_id: zone_id.clone(),
            script: script.clone(),
            previous: existing.filter(|f| f.exists).map(|f| f.config),
            config,
            database,
            secret,
        });
    }
    if !has_route {
        b.warnings.push(Warning::WorkerRequests {
            pattern: pattern.clone(),
        });
        b.steps.push(Step::CreateWorkerRoute {
            hostname: hostname.to_string(),
            zone_id,
            pattern,
            script,
            kind,
            path,
        });
    }
    Ok(())
}

/// Removes Teitunnel's Worker of `kind` at `path` (route first).
fn remove(b: &mut Builder<'_>, hostname: &str, kind: FrontKind, path: &str) -> bool {
    let Some(state) = b.snapshot.front_of(hostname) else {
        return false;
    };
    let Some(front) = state.find(kind, path).cloned() else {
        return false;
    };
    let zone_id = state.zone_id.clone();
    let database = b.snapshot.database.as_ref().and_then(|d| d.id.clone());
    if let Some(route) = front.route {
        b.steps.push(Step::DeleteWorkerRoute {
            hostname: hostname.to_owned(),
            zone_id: zone_id.clone(),
            route,
            kind,
            path: path.to_owned(),
        });
    }
    // A Worker already gone is only forgotten (the step tolerates a missing one).
    b.steps.push(Step::DeleteFrontWorker {
        hostname: hostname.to_owned(),
        zone_id,
        script: front.script,
        previous: front.config,
        database,
    });
    true
}

/// Turns the offline page on (or changes it), or off.
pub(super) fn offline(
    b: &mut Builder<'_>,
    hostname: &Hostname,
    page: Option<&OfflinePage>,
) -> Result<(), PlanError> {
    match page {
        Some(page) => {
            let page = page.normalized().map_err(PlanError::Front)?;
            put(b, hostname, FrontConfig::Offline { page }, None, None)
        }
        None => {
            if remove(b, hostname.as_str(), FrontKind::Offline, "") {
                Ok(())
            } else {
                Err(PlanError::NoFront(hostname.to_string()))
            }
        }
    }
}

/// Turns a webhook inbox on (or changes it), or off.
pub(super) fn inbox(
    b: &mut Builder<'_>,
    hostname: &Hostname,
    path: &str,
    settings: Option<&InboxSettings>,
    secret: Option<&Secret<String>>,
) -> Result<(), PlanError> {
    let path = clean_inbox_path(path).map_err(PlanError::Front)?;
    match settings {
        Some(settings) => {
            let settings = settings.normalized().map_err(PlanError::Front)?;
            if settings.verify.is_some()
                && secret.is_none()
                && !b
                    .snapshot
                    .front_of(hostname.as_str())
                    .and_then(|s| s.find(FrontKind::Inbox, &path))
                    .is_some_and(|f| {
                        f.exists
                            && matches!(&f.config, FrontConfig::Inbox { settings, .. } if settings.verify.is_some())
                    })
            {
                return Err(PlanError::InboxNeedsSecret);
            }
            // Resolve the zone before the database, so an unknown hostname fails first.
            routed(b, hostname)?;
            let database = database(b);
            put(
                b,
                hostname,
                FrontConfig::Inbox { path, settings },
                Some(database),
                secret.cloned(),
            )
        }
        None => {
            if remove(b, hostname.as_str(), FrontKind::Inbox, &path) {
                Ok(())
            } else {
                Err(PlanError::NoFront(format!("{hostname}{path}")))
            }
        }
    }
}

/// Removes every front Worker of a hostname whose last route goes (the offline page
/// and inboxes would otherwise sit in front of nothing).
pub(super) fn remove_all(b: &mut Builder<'_>, hostname: &str) {
    let Some(fronts) = b.snapshot.front_of(hostname).map(|s| s.fronts.clone()) else {
        return;
    };
    for front in &fronts {
        remove(b, hostname, front.config.kind(), front.config.path());
    }
}
