//! Scheduled shares: a route or a share on your domain is on during set hours
//! on set days and paused (Lens's "paused" page, [`crate::pause`]) the rest of the time.
//!
//! Times are wall-clock times in a time zone (this computer's unless one is named), so a
//! change to or from daylight saving time keeps 09:00 at 09:00. A window whose end is
//! earlier than its start runs past midnight into the next day; equal start and end mean
//! the whole day. Schedules are evaluated by whoever serves the route (the app, `teitunnel
//! up` or `serve`, or the terminal running the share), on changes only: pausing or
//! resuming by hand holds until the schedule's next change.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use jiff::{
    Timestamp, ToSpan as _, Zoned,
    civil::{Date, Time},
    tz::TimeZone,
};
use rusqlite::{OptionalExtension as _, params};
use serde::{Deserialize, Serialize};

use crate::{
    store::{Store, StoreError},
    text::{Text, UserText, english_display, msg},
};

/// A day of the week.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Weekday {
    /// Monday.
    Mon,
    /// Tuesday.
    Tue,
    /// Wednesday.
    Wed,
    /// Thursday.
    Thu,
    /// Friday.
    Fri,
    /// Saturday.
    Sat,
    /// Sunday.
    Sun,
}

impl Weekday {
    /// Monday to Sunday.
    pub const ALL: [Self; 7] = [
        Self::Mon,
        Self::Tue,
        Self::Wed,
        Self::Thu,
        Self::Fri,
        Self::Sat,
        Self::Sun,
    ];

    fn of(date: Date) -> Self {
        Self::ALL[usize::try_from(date.weekday().to_monday_zero_offset()).unwrap_or_default()]
    }

    fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|d| *d == self)
            .unwrap_or_default()
    }

    /// The day's three-letter name (`mon`).
    pub fn name(self) -> &'static str {
        ["mon", "tue", "wed", "thu", "fri", "sat", "sun"][self.index()]
    }

    fn parse(text: &str) -> Option<Self> {
        let text = text.trim().to_ascii_lowercase();
        let prefix = text.get(..3)?;
        Self::ALL.into_iter().find(|d| d.name() == prefix)
    }
}

/// When a route is on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    /// The days a window starts on, Monday first.
    pub days: Vec<Weekday>,
    /// Start, `HH:MM` (24-hour).
    pub from: String,
    /// End, `HH:MM`. Earlier than `from`: the next day. Equal: the whole day.
    pub to: String,
    /// An IANA time zone, e.g. `Europe/Berlin` (`None`: this computer's).
    pub time_zone: Option<String>,
}

/// Why a schedule isn't valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ScheduleError {
    /// No days, or a day that isn't one.
    Days(String),
    /// A time that isn't `HH:MM`.
    Time(String),
    /// An unknown time zone.
    TimeZone(String),
    /// Not `DAYS HH:MM-HH:MM`.
    Spec(String),
}

impl UserText for ScheduleError {
    fn text(&self) -> Text {
        use msg::error::schedule as m;
        match self {
            Self::Days(days) => m::days(days),
            Self::Time(time) => m::time(time),
            Self::TimeZone(zone) => m::time_zone(zone),
            Self::Spec(spec) => m::spec(spec),
        }
    }
}

english_display!(ScheduleError);

fn parse_time(text: &str) -> Result<Time, ScheduleError> {
    let invalid = || ScheduleError::Time(text.to_owned());
    let (hours, minutes) = text.trim().split_once(':').ok_or_else(invalid)?;
    let digits = |s: &str, lengths: std::ops::RangeInclusive<usize>| {
        lengths.contains(&s.len()) && s.chars().all(|c| c.is_ascii_digit())
    };
    if !digits(hours, 1..=2) || !digits(minutes, 2..=2) {
        return Err(invalid());
    }
    let hours: i8 = hours.parse().map_err(|_| invalid())?;
    let minutes: i8 = minutes.parse().map_err(|_| invalid())?;
    Time::new(hours, minutes, 0, 0).map_err(|_| invalid())
}

/// Days from text: `mon-fri`, `mon,wed,fri`, `weekdays`, `weekends`, `daily`.
///
/// # Errors
/// [`ScheduleError::Days`].
pub fn parse_days(text: &str) -> Result<Vec<Weekday>, ScheduleError> {
    let invalid = || ScheduleError::Days(text.to_owned());
    let lower = text.trim().to_ascii_lowercase();
    let mut days = match lower.as_str() {
        "daily" | "every day" | "everyday" | "all" => Weekday::ALL.to_vec(),
        "weekdays" => Weekday::ALL[..5].to_vec(),
        "weekends" | "weekend" => Weekday::ALL[5..].to_vec(),
        _ => {
            let mut days = Vec::new();
            for part in lower.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                match part.split_once('-') {
                    Some((first, last)) => {
                        let first = Weekday::parse(first).ok_or_else(invalid)?.index();
                        let last = Weekday::parse(last).ok_or_else(invalid)?.index();
                        let mut day = first;
                        loop {
                            days.push(Weekday::ALL[day]);
                            if day == last {
                                break;
                            }
                            day = (day + 1) % 7;
                        }
                    }
                    None => days.push(Weekday::parse(part).ok_or_else(invalid)?),
                }
            }
            days
        }
    };
    days.sort();
    days.dedup();
    if days.is_empty() {
        return Err(invalid());
    }
    Ok(days)
}

impl Schedule {
    /// A checked schedule (days sorted, times as `HH:MM`).
    ///
    /// # Errors
    /// See [`ScheduleError`].
    pub fn new(
        days: Vec<Weekday>,
        from: &str,
        to: &str,
        time_zone: Option<&str>,
    ) -> Result<Self, ScheduleError> {
        let mut days = days;
        days.sort();
        days.dedup();
        if days.is_empty() {
            return Err(ScheduleError::Days(String::new()));
        }
        let (from, to) = (parse_time(from)?, parse_time(to)?);
        let time_zone = time_zone
            .map(str::trim)
            .filter(|z| !z.is_empty())
            .map(str::to_owned);
        if let Some(zone) = &time_zone {
            TimeZone::get(zone).map_err(|_| ScheduleError::TimeZone(zone.clone()))?;
        }
        Ok(Self {
            days,
            from: format!("{:02}:{:02}", from.hour(), from.minute()),
            to: format!("{:02}:{:02}", to.hour(), to.minute()),
            time_zone,
        })
    }

    /// A schedule from `DAYS HH:MM-HH:MM`, e.g. `mon-fri 09:00-18:00`.
    ///
    /// # Errors
    /// See [`ScheduleError`].
    pub fn parse(spec: &str, time_zone: Option<&str>) -> Result<Self, ScheduleError> {
        let spec = spec.trim();
        let (days, hours) = spec
            .rsplit_once(char::is_whitespace)
            .ok_or_else(|| ScheduleError::Spec(spec.to_owned()))?;
        let (from, to) = hours
            .split_once('-')
            .ok_or_else(|| ScheduleError::Spec(spec.to_owned()))?;
        Self::new(parse_days(days)?, from, to, time_zone)
    }

    /// Checks a schedule that came from outside (IPC, the database).
    ///
    /// # Errors
    /// See [`ScheduleError`].
    pub fn validated(self) -> Result<Self, ScheduleError> {
        Self::new(self.days, &self.from, &self.to, self.time_zone.as_deref())
    }

    fn zone(&self) -> Result<TimeZone, ScheduleError> {
        match &self.time_zone {
            Some(zone) => TimeZone::get(zone).map_err(|_| ScheduleError::TimeZone(zone.clone())),
            None => Ok(TimeZone::system()),
        }
    }

    /// `(start, end)` of every window that starts from two days before `now` to eight
    /// days after, in order.
    fn windows(&self, now: Timestamp) -> Result<Vec<(Timestamp, Timestamp)>, ScheduleError> {
        let zone = self.zone()?;
        let (from, to) = (parse_time(&self.from)?, parse_time(&self.to)?);
        let today = now.to_zoned(zone.clone()).date();
        let at = |date: Date, time: Time| -> Option<Timestamp> {
            date.to_datetime(time)
                .to_zoned(zone.clone())
                .ok()
                .map(|z: Zoned| z.timestamp())
        };
        let mut windows = Vec::new();
        for offset in -2i32..=8 {
            let Ok(date) = today.checked_add(offset.days()) else {
                continue;
            };
            if !self.days.contains(&Weekday::of(date)) {
                continue;
            }
            let end_date = if to <= from {
                date.checked_add(1.day()).ok()
            } else {
                Some(date)
            };
            if let (Some(start), Some(end)) = (at(date, from), end_date.and_then(|d| at(d, to)))
                && start < end
            {
                windows.push((start, end));
            }
        }
        Ok(windows)
    }

    /// Whether the route is on at `now`.
    ///
    /// # Errors
    /// An unknown time zone or a malformed time (a schedule not made by [`Self::new`]).
    pub fn is_on(&self, now: Timestamp) -> Result<bool, ScheduleError> {
        Ok(self
            .windows(now)?
            .iter()
            .any(|(start, end)| *start <= now && now < *end))
    }

    /// When it next turns on or off after `now` (`None`: not within a week).
    ///
    /// # Errors
    /// As [`Self::is_on`].
    pub fn next_change(&self, now: Timestamp) -> Result<Option<Timestamp>, ScheduleError> {
        let windows = self.windows(now)?;
        let on = windows.iter().any(|(s, e)| *s <= now && now < *e);
        if on {
            // The end of the window we're in, following windows that touch or overlap.
            let mut end = now;
            loop {
                let next = windows
                    .iter()
                    .filter(|(s, e)| *s <= end && *e > end)
                    .map(|(_, e)| *e)
                    .max();
                match next {
                    Some(later) => end = later,
                    None => break,
                }
            }
            // On until the last window looked at: always on, as far as we can tell.
            let horizon = windows.iter().map(|(_, e)| *e).max();
            Ok((end > now && Some(end) != horizon).then_some(end))
        } else {
            Ok(windows.iter().map(|(s, _)| *s).filter(|s| *s > now).min())
        }
    }
}

/// Where a schedule applies, and when it next changes (for the UI).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteSchedule {
    /// Account id.
    pub account_id: String,
    /// The route's hostname.
    pub hostname: String,
    /// The schedule.
    pub schedule: Schedule,
    /// On right now.
    pub on: bool,
    /// When it next changes (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = Option<f64>))]
    pub next_change: Option<u64>,
}

fn millis(timestamp: Timestamp) -> u64 {
    u64::try_from(timestamp.as_millisecond()).unwrap_or_default()
}

impl RouteSchedule {
    fn at(account_id: String, hostname: String, schedule: Schedule, now: Timestamp) -> Self {
        Self {
            on: schedule.is_on(now).unwrap_or(true),
            next_change: schedule.next_change(now).ok().flatten().map(millis),
            account_id,
            hostname,
            schedule,
        }
    }
}

/// Schedules, in `account` or everywhere, by hostname.
///
/// # Errors
/// The database can't be read.
pub async fn list(store: &Store, account: Option<&str>) -> Result<Vec<RouteSchedule>, StoreError> {
    let account = account.map(str::to_owned);
    let rows = store
        .call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT account_id, hostname, schedule FROM route_schedules
                 WHERE ?1 IS NULL OR account_id = ?1 ORDER BY hostname",
            )?;
            let rows = stmt.query_map(params![account], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        })
        .await?;
    let now = Timestamp::now();
    Ok(rows
        .into_iter()
        .filter_map(|(account, hostname, raw)| {
            let schedule: Schedule = serde_json::from_str(&raw).ok()?;
            Some(RouteSchedule::at(account, hostname, schedule, now))
        })
        .collect())
}

/// The schedule of one route, if it has one.
///
/// # Errors
/// The database can't be read.
pub async fn get(
    store: &Store,
    account: &str,
    hostname: &str,
) -> Result<Option<RouteSchedule>, StoreError> {
    let (account, hostname) = (account.to_owned(), hostname.trim().to_ascii_lowercase());
    let raw = {
        let (account, hostname) = (account.clone(), hostname.clone());
        store
            .call(move |conn| {
                Ok(conn
                    .query_row(
                        "SELECT schedule FROM route_schedules WHERE account_id = ?1 AND hostname = ?2",
                        params![account, hostname],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?)
            })
            .await?
    };
    Ok(raw
        .and_then(|raw| serde_json::from_str::<Schedule>(&raw).ok())
        .map(|schedule| RouteSchedule::at(account, hostname, schedule, Timestamp::now())))
}

/// Sets (or, with `None`, removes) a route's schedule. Removing it doesn't resume a
/// route the schedule paused; [`Scheduler::tick`] does that.
///
/// # Errors
/// The database can't be written.
pub async fn set(
    store: &Store,
    account: &str,
    hostname: &str,
    schedule: Option<&Schedule>,
) -> Result<(), StoreError> {
    let (account, hostname) = (account.to_owned(), hostname.trim().to_ascii_lowercase());
    let raw = schedule.map(serde_json::to_string).transpose()?;
    let now = i64::try_from(crate::domain_shares::now_ms()).unwrap_or(i64::MAX);
    store
        .call(move |conn| {
            match raw {
                Some(raw) => conn.execute(
                    "INSERT INTO route_schedules (account_id, hostname, schedule, created_at)
                     VALUES (?1, ?2, ?3, ?4)
                     ON CONFLICT (account_id, hostname) DO UPDATE SET schedule = ?3",
                    params![account, hostname, raw, now],
                )?,
                None => conn.execute(
                    "DELETE FROM route_schedules WHERE account_id = ?1 AND hostname = ?2",
                    params![account, hostname],
                )?,
            };
            Ok(())
        })
        .await
}

/// What a tick decided for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// Account id.
    pub account_id: String,
    /// Hostname.
    pub hostname: String,
    /// `true`: its window began (resume it if the schedule paused it); `false`: it
    /// ended (pause it).
    pub on: bool,
}

/// Remembers what each schedule said last, so only changes act (a person pausing or
/// resuming by hand isn't undone until the schedule's next change).
#[derive(Debug, Default)]
pub struct Scheduler {
    last: Mutex<HashMap<(String, String), bool>>,
}

impl Scheduler {
    /// A scheduler that hasn't seen any schedule yet (its first tick acts on each).
    pub fn new() -> Self {
        Self::default()
    }

    /// The turns due at `now` for `schedules` (those this process serves). Schedules that
    /// went away are forgotten, and `on: true` is reported for them once, so a route a
    /// removed schedule had paused comes back.
    pub fn tick(&self, schedules: &[RouteSchedule], now: Timestamp) -> Vec<Turn> {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        let mut turns = Vec::new();
        let mut seen = Vec::new();
        for entry in schedules {
            let key = (entry.account_id.clone(), entry.hostname.clone());
            let Ok(on) = entry.schedule.is_on(now) else {
                continue;
            };
            if last.insert(key.clone(), on) != Some(on) {
                turns.push(Turn {
                    account_id: key.0.clone(),
                    hostname: key.1.clone(),
                    on,
                });
            }
            seen.push(key);
        }
        let gone: Vec<(String, String)> = last
            .keys()
            .filter(|key| !seen.contains(key))
            .cloned()
            .collect();
        for key in gone {
            if last.remove(&key) == Some(false) {
                turns.push(Turn {
                    account_id: key.0,
                    hostname: key.1,
                    on: true,
                });
            }
        }
        turns
    }
}

/// Evaluates the schedules this process serves and asks for the pauses and resumes they
/// call for (the [`crate::pause::Enforcer`] then applies them): those of shares on your
/// domain `owner` started, and, when `host` (this process holds the route host lease),
/// those of routes. Returns what couldn't be done (tried again at the next change).
pub async fn run_tick(
    store: &Store,
    scheduler: &Scheduler,
    owner: &str,
    host: bool,
    now: Timestamp,
) -> Vec<Text> {
    let (Ok(schedules), Ok(shares)) = (
        list(store, None).await,
        crate::engine::Local::new(store.clone()).shares(None).await,
    ) else {
        return Vec::new();
    };
    let mine: Vec<RouteSchedule> = schedules
        .into_iter()
        .filter(|entry| {
            match shares
                .iter()
                .find(|s| s.account_id == entry.account_id && s.hostname == entry.hostname)
            {
                Some(share) => share.owner == owner,
                None => host,
            }
        })
        .collect();
    let mut failures = Vec::new();
    for turn in scheduler.tick(&mine, now) {
        if turn.on {
            let paused = crate::pause::find(store, &turn.account_id, &turn.hostname).await;
            if let Ok(Some(paused)) = paused
                && paused.by_schedule
                && let Err(err) =
                    crate::pause::request_resume(store, &turn.account_id, &turn.hostname).await
            {
                failures.push(err.text());
            }
        } else if let Err(err) =
            crate::pause::request(store, &turn.account_id, &turn.hostname, true).await
        {
            failures.push(err.text());
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn at(text: &str) -> Timestamp {
        text.parse::<Zoned>().unwrap().timestamp()
    }

    fn berlin(spec: &str) -> Schedule {
        Schedule::parse(spec, Some("Europe/Berlin")).unwrap()
    }

    #[test]
    fn parses_days_and_specs() {
        assert_eq!(parse_days("mon-fri").unwrap(), Weekday::ALL[..5].to_vec());
        assert_eq!(
            parse_days("weekends").unwrap(),
            [Weekday::Sat, Weekday::Sun]
        );
        assert_eq!(
            parse_days("fri-mon").unwrap(),
            [Weekday::Mon, Weekday::Fri, Weekday::Sat, Weekday::Sun]
        );
        assert_eq!(
            parse_days("Wed, monday").unwrap(),
            [Weekday::Mon, Weekday::Wed]
        );
        assert_eq!(parse_days("daily").unwrap().len(), 7);
        assert!(parse_days("").is_err());
        assert!(parse_days("someday").is_err());

        let schedule = Schedule::parse("mon-fri 9:00-18:30", None).unwrap();
        assert_eq!(
            (schedule.from.as_str(), schedule.to.as_str()),
            ("09:00", "18:30")
        );
        assert!(Schedule::parse("mon-fri", None).is_err());
        assert!(Schedule::parse("mon-fri 25:00-18:00", None).is_err());
        assert!(Schedule::parse("mon-fri 09:00-18:60", None).is_err());
        assert!(Schedule::parse("mon 09:00-10:00", Some("Mars/Olympus")).is_err());
    }

    #[test]
    fn office_hours() {
        let schedule = berlin("mon-fri 09:00-18:00");
        // Friday 2026-09-25.
        assert!(
            schedule
                .is_on(at("2026-09-25T10:00[Europe/Berlin]"))
                .unwrap()
        );
        assert!(
            !schedule
                .is_on(at("2026-09-25T08:59[Europe/Berlin]"))
                .unwrap()
        );
        assert!(
            !schedule
                .is_on(at("2026-09-25T18:00[Europe/Berlin]"))
                .unwrap()
        );
        assert!(
            !schedule
                .is_on(at("2026-09-26T10:00[Europe/Berlin]"))
                .unwrap()
        );
        assert_eq!(
            schedule
                .next_change(at("2026-09-25T10:00[Europe/Berlin]"))
                .unwrap(),
            Some(at("2026-09-25T18:00[Europe/Berlin]"))
        );
        // After Friday evening, next Monday morning.
        assert_eq!(
            schedule
                .next_change(at("2026-09-25T19:00[Europe/Berlin]"))
                .unwrap(),
            Some(at("2026-09-28T09:00[Europe/Berlin]"))
        );
    }

    #[test]
    fn windows_past_midnight_belong_to_their_start_day() {
        let schedule = berlin("fri 22:00-02:00");
        assert!(
            schedule
                .is_on(at("2026-09-25T23:00[Europe/Berlin]"))
                .unwrap()
        );
        assert!(
            schedule
                .is_on(at("2026-09-26T01:59[Europe/Berlin]"))
                .unwrap()
        );
        assert!(
            !schedule
                .is_on(at("2026-09-26T02:00[Europe/Berlin]"))
                .unwrap()
        );
        assert!(
            !schedule
                .is_on(at("2026-09-24T23:00[Europe/Berlin]"))
                .unwrap()
        );
    }

    #[test]
    fn whole_days_and_touching_windows() {
        let schedule = berlin("daily 00:00-00:00");
        assert!(
            schedule
                .is_on(at("2026-09-25T12:00[Europe/Berlin]"))
                .unwrap()
        );
        assert_eq!(
            schedule
                .next_change(at("2026-09-25T12:00[Europe/Berlin]"))
                .unwrap(),
            None,
            "always on within the week looked at"
        );
    }

    #[test]
    fn daylight_saving_keeps_the_wall_clock() {
        // Berlin leaves summer time on 2026-10-25 (Sunday).
        let schedule = berlin("daily 09:00-10:00");
        assert!(
            schedule
                .is_on(at("2026-10-24T09:30[Europe/Berlin]"))
                .unwrap()
        );
        assert!(
            schedule
                .is_on(at("2026-10-26T09:30[Europe/Berlin]"))
                .unwrap()
        );
        assert!(
            !schedule
                .is_on(at("2026-10-26T10:30[Europe/Berlin]"))
                .unwrap()
        );
        // And in another zone the same instant is judged by Berlin's clock.
        assert!(schedule.is_on(at("2026-10-26T08:30[UTC]")).unwrap());
    }

    fn entry(host: &str, schedule: Schedule) -> RouteSchedule {
        RouteSchedule {
            account_id: "a".into(),
            hostname: host.into(),
            schedule,
            on: true,
            next_change: None,
        }
    }

    #[test]
    fn ticks_act_on_changes_only() {
        let scheduler = Scheduler::new();
        let schedules = [entry("x.example.com", berlin("mon-fri 09:00-18:00"))];
        let evening = at("2026-09-25T19:00[Europe/Berlin]");
        let first = scheduler.tick(&schedules, evening);
        assert_eq!(first.len(), 1, "the first tick acts");
        assert!(!first[0].on);
        assert!(scheduler.tick(&schedules, evening).is_empty());
        let monday = at("2026-09-28T09:00[Europe/Berlin]");
        assert!(scheduler.tick(&schedules, monday)[0].on);
        let later = at("2026-09-28T19:00[Europe/Berlin]");
        assert!(!scheduler.tick(&schedules, later)[0].on);
        // The schedule is removed while it holds the route paused: it comes back.
        let turns = scheduler.tick(&[], later);
        assert_eq!(
            turns,
            [Turn {
                account_id: "a".into(),
                hostname: "x.example.com".into(),
                on: true
            }]
        );
        assert!(scheduler.tick(&[], later).is_empty());
    }

    #[tokio::test]
    async fn stored_per_route() {
        let store = Store::open_in_memory().unwrap();
        let schedule = berlin("mon-fri 09:00-18:00");
        set(&store, "a", "X.example.com", Some(&schedule))
            .await
            .unwrap();
        let listed = list(&store, Some("a")).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].hostname, "x.example.com");
        assert_eq!(listed[0].schedule, schedule);
        assert!(get(&store, "a", "x.example.com").await.unwrap().is_some());
        set(&store, "a", "x.example.com", None).await.unwrap();
        assert!(list(&store, None).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn ticks_pause_and_resume_what_this_process_serves() {
        use crate::domain_shares::{APP_OWNER, DomainShare};
        let store = Store::open_in_memory().unwrap();
        let local = crate::engine::Local::new(store.clone());
        for (hostname, owner) in [("mine.xyz.com", APP_OWNER), ("theirs.xyz.com", "1-1")] {
            local
                .record_share(&DomainShare {
                    account_id: "a".into(),
                    hostname: hostname.into(),
                    origin: "3000".into(),
                    owner: owner.into(),
                    expires_at: None,
                    created_at: 1,
                    source: None,
                    folder: false,
                    paused: false,
                    schedule: None,
                })
                .await
                .unwrap();
        }
        let office = berlin("mon-fri 09:00-18:00");
        for hostname in ["mine.xyz.com", "theirs.xyz.com", "route.xyz.com"] {
            set(&store, "a", hostname, Some(&office)).await.unwrap();
        }
        let scheduler = Scheduler::new();
        let evening = at("2026-09-25T19:00[Europe/Berlin]");
        // Not the host: only the app's own share is paused.
        assert!(
            run_tick(&store, &scheduler, APP_OWNER, false, evening)
                .await
                .is_empty()
        );
        let paused: Vec<String> = crate::pause::list(&store, None)
            .await
            .unwrap()
            .into_iter()
            .map(|p| p.hostname)
            .collect();
        assert_eq!(paused, ["mine.xyz.com"]);
        assert!(
            crate::pause::find(&store, "a", "mine.xyz.com")
                .await
                .unwrap()
                .unwrap()
                .by_schedule
        );
        // Monday morning: resumed.
        let monday = at("2026-09-28T09:30[Europe/Berlin]");
        run_tick(&store, &scheduler, APP_OWNER, false, monday).await;
        assert!(crate::pause::list(&store, None).await.unwrap().is_empty());
        // Paused by hand during the window: the schedule leaves it alone at its next "on".
        crate::pause::request(&store, "a", "mine.xyz.com", false)
            .await
            .unwrap();
        run_tick(&store, &scheduler, APP_OWNER, false, monday).await;
        assert_eq!(crate::pause::list(&store, None).await.unwrap().len(), 1);
    }

    fn weekday() -> impl Strategy<Value = Weekday> {
        (0usize..7).prop_map(|i| Weekday::ALL[i])
    }

    proptest! {
        /// Whatever the schedule, `next_change` is where `is_on` flips: just before it
        /// the state is the current one, and at it the other.
        #[test]
        fn next_change_is_where_the_state_flips(
            days in proptest::collection::vec(weekday(), 1..7),
            from in (0i8..24, 0i8..60),
            to in (0i8..24, 0i8..60),
            offset_minutes in 0i64..(14 * 24 * 60),
        ) {
            let schedule = Schedule::new(
                days,
                &format!("{:02}:{:02}", from.0, from.1),
                &format!("{:02}:{:02}", to.0, to.1),
                Some("Europe/Berlin"),
            ).unwrap();
            let now = at("2026-09-21T00:00[Europe/Berlin]")
                .checked_add(offset_minutes.minutes())
                .unwrap();
            let on = schedule.is_on(now).unwrap();
            if let Some(change) = schedule.next_change(now).unwrap() {
                prop_assert!(change > now);
                let before = change.checked_sub(1.second()).unwrap();
                prop_assert_eq!(schedule.is_on(before).unwrap(), on);
                prop_assert_eq!(schedule.is_on(change).unwrap(), !on);
            }
        }

        #[test]
        fn day_lists_parse_back(days in proptest::collection::vec(weekday(), 1..7)) {
            let text: Vec<&str> = days.iter().map(|d| d.name()).collect();
            let mut expected = days.clone();
            expected.sort();
            expected.dedup();
            prop_assert_eq!(parse_days(&text.join(",")).unwrap(), expected);
        }
    }
}
