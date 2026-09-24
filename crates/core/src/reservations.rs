//! Reservations for teams sharing one Cloudflare account (M12-11): which hostnames are
//! held and by whom, read from the DNS comments Teitunnel writes (see
//! [`crate::engine::ownership`]), with a local cache for when Cloudflare can't be
//! reached. Reserving and releasing are engine changes (`Change::ReserveHostname`,
//! `Change::ReleaseHostname`), planned and applied like routes.

use rusqlite::params;
use serde::Serialize;

use crate::{
    domain::Hostname,
    engine::{
        CloudApi, Engine, EngineError, ObserveError,
        ownership::{
            Hold, HoldKind, LEASE_ADDRESS, Marker, Me, Ownership, held_by_other, same_owner,
        },
    },
    store::StoreError,
};

/// The marker every Teitunnel DNS comment starts with.
const MARKER: &str = "teitunnel:";

/// One reserved hostname.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Reservation {
    /// The hostname.
    pub hostname: String,
    /// Who holds it (`person@machine`); `None` when the writer didn't say.
    pub owner: Option<String>,
    /// When it ends (milliseconds since the epoch); `None`: no end.
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub until: Option<u64>,
    /// The holder routes it too (the lease is on the route's record).
    pub routed: bool,
    /// Held by this owner.
    pub mine: bool,
    /// It has ended (the name is free; the record is cleaned up on the next change).
    pub ended: bool,
}

/// The account's reservations, and whether they came from the cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Reservations {
    /// By hostname.
    pub items: Vec<Reservation>,
    /// Read from the local cache because Cloudflare couldn't be reached.
    pub cached: bool,
}

/// Whether a hostname can be used, as the hostname field shows it while typing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Availability {
    /// Nothing is there.
    Free,
    /// This owner holds it (a reservation, or a route of this machine's).
    Yours,
    /// Someone else holds it.
    Held {
        /// Who, until when, and how.
        hold: Hold,
    },
    /// A DNS record Teitunnel didn't create is there.
    Foreign,
    /// Not in any of the account's domains.
    NoZone,
}

/// Reads the account's reservations from DNS (every zone) and caches them. When
/// Cloudflare can't be reached, the cache answers (`cached: true`).
///
/// # Errors
/// Authentication and API errors, or database errors with nothing cached.
pub async fn list<C: CloudApi>(
    engine: &Engine,
    api: &C,
    account: &str,
) -> Result<Reservations, EngineError> {
    let now = crate::domain_shares::now_ms();
    match read(api, account, engine.owner(), now).await {
        Ok(items) => {
            if let Err(err) = save(engine, account, &items, now).await {
                tracing::warn!(%err, "couldn't cache the reservations");
            }
            Ok(Reservations {
                items,
                cached: false,
            })
        }
        Err(err) if err.status().is_none() => Ok(Reservations {
            items: cached(engine, account)
                .await
                .map_err(ObserveError::from)?
                .into_iter()
                .map(|mut r| {
                    r.ended = r.until.is_some_and(|u| u <= now);
                    r
                })
                .collect(),
            cached: true,
        }),
        Err(err) => Err(ObserveError::from(err).into()),
    }
}

async fn read<C: CloudApi>(
    api: &C,
    account: &str,
    owner: &str,
    now: u64,
) -> cf_api::Result<Vec<Reservation>> {
    let mut items = Vec::new();
    for zone in api.zones(account).await? {
        for record in api.records_with_comment(&zone.id, MARKER).await? {
            if let Some(reservation) = reservation_of(&record, owner, now) {
                items.push(reservation);
            }
        }
    }
    items.sort_by(|a, b| a.hostname.cmp(&b.hostname));
    items.dedup_by(|a, b| a.hostname == b.hostname);
    Ok(items)
}

/// The reservation a record holds, if it holds one.
fn reservation_of(record: &cf_api::DnsRecord, owner: &str, now: u64) -> Option<Reservation> {
    let ownership = Ownership::parse(record.comment.as_deref()?)?;
    if !ownership.lease {
        return None;
    }
    Some(Reservation {
        hostname: record.name.to_ascii_lowercase(),
        mine: ownership
            .owner
            .as_deref()
            .is_some_and(|o| same_owner(o, owner)),
        routed: matches!(ownership.marker, Marker::Route(_)),
        ended: !ownership.leased_at(now),
        owner: ownership.owner,
        until: ownership.until,
    })
}

async fn save(
    engine: &Engine,
    account: &str,
    items: &[Reservation],
    now: u64,
) -> Result<(), StoreError> {
    let (account, items) = (account.to_owned(), items.to_vec());
    engine
        .local()
        .store()
        .call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "DELETE FROM reservations_cache WHERE account_id = ?1",
                params![account],
            )?;
            for r in &items {
                tx.execute(
                    "INSERT OR REPLACE INTO reservations_cache
                       (account_id, hostname, owner, until, routed, mine, seen_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        account,
                        r.hostname,
                        r.owner,
                        r.until.and_then(|u| i64::try_from(u).ok()),
                        i64::from(r.routed),
                        i64::from(r.mine),
                        i64::try_from(now).unwrap_or(i64::MAX),
                    ],
                )?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
}

/// The reservations last read for `account`.
///
/// # Errors
/// Database errors.
pub async fn cached(engine: &Engine, account: &str) -> Result<Vec<Reservation>, StoreError> {
    let account = account.to_owned();
    engine
        .local()
        .store()
        .call(move |conn| {
            let mut statement = conn.prepare(
                "SELECT hostname, owner, until, routed, mine FROM reservations_cache
                 WHERE account_id = ?1 ORDER BY hostname",
            )?;
            let rows = statement.query_map(params![account], |row| {
                Ok(Reservation {
                    hostname: row.get(0)?,
                    owner: row.get(1)?,
                    until: row
                        .get::<_, Option<i64>>(2)?
                        .and_then(|u| u64::try_from(u).ok()),
                    routed: row.get::<_, i64>(3)? != 0,
                    mine: row.get::<_, i64>(4)? != 0,
                    ended: false,
                })
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await
}

/// Whether `hostname` is free, held by this owner, or someone else's: the records at the
/// name, read now (one request).
///
/// # Errors
/// API and database errors.
pub async fn availability<C: CloudApi>(
    engine: &Engine,
    api: &C,
    account: &str,
    hostname: &str,
) -> Result<Availability, EngineError> {
    let Ok(hostname) = Hostname::parse(hostname) else {
        return Ok(Availability::NoZone);
    };
    let zones = api.zones(account).await.map_err(ObserveError::from)?;
    let Some(zone) = hostname.zone_in(&zones) else {
        return Ok(Availability::NoZone);
    };
    let records: Vec<cf_api::DnsRecord> = api
        .records_named(&zone.id, hostname.as_str())
        .await
        .map_err(ObserveError::from)?
        .into_iter()
        .filter(|r| matches!(r.kind.as_str(), "A" | "AAAA" | "CNAME"))
        .collect();
    let tunnels: Vec<String> = engine
        .local()
        .tunnels(account)
        .await
        .map_err(ObserveError::from)?
        .into_iter()
        .map(|t| t.tunnel_id)
        .collect();
    let me = Me {
        owner: engine.owner(),
        tunnels: &tunnels,
        now: crate::domain_shares::now_ms(),
    };
    Ok(availability_of(&records, me))
}

/// [`availability`] for the records at a name.
pub fn availability_of(records: &[cf_api::DnsRecord], me: Me<'_>) -> Availability {
    if records.is_empty() {
        return Availability::Free;
    }
    if let Some(hold) = records.iter().find_map(|r| held_by_other(r, me)) {
        return Availability::Held { hold };
    }
    let mut yours = false;
    for record in records {
        let Some(ownership) = record.comment.as_deref().and_then(Ownership::parse) else {
            return Availability::Foreign;
        };
        let placeholder = ownership.marker == Marker::Lease && record.content == LEASE_ADDRESS;
        // An ended lease is free; anything else of Teitunnel's that nobody else holds is
        // this owner's (their lease, or a route through one of this machine's tunnels).
        if !(placeholder && !ownership.leased_at(me.now)) {
            yours = true;
        }
    }
    if yours {
        Availability::Yours
    } else {
        Availability::Free
    }
}

/// A hold, as one English-free sentence for the CLI and agents.
pub fn describe(hold: &Hold) -> crate::text::Text {
    use crate::text::msg::reservations::held as m;
    let owner = hold.owner.clone().unwrap_or_else(|| m::someone().english());
    match (hold.kind, hold.until) {
        (HoldKind::Route, _) => m::routed(&hold.hostname, owner),
        (HoldKind::Reservation, Some(until)) => m::reserved(
            &hold.hostname,
            owner,
            crate::engine::ownership::format_until(until),
        ),
        (HoldKind::Reservation, None) => m::reserved_forever(&hold.hostname, owner),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        engine::{
            Local,
            fake::{CloudState, FakeCloud},
        },
        store::Store,
    };

    const DAY: u64 = 86_400_000;

    fn record(name: &str, content: &str, comment: Option<String>) -> cf_api::DnsRecord {
        cf_api::DnsRecord {
            id: format!("id-{name}"),
            name: name.into(),
            kind: if content == LEASE_ADDRESS {
                "AAAA"
            } else {
                "CNAME"
            }
            .into(),
            content: content.into(),
            proxied: true,
            comment,
            ttl: 1,
        }
    }

    #[test]
    fn availability_while_typing() {
        let tunnels = ["mine".to_owned()];
        let me = Me {
            owner: "alice@mac",
            tunnels: &tunnels,
            now: DAY,
        };
        assert_eq!(availability_of(&[], me), Availability::Free);
        let theirs = record(
            "a.xyz.com",
            LEASE_ADDRESS,
            Some(Ownership::lease("bob@pc", Some(2 * DAY)).render()),
        );
        assert!(matches!(
            availability_of(std::slice::from_ref(&theirs), me),
            Availability::Held { hold } if hold.owner.as_deref() == Some("bob@pc")
        ));
        let later = Me { now: 3 * DAY, ..me };
        assert_eq!(availability_of(&[theirs], later), Availability::Free);
        let ours = record(
            "a.xyz.com",
            "mine.cfargotunnel.com",
            Some(Ownership::route("r", "alice@mac").render()),
        );
        assert_eq!(availability_of(&[ours], me), Availability::Yours);
        let foreign = record("a.xyz.com", "elsewhere.example.net", None);
        assert_eq!(availability_of(&[foreign], me), Availability::Foreign);
    }

    #[tokio::test]
    async fn lists_every_zones_reservations_and_caches_them() {
        let engine =
            Engine::new(Local::new(Store::open_in_memory().unwrap())).with_owner("alice@mac");
        let cloud = FakeCloud::new(CloudState {
            zones: vec![
                crate::engine::ZoneRef {
                    id: "z1".into(),
                    name: "xyz.com".into(),
                },
                crate::engine::ZoneRef {
                    id: "z2".into(),
                    name: "yx.com".into(),
                },
            ],
            ..CloudState::default()
        });
        {
            let mut state = cloud.state.lock().unwrap();
            state.records.insert(
                "z1".into(),
                vec![
                    record(
                        "b.xyz.com",
                        LEASE_ADDRESS,
                        Some(Ownership::lease("bob@pc", Some(DAY)).render()),
                    ),
                    record(
                        "route.xyz.com",
                        "t.cfargotunnel.com",
                        Some(Ownership::route("r", "bob@pc").render()),
                    ),
                ],
            );
            state.records.insert(
                "z2".into(),
                vec![record(
                    "a.yx.com",
                    "t.cfargotunnel.com",
                    Some(
                        Ownership {
                            lease: true,
                            ..Ownership::route("r", "alice@mac")
                        }
                        .render(),
                    ),
                )],
            );
        }
        let listed = list(&engine, &cloud, "acc").await.unwrap();
        assert!(!listed.cached);
        let names: Vec<(&str, bool, bool, bool)> = listed
            .items
            .iter()
            .map(|r| (r.hostname.as_str(), r.mine, r.routed, r.ended))
            .collect();
        assert_eq!(
            names,
            [
                ("a.yx.com", true, true, false),
                ("b.xyz.com", false, false, true)
            ],
            "a route without a lease isn't a reservation; an ended lease is listed as ended"
        );
        let cache = cached(&engine, "acc").await.unwrap();
        assert_eq!(cache.len(), 2);
        assert_eq!(cache[1].owner.as_deref(), Some("bob@pc"));
    }

    #[test]
    fn describes_holds() {
        let hold = Hold {
            hostname: "a.xyz.com".into(),
            owner: Some("bob@pc".into()),
            until: Some(crate::engine::ownership::parse_until("2026-12-31T00:00Z").unwrap()),
            kind: HoldKind::Reservation,
        };
        assert_eq!(
            describe(&hold).english(),
            "a.xyz.com is reserved by bob@pc until 2026-12-31T00:00Z."
        );
        let routed = Hold {
            owner: None,
            kind: HoldKind::Route,
            ..hold
        };
        assert_eq!(
            describe(&routed).english(),
            "a.xyz.com is routed by another Teitunnel."
        );
    }
}
