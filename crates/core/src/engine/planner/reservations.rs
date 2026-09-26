//! Planning reservations: leases on hostnames, kept in DNS so every Teitunnel
//! sharing the account sees who holds a name.
//!
//! - reserve a free name: a placeholder record (proxied `AAAA 100::`) with the lease;
//! - reserve a name this machine routes: the route's record carries the lease;
//! - renew: only the comment changes;
//! - someone else's name: taken over only with a confirmation (their record goes);
//! - a record Teitunnel didn't write is never touched for a reservation;
//! - removing a route that carried a lease puts the placeholder back.

use super::{Builder, PlanError};
use crate::{
    domain::Hostname,
    engine::{
        ownership::{Marker, Ownership},
        types::{ObservedRecord, Step, Warning},
    },
};

impl Builder<'_> {
    /// Someone else's hold on `hostname`, if any.
    pub(super) fn holder(&self, hostname: &str) -> Option<&crate::engine::ownership::Hold> {
        self.snapshot
            .held
            .iter()
            .find(|h| h.hostname.eq_ignore_ascii_case(hostname))
    }

    /// Flags a change that takes `hostname` from whoever holds it: the plan says who and
    /// needs a confirmation (`--take-over`).
    pub(super) fn take_over(&mut self, hostname: &str) {
        if let Some(hold) = self.holder(hostname).cloned() {
            self.requires_confirmation = true;
            let warning = Warning::HeldBy {
                hostname: hold.hostname,
                owner: hold.owner,
                until: hold.until,
                kind: hold.kind,
            };
            if !self.warnings.contains(&warning) {
                self.warnings.push(warning);
            }
        }
    }
}

/// The A/AAAA/CNAME records at `hostname`, with what Teitunnel wrote on them (`None`:
/// not Teitunnel's).
fn records<'a>(b: &Builder<'a>, hostname: &'a str) -> Vec<(&'a ObservedRecord, Option<Ownership>)> {
    b.snapshot
        .records_named(hostname)
        .filter(|r| matches!(r.record.kind.as_str(), "A" | "AAAA" | "CNAME"))
        .map(|r| (r, r.record.comment.as_deref().and_then(Ownership::parse)))
        .collect()
}

/// Reserves `hostname` until `until` (or with no end).
pub(super) fn reserve(
    b: &mut Builder<'_>,
    hostname: &Hostname,
    until: Option<u64>,
) -> Result<(), PlanError> {
    let zone_id = b.zone_id(hostname)?;
    let name = hostname.as_str();
    let found = records(b, name);
    if found.iter().any(|(_, ownership)| ownership.is_none()) {
        return Err(PlanError::HostnameInUse(name.to_owned()));
    }
    let create = Step::CreateReservation {
        zone_id,
        hostname: name.to_owned(),
        until,
    };
    if b.holder(name).is_some() {
        b.take_over(name);
        for (record, _) in found {
            b.steps.push(Step::DeleteRecord {
                zone_id: record.zone_id.clone(),
                record: record.record.clone(),
            });
        }
        b.steps.push(create);
        return Ok(());
    }
    let now = b.snapshot.now;
    match found.as_slice() {
        [] => b.steps.push(create),
        // This machine's route, or a lease of its own: only the comment changes.
        [(record, Some(ownership))]
            if matches!(ownership.marker, Marker::Route(_))
                || (ownership.marker == Marker::Lease && ownership.leased_at(now)) =>
        {
            if !(ownership.leased_at(now) && ownership.until == until) {
                b.steps.push(Step::SetLease {
                    zone_id: record.zone_id.clone(),
                    record: record.record.clone(),
                    lease: true,
                    until,
                });
            }
        }
        // Ended leases and leftovers: free, replaced without asking.
        _ => {
            for (record, _) in found {
                b.steps.push(Step::DeleteRecord {
                    zone_id: record.zone_id.clone(),
                    record: record.record.clone(),
                });
            }
            b.steps.push(create);
        }
    }
    Ok(())
}

/// Gives up the reservation of `hostname`. A route on it stays; someone else's lease
/// is released only with a confirmation.
pub(super) fn release(b: &mut Builder<'_>, hostname: &Hostname) -> Result<(), PlanError> {
    b.zone_id(hostname)?;
    let name = hostname.as_str();
    let leased: Vec<(ObservedRecord, Ownership)> = records(b, name)
        .into_iter()
        .filter_map(|(record, ownership)| Some((record.clone(), ownership?)))
        .filter(|(_, ownership)| ownership.lease)
        .collect();
    if leased.is_empty() {
        return Err(PlanError::NotReserved(name.to_owned()));
    }
    b.take_over(name);
    for (record, ownership) in leased {
        match ownership.marker {
            Marker::Route(_) => b.steps.push(Step::SetLease {
                zone_id: record.zone_id,
                record: record.record,
                lease: false,
                until: None,
            }),
            _ => b.steps.push(Step::DeleteRecord {
                zone_id: record.zone_id,
                record: record.record,
            }),
        }
    }
    Ok(())
}

/// After a route's record is deleted: if it carried a lease that hasn't ended, the
/// placeholder takes its place so the name stays reserved.
pub(super) fn restore(b: &mut Builder<'_>, record: &ObservedRecord) {
    let Some(ownership) = record.record.comment.as_deref().and_then(Ownership::parse) else {
        return;
    };
    if matches!(ownership.marker, Marker::Route(_)) && ownership.leased_at(b.snapshot.now) {
        b.steps.push(Step::CreateReservation {
            zone_id: record.zone_id.clone(),
            hostname: record.record.name.clone(),
            until: ownership.until,
        });
    }
}
