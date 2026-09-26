//! Sharing extras from the terminal: pausing and resuming (`shares --pause`,
//! `--resume`), schedules (`schedule`, `schedules`), and the loops that apply them in
//! the processes that serve routes: a terminal's `share --on`, `teitunnel mcp`, and
//! `teitunnel up`/`serve` (which also hold the route host lease when the app doesn't).

use std::{process::ExitCode, sync::Arc, time::Duration};

use jiff::Timestamp;
use teitunnel_core::{
    inspect::{Inspector, routes},
    machine::MachineTunnels,
    pause::{self, Enforcer, PauseError},
    schedule::{self, Schedule, Scheduler},
    store::Store,
    text::UserText as _,
};
use tokio::task::JoinHandle;

use crate::{context::App, share::status};

/// How often pauses are applied.
const PAUSE_EVERY: Duration = Duration::from_secs(2);
/// How often schedules are looked at, in pause ticks (30 s).
const SCHEDULE_TICKS: u32 = 15;

/// Applies the pauses and schedules of this process's own shares on your domain (its
/// taps show the paused page) until aborted.
pub(crate) fn spawn_share_loop(store: Store, inspector: Inspector) -> JoinHandle<()> {
    tokio::spawn(async move {
        let enforcer = Enforcer::new();
        let scheduler = Scheduler::new();
        let owner = inspector.owner().to_owned();
        let mut tick = tokio::time::interval(PAUSE_EVERY);
        let mut count = 0u32;
        let mut paused: Vec<String> = Vec::new();
        loop {
            tick.tick().await;
            if count.is_multiple_of(SCHEDULE_TICKS) {
                for failure in
                    schedule::run_tick(&store, &scheduler, &owner, false, Timestamp::now()).await
                {
                    status(&format!("Schedule: {}", failure.english()));
                }
            }
            count = count.wrapping_add(1);
            let _ = enforcer.sync_taps(&store, &inspector).await;
            let now: Vec<String> = pause::list(&store, None)
                .await
                .unwrap_or_default()
                .into_iter()
                .filter(|p| p.owner == owner)
                .map(|p| p.hostname)
                .collect();
            for hostname in now.iter().filter(|h| !paused.contains(h)) {
                status(&format!(
                    "https://{hostname} is paused: visitors see a paused page."
                ));
            }
            for hostname in paused.iter().filter(|h| !now.contains(h)) {
                status(&format!("https://{hostname} is served again."));
            }
            paused = now;
        }
    })
}

/// `up` and `serve`: serve paused pages and run schedules for this machine's routes
/// while the app doesn't (the route host lease), and for this process's own shares.
pub(crate) struct RouteHost {
    task: JoinHandle<()>,
    owner: String,
}

impl RouteHost {
    /// Starts it.
    pub(crate) fn spawn(app: &App, machine: MachineTunnels, inspector: Inspector) -> Self {
        let owner = inspector.owner().to_owned();
        let (accounts, engine, name, store) = (
            app.accounts.clone(),
            Arc::clone(&app.engine),
            app.machine_name.clone(),
            app.store().clone(),
        );
        let task = {
            let owner = owner.clone();
            tokio::spawn(async move {
                let enforcer = Enforcer::new();
                let scheduler = Scheduler::new();
                let mut tick = tokio::time::interval(Duration::from_secs(3));
                let mut count = 0u32;
                let mut host = false;
                loop {
                    tick.tick().await;
                    if count.is_multiple_of(10) {
                        let claimed = pause::claim_host(&store, &owner, false)
                            .await
                            .unwrap_or(false);
                        if claimed && !host {
                            status("Pausing and schedules of this machine's routes run here.");
                        }
                        host = claimed;
                        for failure in
                            schedule::run_tick(&store, &scheduler, &owner, host, Timestamp::now())
                                .await
                        {
                            status(&format!("Schedule: {}", failure.english()));
                        }
                    }
                    count = count.wrapping_add(1);
                    for failure in enforcer
                        .sync(&accounts, &engine, &machine, &name, &inspector)
                        .await
                    {
                        status(&format!("Pause: {}", failure.english()));
                    }
                }
            })
        };
        Self { task, owner }
    }

    /// Stops it: gives the lease up and points routes this process inspected for a
    /// pause back at their services.
    pub(crate) async fn stop(self, app: &App, machine: &MachineTunnels, inspector: &Inspector) {
        self.task.abort();
        let _ = pause::release_host(app.store(), &self.owner).await;
        let owner = self.owner.clone();
        for failure in routes::sweep(
            &app.accounts,
            &app.engine,
            machine,
            &app.machine_name,
            Some(inspector),
            move |route| route.owner == owner,
        )
        .await
        {
            status(&format!(
                "Couldn't point a route back at its service: {}",
                failure.english()
            ));
        }
    }
}

/// The account a hostname belongs to: its share's, or the one named (or the only one).
async fn account_for(app: &App, hostname: &str, wanted: Option<&str>) -> Result<String, String> {
    if wanted.is_none()
        && let Some(account) = pause::share_account(app.store(), hostname)
            .await
            .map_err(|e| e.to_string())?
    {
        return Ok(account);
    }
    Ok(app.account(wanted).await?.id)
}

/// `teitunnel shares --pause|--resume <hostname>` without the app: recorded here, applied
/// by the process serving it.
pub(crate) async fn pause_here(
    app: &App,
    id: &str,
    account: Option<&str>,
    paused: bool,
) -> Result<ExitCode, String> {
    let hostname = pause::hostname_of(id);
    let account = account_for(app, &hostname, account).await?;
    if paused {
        pause::request(app.store(), &account, &hostname, false)
            .await
            .map_err(|e: PauseError| e.text().english())?;
        out!(
            "Paused https://{hostname}. Visitors see a paused page until `teitunnel shares --resume {hostname}`."
        )?;
    } else {
        let was = pause::request_resume(app.store(), &account, &hostname)
            .await
            .map_err(|e| e.to_string())?;
        if was {
            out!("https://{hostname} is served again.")?;
        } else {
            out!("https://{hostname} wasn't paused.")?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// `teitunnel schedule <hostname> [DAYS HOURS] [--tz] [--off]`.
pub(crate) async fn set_schedule(
    app: &App,
    hostname: &str,
    spec: &[String],
    time_zone: Option<&str>,
    off: bool,
    account: Option<&str>,
) -> Result<ExitCode, String> {
    let hostname = pause::hostname_of(hostname);
    let account = account_for(app, &hostname, account).await?;
    if off {
        schedule::set(app.store(), &account, &hostname, None)
            .await
            .map_err(|e| e.to_string())?;
        out!("https://{hostname} has no schedule any more (it stays as it is now).")?;
        return Ok(ExitCode::SUCCESS);
    }
    if spec.is_empty() {
        return match schedule::get(app.store(), &account, &hostname)
            .await
            .map_err(|e| e.to_string())?
        {
            Some(entry) => {
                out!("{}", schedule_line(&entry))?;
                Ok(ExitCode::SUCCESS)
            }
            None => {
                out!(
                    "https://{hostname} has no schedule. Set one: `teitunnel schedule {hostname} mon-fri 09:00-18:00`."
                )?;
                Ok(ExitCode::SUCCESS)
            }
        };
    }
    let schedule = Schedule::parse(&spec.join(" "), time_zone).map_err(|e| e.text().english())?;
    schedule::set(app.store(), &account, &hostname, Some(&schedule))
        .await
        .map_err(|e| e.to_string())?;
    if let Some(entry) = schedule::get(app.store(), &account, &hostname)
        .await
        .map_err(|e| e.to_string())?
    {
        out!("{}", schedule_line(&entry))?;
    }
    let shared = pause::share_account(app.store(), &hostname)
        .await
        .ok()
        .flatten()
        .is_some();
    let host = pause::host(app.store()).await.ok().flatten();
    if !shared && host.is_none() {
        status(
            "Nothing on this computer runs schedules right now: open the app, or run `teitunnel up` or `teitunnel serve`.",
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn when(ms: u64) -> String {
    i64::try_from(ms)
        .ok()
        .and_then(|ms| Timestamp::from_millisecond(ms).ok())
        .map_or_else(String::new, |t| {
            t.to_zoned(jiff::tz::TimeZone::system())
                .strftime("%a %H:%M")
                .to_string()
        })
}

fn schedule_line(entry: &schedule::RouteSchedule) -> String {
    let s = &entry.schedule;
    let days: Vec<&str> = s.days.iter().map(|d| d.name()).collect();
    let zone = s.time_zone.as_deref().unwrap_or("local time");
    let now = if entry.on { "on" } else { "paused" };
    let next = entry.next_change.map_or_else(String::new, |at| {
        format!(
            ", {} at {}",
            if entry.on { "pauses" } else { "starts" },
            when(at)
        )
    });
    format!(
        "https://{}\t{} {}-{} ({zone})\t{now}{next}",
        entry.hostname,
        days.join(","),
        s.from,
        s.to
    )
}

/// `teitunnel schedules`.
pub(crate) async fn list_schedules(app: &App, json: bool) -> Result<ExitCode, String> {
    let schedules = schedule::list(app.store(), None)
        .await
        .map_err(|e| e.to_string())?;
    if json {
        out!(
            "{}",
            serde_json::to_string(&schedules).map_err(|e| e.to_string())?
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if schedules.is_empty() {
        out!("No schedules. Set one: `teitunnel schedule demo.teispace.com mon-fri 09:00-18:00`.")?;
    }
    for entry in &schedules {
        out!("{}", schedule_line(entry))?;
    }
    Ok(ExitCode::SUCCESS)
}
