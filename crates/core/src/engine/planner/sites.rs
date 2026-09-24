//! Planning Snapshot changes (the Worker, its address and its login).
//!
//! Order: files → Worker/version → login → address, so a Snapshot is never reachable
//! before its login is in place; deleting runs the other way and removes the Worker
//! last (it can't be undone).

use super::{Builder, PlanError};
use crate::engine::{
    ownership::{Marker, Ownership},
    sites::{SiteAddress, SiteContent, SiteFile, SiteSettings, SiteSpec, SiteState},
    types::{Step, Warning},
};

fn state<'a>(b: &Builder<'a>, site: &SiteSpec) -> Result<&'a SiteState, PlanError> {
    b.snapshot
        .site
        .as_ref()
        .filter(|s| s.script == site.script)
        .ok_or_else(|| PlanError::NoSuchSnapshot(site.name.clone()))
}

/// The address's `workers.dev` hostname.
pub(crate) fn workers_dev_address(script: &str, subdomain: &str) -> String {
    format!("{script}.{subdomain}.workers.dev")
}

/// Makes the Snapshot answer where `site.address` says, if it doesn't already. Foreign
/// DNS records on a custom hostname are deleted first, with confirmation (Cloudflare
/// won't attach a Custom Domain over them).
fn serve(b: &mut Builder<'_>, site: &SiteSpec, state: &SiteState) -> Result<(), PlanError> {
    match &site.address {
        SiteAddress::Domain { hostname } => {
            let zone_id = b.zone_id(hostname)?;
            if state
                .domains
                .iter()
                .any(|d| d.hostname.eq_ignore_ascii_case(hostname.as_str()))
            {
                return Ok(());
            }
            let routed = b
                .snapshot
                .routes()
                .iter()
                .any(|r| r.hostname.as_deref() == Some(hostname.as_str()))
                || b.snapshot
                    .elsewhere
                    .iter()
                    .any(|r| r.hostname.eq_ignore_ascii_case(hostname.as_str()));
            if routed {
                return Err(PlanError::HostnameRouted(hostname.to_string()));
            }
            if let Some(worker) = &state.hostname_taken_by {
                return Err(PlanError::HostnameServed {
                    hostname: hostname.to_string(),
                    worker: worker.clone(),
                });
            }
            let (placeholders, existing): (Vec<_>, Vec<_>) = b
                .snapshot
                .records_named(hostname.as_str())
                .filter(|r| matches!(r.record.kind.as_str(), "A" | "AAAA" | "CNAME"))
                .cloned()
                .partition(|r| {
                    r.record
                        .comment
                        .as_deref()
                        .and_then(Ownership::parse)
                        .is_some_and(|o| o.marker == Marker::Lease)
                });
            // A reservation's placeholder makes way for the Snapshot (someone else's only
            // with a confirmation).
            if !placeholders.is_empty() {
                b.take_over(hostname.as_str());
            }
            for record in placeholders {
                b.steps.push(Step::DeleteRecord {
                    zone_id: record.zone_id.clone(),
                    record: record.record,
                });
            }
            if existing.iter().any(|r| r.owned) {
                return Err(PlanError::HostnameRouted(hostname.to_string()));
            }
            for record in existing {
                b.requires_confirmation = true;
                b.warnings.push(Warning::DeletesForeignRecord {
                    hostname: record.record.name.clone(),
                    kind: record.record.kind.clone(),
                    content: record.record.content.clone(),
                });
                b.steps.push(Step::DeleteRecord {
                    zone_id: record.zone_id.clone(),
                    record: record.record,
                });
            }
            b.steps.push(Step::AttachSnapshotDomain {
                zone_id,
                hostname: hostname.to_string(),
                script: site.script.clone(),
            });
        }
        SiteAddress::WorkersDev => {
            let subdomain = state
                .subdomain
                .as_deref()
                .ok_or(PlanError::NoWorkersSubdomain)?;
            if !state.workers_dev {
                b.steps.push(Step::EnableWorkersDev {
                    script: site.script.clone(),
                    address: workers_dev_address(&site.script, subdomain),
                });
            }
        }
    }
    Ok(())
}

/// A login on a custom hostname: added before the Snapshot answers there.
fn protect(b: &mut Builder<'_>, site: &SiteSpec) -> Result<(), PlanError> {
    let Some(rule) = &site.access else {
        return Ok(());
    };
    match site.address.hostname() {
        Some(hostname) => b.protect(hostname.as_str(), rule),
        None => Err(PlanError::SnapshotLoginNeedsDomain),
    }
}

/// Publishes a new Snapshot.
pub(super) fn publish(
    b: &mut Builder<'_>,
    site: &SiteSpec,
    settings: &SiteSettings,
    content: &SiteContent,
) -> Result<(), PlanError> {
    let state = state(b, site)?;
    if state.exists {
        return Err(PlanError::SnapshotExists(site.name.clone()));
    }
    if let SiteAddress::Domain { hostname } = &site.address {
        b.zone_id(hostname)?;
    }
    if site.access.is_some() && site.address.hostname().is_none() {
        return Err(PlanError::SnapshotLoginNeedsDomain);
    }
    if matches!(site.address, SiteAddress::WorkersDev) && state.subdomain.is_none() {
        return Err(PlanError::NoWorkersSubdomain);
    }
    b.steps.push(Step::UploadSnapshotFiles {
        script: site.script.clone(),
        content: content.clone(),
        changed_files: content.files.len() as u64,
        changed_bytes: content.bytes(),
    });
    b.steps.push(Step::CreateSnapshotWorker {
        snapshot: site.id.clone(),
        script: site.script.clone(),
        settings: settings.clone(),
    });
    protect(b, site)?;
    serve(b, site, state)
}

/// Publishes a new version and brings the login and address in line.
pub(super) fn update(
    b: &mut Builder<'_>,
    site: &SiteSpec,
    settings: &SiteSettings,
    content: &SiteContent,
    previous: &[SiteFile],
) -> Result<(), PlanError> {
    let state = state(b, site)?;
    if !state.exists {
        return Err(PlanError::NoSuchSnapshot(site.name.clone()));
    }
    protect(b, site)?;
    let (changed_files, changed_bytes) = content.changes_from(previous);
    b.steps.push(Step::UploadSnapshotFiles {
        script: site.script.clone(),
        content: content.clone(),
        changed_files,
        changed_bytes,
    });
    b.steps.push(Step::PublishSnapshotVersion {
        snapshot: site.id.clone(),
        script: site.script.clone(),
        settings: settings.clone(),
        previous: state.active_version.clone(),
    });
    serve(b, site, state)?;
    if site.access.is_none()
        && let Some(hostname) = site.address.hostname()
    {
        b.unprotect(hostname.as_str());
    }
    Ok(())
}

/// Makes an earlier version live again (nothing to do if it already is).
pub(super) fn rollback(
    b: &mut Builder<'_>,
    site: &SiteSpec,
    version_id: &str,
    number: u32,
) -> Result<(), PlanError> {
    let state = state(b, site)?;
    if !state.exists {
        return Err(PlanError::NoSuchSnapshot(site.name.clone()));
    }
    if state.active_version.as_deref() != Some(version_id) {
        b.steps.push(Step::RollBackSnapshot {
            snapshot: site.id.clone(),
            script: site.script.clone(),
            version_id: version_id.to_owned(),
            number,
            previous: state.active_version.clone(),
        });
    }
    Ok(())
}

/// Removes the address, then the login, then the Worker. A Snapshot whose Worker is
/// already gone plans nothing (it's just forgotten).
pub(super) fn delete(b: &mut Builder<'_>, site: &SiteSpec) {
    let Some(state) = b.snapshot.site.as_ref().filter(|s| s.script == site.script) else {
        return;
    };
    if state.workers_dev {
        let subdomain = state.subdomain.clone().unwrap_or_default();
        b.steps.push(Step::DisableWorkersDev {
            script: site.script.clone(),
            address: workers_dev_address(&site.script, &subdomain),
        });
    }
    for domain in &state.domains {
        b.steps.push(Step::DetachSnapshotDomain {
            domain: domain.clone(),
        });
    }
    if let Some(hostname) = site.address.hostname() {
        b.unprotect(hostname.as_str());
    }
    if state.exists {
        b.steps.push(Step::DeleteSnapshotWorker {
            snapshot: site.id.clone(),
            script: site.script.clone(),
        });
    }
}
