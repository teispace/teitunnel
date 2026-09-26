//! Who holds a hostname, for teams sharing one Cloudflare account.
//!
//! Teitunnel marks every DNS record it writes with a comment. Since 0.3 the comment also
//! names the owner (person and machine, e.g. `alice@Alice-MacBook`), and a hostname can
//! be *reserved*: a lease, optionally until a date, kept in DNS itself so every
//! Teitunnel on the account sees it.
//!
//! Comment grammar (at most [`MAX_COMMENT`] characters, Cloudflare's limit on the Free
//! plan):
//!
//! ```text
//! teitunnel:route=<route id>[;by=<owner>][;lease][;until=<YYYY-MM-DDTHH:MMZ>]
//! teitunnel:lease[;by=<owner>][;until=<YYYY-MM-DDTHH:MMZ>]
//! ```
//!
//! Older versions wrote `teitunnel:route=<id>` alone; it still parses (no owner). A
//! reservation on a name nobody routes is a placeholder record: a proxied `AAAA 100::`
//! (the IPv6 discard prefix, RFC 6666, the address Cloudflare documents for
//! "originless" hostnames), so the name is visibly taken in the dashboard, nothing can
//! reach an origin through it, and Cloudflare refuses a conflicting CNAME. A route on a
//! reserved name keeps the lease in its own comment (`;lease`), and removing the route
//! puts the placeholder back.

use serde::Serialize;

/// Cloudflare's longest DNS record comment on the Free, Pro and Business plans.
pub const MAX_COMMENT: usize = 100;
/// The placeholder's address: the IPv6 discard prefix.
pub const LEASE_ADDRESS: &str = "100::";
/// The placeholder's record type.
pub const LEASE_KIND: &str = "AAAA";

const PREFIX: &str = "teitunnel:";
const MAX_OWNER: usize = 40;

/// What a Teitunnel comment marks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Marker {
    /// A route's record (its id).
    Route(String),
    /// A reservation's placeholder.
    Lease,
    /// Something a newer Teitunnel wrote.
    Other(String),
}

/// A parsed Teitunnel DNS comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ownership {
    /// What it marks.
    pub marker: Marker,
    /// Who made it (`person@machine`), when the writer said.
    pub owner: Option<String>,
    /// A route's record that also holds a reservation.
    pub lease: bool,
    /// When the reservation ends (milliseconds since the epoch); `None`: never.
    pub until: Option<u64>,
}

impl Ownership {
    /// A route's record.
    pub fn route(route_id: &str, owner: &str) -> Self {
        Self {
            marker: Marker::Route(route_id.to_owned()),
            owner: Some(owner.to_owned()),
            lease: false,
            until: None,
        }
    }

    /// A reservation's placeholder.
    pub fn lease(owner: &str, until: Option<u64>) -> Self {
        Self {
            marker: Marker::Lease,
            owner: Some(owner.to_owned()),
            lease: true,
            until,
        }
    }

    /// Reads a comment; `None` if Teitunnel didn't write it.
    pub fn parse(comment: &str) -> Option<Self> {
        let rest = comment.trim().strip_prefix(PREFIX)?;
        let mut parts = rest.split(';');
        let head = parts.next().unwrap_or_default();
        let marker = if let Some(id) = head.strip_prefix("route=") {
            Marker::Route(id.to_owned())
        } else if head == "lease" {
            Marker::Lease
        } else {
            Marker::Other(head.to_owned())
        };
        let mut ownership = Self {
            lease: marker == Marker::Lease,
            marker,
            owner: None,
            until: None,
        };
        for part in parts {
            match part.split_once('=') {
                Some(("by", owner)) if !owner.is_empty() => {
                    ownership.owner = Some(owner.to_owned());
                }
                Some(("until", at)) => ownership.until = parse_until(at),
                None if part == "lease" => ownership.lease = true,
                _ => {}
            }
        }
        Some(ownership)
    }

    /// The comment, within [`MAX_COMMENT`] characters (the owner is shortened if needed).
    pub fn render(&self) -> String {
        let mut out = String::from(PREFIX);
        match &self.marker {
            Marker::Route(id) => {
                out.push_str("route=");
                out.push_str(id);
            }
            Marker::Lease => out.push_str("lease"),
            Marker::Other(head) => out.push_str(head),
        }
        let mut tail = String::new();
        if self.lease && self.marker != Marker::Lease {
            tail.push_str(";lease");
        }
        if let Some(until) = self.until {
            tail.push_str(";until=");
            tail.push_str(&format_until(until));
        }
        if let Some(owner) = &self.owner {
            let room = MAX_COMMENT.saturating_sub(out.len() + tail.len() + ";by=".len());
            let owner: String = sanitize_owner(owner).chars().take(room).collect();
            if !owner.is_empty() {
                out.push_str(";by=");
                out.push_str(&owner);
            }
        }
        out.push_str(&tail);
        out
    }

    /// The route id, for a route's record.
    pub fn route_id(&self) -> Option<&str> {
        match &self.marker {
            Marker::Route(id) => Some(id),
            _ => None,
        }
    }

    /// Whether it holds a reservation at `now` (milliseconds): a lease that hasn't ended.
    pub fn leased_at(&self, now: u64) -> bool {
        self.lease && self.until.is_none_or(|until| until > now)
    }
}

/// The comment Teitunnel writes on a route's record: the route and who made it, keeping
/// a reservation the record held (a placeholder of `owner`'s, or its own lease).
pub fn route_comment(route_id: &str, owner: &str, previous: Option<&str>, now: u64) -> String {
    let mut ownership = Ownership::route(route_id, owner);
    if let Some(held) = previous.and_then(Ownership::parse)
        && held.leased_at(now)
        && held.owner.as_deref().is_some_and(|o| same_owner(o, owner))
    {
        ownership.lease = true;
        ownership.until = held.until;
    }
    ownership.render()
}

/// Whether two owner labels name the same person on the same machine (case-insensitive,
/// after the shortening [`Ownership::render`] may have applied).
pub fn same_owner(a: &str, b: &str) -> bool {
    /// A label shortened to fit a comment keeps at least this much.
    const SHORTENED: usize = 24;
    let (a, b) = (
        sanitize_owner(a).to_ascii_lowercase(),
        sanitize_owner(b).to_ascii_lowercase(),
    );
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    !short.is_empty() && (short == long || (short.len() >= SHORTENED && long.starts_with(&short)))
}

/// Who this is, for the comments Teitunnel writes: `TEITUNNEL_OWNER` if set (CI jobs use
/// a stable name), else `<user>@<machine>`.
pub fn owner_label() -> String {
    if let Ok(owner) = std::env::var("TEITUNNEL_OWNER")
        && !owner.trim().is_empty()
    {
        return sanitize_owner(owner.trim());
    }
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_default();
    let machine = crate::machine::machine_name();
    let label = if user.trim().is_empty() {
        machine
    } else {
        format!("{}@{machine}", user.trim())
    };
    sanitize_owner(&label)
}

/// An owner label safe inside a comment: letters, digits and `@ . _ + - /`, spaces as
/// `-`, at most 40 characters.
pub fn sanitize_owner(owner: &str) -> String {
    let mut out = String::with_capacity(owner.len());
    for c in owner.trim().chars() {
        if c.is_ascii_alphanumeric() || matches!(c, '@' | '.' | '_' | '+' | '-' | '/') {
            out.push(c);
        } else if !out.ends_with('-') {
            out.push('-');
        }
        if out.len() >= MAX_OWNER {
            break;
        }
    }
    out.trim_matches('-').to_owned()
}

/// `2026-12-31T00:00Z` for milliseconds since the epoch (UTC, minutes).
pub fn format_until(ms: u64) -> String {
    let minutes = ms / 60_000;
    let days = i64::try_from(minutes / (24 * 60)).unwrap_or(i64::MAX);
    let (year, month, day) = civil_from_days(days);
    let rest = minutes % (24 * 60);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}Z",
        rest / 60,
        rest % 60
    )
}

/// Reads `YYYY-MM-DD`, `YYYY-MM-DDTHH:MMZ` or `YYYY-MM-DDTHH:MM:SSZ` (UTC) as
/// milliseconds since the epoch. A bare date means the end of that day.
pub fn parse_until(text: &str) -> Option<u64> {
    let text = text.trim();
    let (date, time) = match text.split_once(['T', 't', ' ']) {
        Some((date, time)) => (date, Some(time)),
        None => (text, None),
    };
    let mut fields = date.split('-');
    let year: i64 = fields.next()?.parse().ok()?;
    let month: u32 = fields.next()?.parse().ok()?;
    let day: u32 = fields.next()?.parse().ok()?;
    if fields.next().is_some() || !(1970..=9999).contains(&year) || !(1..=12).contains(&month) {
        return None;
    }
    if day == 0 || day > days_in_month(year, month) {
        return None;
    }
    let seconds = match time {
        None => 24 * 3600,
        Some(time) => {
            let time = time.trim_end_matches(['Z', 'z']);
            let mut parts = time.split(':');
            let hours: u64 = parts.next()?.parse().ok()?;
            let minutes: u64 = parts.next()?.parse().ok()?;
            let secs: u64 = match parts.next() {
                Some(s) => s.parse().ok()?,
                None => 0,
            };
            if parts.next().is_some() || hours > 23 || minutes > 59 || secs > 59 {
                return None;
            }
            hours * 3600 + minutes * 60 + secs
        }
    };
    let days = u64::try_from(days_from_civil(year, month, day)).ok()?;
    Some((days * 86_400 + seconds) * 1000)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days since 1970-01-01 (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// How a name is held.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum HoldKind {
    /// Reserved (a placeholder, or a lease on a route's record).
    Reservation,
    /// Routed by another machine's tunnel.
    Route,
}

/// A hostname someone else holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Hold {
    /// The hostname.
    pub hostname: String,
    /// Who (`person@machine`); `None` when an older Teitunnel made it.
    pub owner: Option<String>,
    /// Until when (milliseconds since the epoch); `None`: no end.
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub until: Option<u64>,
    /// Reserved or routed.
    pub kind: HoldKind,
}

/// Who's asking, to tell their names from everyone else's.
#[derive(Debug, Clone, Copy)]
pub struct Me<'a> {
    /// This machine's owner label.
    pub owner: &'a str,
    /// This machine's tunnel ids in the account.
    pub tunnels: &'a [String],
    /// Now (milliseconds since the epoch).
    pub now: u64,
}

/// Whether `record` holds its name for someone other than `me`: a live reservation of
/// someone else's, or a Teitunnel route through a tunnel that isn't one of this
/// machine's. Records Teitunnel didn't write hold nothing here (they're foreign, which
/// the planner treats separately).
pub fn held_by_other(record: &cf_api::DnsRecord, me: Me<'_>) -> Option<Hold> {
    let ownership = Ownership::parse(record.comment.as_deref()?)?;
    let mine = |owner: Option<&str>| owner.is_some_and(|o| same_owner(o, me.owner));
    let target = record.content.to_ascii_lowercase();
    let tunnel = target.strip_suffix(".cfargotunnel.com");
    let hold = |kind, until| Hold {
        hostname: record.name.to_ascii_lowercase(),
        owner: ownership.owner.clone(),
        until,
        kind,
    };
    match (&ownership.marker, tunnel) {
        // Through one of this machine's tunnels, or written by this same owner elsewhere
        // (a CI job's stable label, across runs): this owner's.
        (Marker::Route(_), Some(tunnel))
            if me.tunnels.iter().any(|t| t == tunnel) || mine(ownership.owner.as_deref()) =>
        {
            None
        }
        (Marker::Route(_), Some(_)) => Some(if ownership.leased_at(me.now) {
            hold(HoldKind::Reservation, ownership.until)
        } else {
            hold(HoldKind::Route, None)
        }),
        _ if ownership.leased_at(me.now) && !mine(ownership.owner.as_deref()) => {
            Some(hold(HoldKind::Reservation, ownership.until))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: u64 = 86_400_000;

    #[test]
    fn old_comments_still_parse() {
        let old = Ownership::parse("teitunnel:route=abc123def456").unwrap();
        assert_eq!(old.route_id(), Some("abc123def456"));
        assert_eq!(old.owner, None);
        assert!(!old.lease);
        assert_eq!(old.render(), "teitunnel:route=abc123def456");
        assert_eq!(Ownership::parse("made by hand"), None);
        assert_eq!(Ownership::parse(""), None);
    }

    #[test]
    fn new_comments_round_trip() {
        let until = parse_until("2026-12-31").unwrap();
        for ownership in [
            Ownership::route("abc123def456", "alice@Alice-MacBook"),
            Ownership::lease("alice@Alice-MacBook", Some(until)),
            Ownership::lease("bob@ci", None),
            Ownership {
                lease: true,
                until: Some(until),
                ..Ownership::route("abc123def456", "alice@Alice-MacBook")
            },
        ] {
            let comment = ownership.render();
            assert!(comment.len() <= MAX_COMMENT, "{comment}");
            assert_eq!(Ownership::parse(&comment), Some(ownership), "{comment}");
        }
        assert_eq!(
            Ownership::lease("alice@Alice-MacBook", Some(until)).render(),
            "teitunnel:lease;by=alice@Alice-MacBook;until=2027-01-01T00:00Z"
        );
    }

    #[test]
    fn unknown_parts_are_ignored_and_owners_are_made_safe() {
        let parsed = Ownership::parse("teitunnel:route=r1;by=a@b;color=blue;lease").unwrap();
        assert_eq!(parsed.owner.as_deref(), Some("a@b"));
        assert!(parsed.lease);
        let odd = Ownership::route("r1", "Krishna Adhikari@Krishna's MacBook;until=x").render();
        assert_eq!(
            odd,
            "teitunnel:route=r1;by=Krishna-Adhikari@Krishna-s-MacBook-until"
        );
        let long = Ownership::lease(&"x".repeat(200), Some(DAY)).render();
        assert!(long.len() <= MAX_COMMENT);
        assert!(Ownership::parse(&long).unwrap().until.is_some());
    }

    #[test]
    fn dates_round_trip() {
        assert_eq!(format_until(0), "1970-01-01T00:00Z");
        let at = parse_until("2026-12-31T23:59Z").unwrap();
        assert_eq!(format_until(at), "2026-12-31T23:59Z");
        assert_eq!(parse_until("2024-02-29T12:30:15Z"), Some(1_709_209_815_000));
        assert_eq!(
            parse_until("2026-12-31"),
            parse_until("2027-01-01T00:00Z"),
            "a date means the end of that day"
        );
        for bad in [
            "",
            "2026-13-01",
            "2026-02-30",
            "2026-1-1x",
            "tomorrow",
            "2026-01-01T25:00Z",
        ] {
            assert_eq!(parse_until(bad), None, "{bad}");
        }
    }

    #[test]
    fn leases_end() {
        let lease = Ownership::lease("a@b", Some(10 * DAY));
        assert!(lease.leased_at(DAY));
        assert!(!lease.leased_at(10 * DAY));
        assert!(Ownership::lease("a@b", None).leased_at(u64::MAX));
        assert!(!Ownership::route("r", "a@b").leased_at(0));
    }

    #[test]
    fn a_route_keeps_its_owners_lease() {
        let lease = Ownership::lease("alice@mac", Some(10 * DAY)).render();
        let kept = route_comment("r1", "alice@mac", Some(&lease), DAY);
        assert_eq!(
            Ownership::parse(&kept),
            Some(Ownership {
                lease: true,
                until: Some(10 * DAY),
                ..Ownership::route("r1", "alice@mac")
            })
        );
        // Someone else's (a take-over) or an ended lease isn't carried over.
        assert_eq!(
            route_comment("r1", "bob@pc", Some(&lease), DAY),
            "teitunnel:route=r1;by=bob@pc"
        );
        assert_eq!(
            route_comment("r1", "alice@mac", Some(&lease), 11 * DAY),
            "teitunnel:route=r1;by=alice@mac"
        );
        assert_eq!(
            route_comment("r1", "alice@mac", Some("hand-made"), DAY),
            "teitunnel:route=r1;by=alice@mac"
        );
    }

    fn record(name: &str, content: &str, comment: Option<&str>) -> cf_api::DnsRecord {
        cf_api::DnsRecord {
            id: "rec".into(),
            name: name.into(),
            kind: if content == LEASE_ADDRESS {
                "AAAA"
            } else {
                "CNAME"
            }
            .into(),
            content: content.into(),
            proxied: true,
            comment: comment.map(str::to_owned),
            ttl: 1,
        }
    }

    #[test]
    fn tells_other_peoples_names_from_mine() {
        let tunnels = ["t-mine".to_owned()];
        let me = Me {
            owner: "alice@mac",
            tunnels: &tunnels,
            now: DAY,
        };
        let theirs = Ownership::lease("bob@pc", Some(2 * DAY)).render();
        let held = held_by_other(&record("a.xyz.com", LEASE_ADDRESS, Some(&theirs)), me).unwrap();
        assert_eq!(held.kind, HoldKind::Reservation);
        assert_eq!(held.owner.as_deref(), Some("bob@pc"));
        assert_eq!(held.until, Some(2 * DAY));
        // Expired: free.
        let ended = Me { now: 3 * DAY, ..me };
        assert_eq!(
            held_by_other(&record("a.xyz.com", LEASE_ADDRESS, Some(&theirs)), ended),
            None
        );
        // Mine.
        let mine = Ownership::lease("alice@mac", None).render();
        assert_eq!(
            held_by_other(&record("a.xyz.com", LEASE_ADDRESS, Some(&mine)), me),
            None
        );
        // Another machine's route, old or new comment.
        let route = held_by_other(
            &record(
                "b.xyz.com",
                "t-other.cfargotunnel.com",
                Some("teitunnel:route=r"),
            ),
            me,
        )
        .unwrap();
        assert_eq!((route.kind, route.owner), (HoldKind::Route, None));
        // A route through one of my tunnels is mine, whoever wrote it.
        assert_eq!(
            held_by_other(
                &record(
                    "b.xyz.com",
                    "t-mine.cfargotunnel.com",
                    Some("teitunnel:route=r;by=bob@pc")
                ),
                me
            ),
            None
        );
        // The same owner's route through another tunnel (an earlier CI run): mine.
        assert_eq!(
            held_by_other(
                &record(
                    "b.xyz.com",
                    "t-other.cfargotunnel.com",
                    Some("teitunnel:route=r;by=alice@mac")
                ),
                me
            ),
            None
        );
        // Not Teitunnel's: not a hold (it's foreign).
        assert_eq!(
            held_by_other(&record("c.xyz.com", "x.example.net", None), me),
            None
        );
    }

    #[test]
    fn owner_labels_compare_loosely() {
        assert!(same_owner("Alice@Mac", "alice@mac"));
        assert!(!same_owner("alice@mac", "alice@pc"));
        let long = format!("{}@{}", "a".repeat(30), "b".repeat(30));
        let short: String = sanitize_owner(&long);
        assert!(same_owner(&short, &long), "a shortened label still matches");
    }
}
