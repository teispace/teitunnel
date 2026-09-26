//! `teitunnel analytics` and `teitunnel uptime`, and the uptime loop `up` and `serve` run.

use std::{collections::HashSet, process::ExitCode, time::Duration};

use teitunnel_core::{
    analytics::{Analytics, AnalyticsError, AnalyticsRange, RouteRef, RouteStats, path_prefix},
    uptime::{self, Monitor, UptimeSummary},
};

use crate::context::App;

/// Milliseconds since the epoch.
fn now_ms() -> i64 {
    i64::try_from(teitunnel_core::domain_shares::now_ms()).unwrap_or(i64::MAX)
}

/// `98.5%`, or `–` without data.
pub(crate) fn percent(share: Option<f64>) -> String {
    share.map_or_else(
        || "–".to_owned(),
        |s| {
            let p = s * 100.0;
            if p >= 99.95 || p == 0.0 {
                format!("{p:.0}%")
            } else {
                format!("{p:.1}%")
            }
        },
    )
}

fn millis(ms: Option<f64>) -> String {
    ms.map_or_else(|| "–".to_owned(), |v| format!("{} ms", v.round()))
}

/// Requests per second: `12/s`, `3.4/s`, `0.05/s`, `<0.01/s`.
pub(crate) fn per_second(rate: f64) -> String {
    match rate {
        r if r >= 10.0 => format!("{r:.0}/s"),
        r if r >= 1.0 => format!("{r:.1}/s"),
        r if r >= 0.01 => format!("{r:.2}/s"),
        r if r > 0.0 => "<0.01/s".to_owned(),
        _ => "0/s".to_owned(),
    }
}

fn bytes(n: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let n = n as f64;
    match n {
        n if n >= 1e9 => format!("{:.1} GB", n / 1e9),
        n if n >= 1e6 => format!("{:.1} MB", n / 1e6),
        n if n >= 1e3 => format!("{:.1} kB", n / 1e3),
        n => format!("{n} B"),
    }
}

/// Starts checking this machine's routes every minute (unless the app or another
/// `teitunnel` already does) and prints alerts. Returns the monitor, to release its
/// lease when the command ends.
pub(crate) fn spawn_monitor(app: &App, analytics: Analytics) -> Monitor {
    let monitor = app.monitor(analytics, &format!("cli-{}", std::process::id()));
    let looping = monitor.clone();
    tokio::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        let mut tick = tokio::time::interval(uptime::INTERVAL);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let report = looping.tick(now_ms(), &HashSet::new()).await;
            for alert in &report.alerts {
                crate::share::status(&format!(
                    "{}: {}",
                    alert.title.english(),
                    alert.body.english()
                ));
            }
        }
    });
    monitor
}

/// Why analytics couldn't be read, with what to do.
fn explain(err: &AnalyticsError) -> String {
    match err {
        AnalyticsError::Permission => format!(
            "{} Edit the token at {} and add Zone ▸ Analytics ▸ Read.",
            err,
            teitunnel_core::accounts::TOKENS_PAGE
        ),
        other => other.to_string(),
    }
}

fn print_route(stats: &RouteStats) -> Result<(), String> {
    let route = stats.route.key();
    out!("{route}")?;
    out!("  Requests      {}", stats.requests)?;
    if stats.requests > 0 {
        out!(
            "  Per second    {} average · {} peak",
            per_second(stats.rate.average),
            per_second(stats.rate.peak)
        )?;
    }
    out!("  Sent          {}", bytes(stats.bytes))?;
    let c = stats.classes;
    out!(
        "  Responses     2xx {} · 3xx {} · 4xx {} · 5xx {}",
        c.ok,
        c.redirects,
        c.client_errors,
        c.server_errors
    )?;
    if let Some(origin) = stats.origin_ms {
        out!(
            "  Origin time   p50 {} · p95 {} · p99 {}",
            millis(origin.p50),
            millis(origin.p95),
            millis(origin.p99)
        )?;
    }
    for (title, rows) in [
        ("Top paths", &stats.paths),
        ("Countries", &stats.countries),
        ("Browsers", &stats.browsers),
        ("Bots", &stats.bots),
        ("Cache", &stats.cache),
    ] {
        if rows.is_empty() {
            continue;
        }
        out!("  {title}")?;
        for row in rows.iter().take(5) {
            let key = match row.key.as_str() {
                "" if title == "Bots" => "People",
                "" => "(none)",
                key => key,
            };
            out!("    {:>8}  {key}", row.requests)?;
        }
    }
    if !stats.unavailable.is_empty() {
        out!(
            "  Not on this plan: {}",
            stats
                .unavailable
                .iter()
                .map(|p| format!("{p:?}").to_lowercase())
                .collect::<Vec<_>>()
                .join(", ")
        )?;
    }
    Ok(())
}

/// `teitunnel analytics [hostname]`: the edge's numbers for one route, or all of this
/// machine's side by side.
pub(crate) async fn analytics(
    app: &App,
    hostname: Option<&str>,
    path: Option<&str>,
    range: AnalyticsRange,
    account: Option<&str>,
    json: bool,
) -> Result<ExitCode, String> {
    let analytics = Analytics::default();
    if let Some(hostname) = hostname {
        let route = RouteRef {
            hostname: hostname.trim().to_ascii_lowercase(),
            path: path
                .and_then(|p| path_prefix(p).or_else(|| p.starts_with('/').then(|| p.to_owned()))),
        };
        let only = match account {
            Some(wanted) => Some(app.account(Some(wanted)).await?.id),
            None => None,
        };
        let stats = analytics
            .route_in_any(&app.accounts, only.as_deref(), &route, range)
            .await
            .map_err(|err| match err {
                AnalyticsError::NoZone(_) => {
                    format!("No connected account has a domain for {}.", route.hostname)
                }
                other => explain(&other),
            })?;
        if json {
            out!(
                "{}",
                serde_json::to_string(&stats).map_err(|e| e.to_string())?
            )?;
        } else {
            print_route(&stats)?;
        }
        return Ok(ExitCode::SUCCESS);
    }
    let accounts = match account {
        Some(wanted) => vec![app.account(Some(wanted)).await?],
        None => app.accounts.list().await.map_err(|e| e.to_string())?,
    };
    let targets = uptime::targets(&app.accounts, app.engine.local()).await;
    let monitor = app.monitor(analytics.clone(), "cli-read");
    let uptimes = monitor
        .uptime()
        .summaries(&targets, now_ms())
        .await
        .map_err(|e| e.to_string())?;
    let mut all = Vec::new();
    for account in &accounts {
        let mut hosts: Vec<String> = targets
            .iter()
            .filter(|t| t.account_id == account.id)
            .map(|t| t.route.hostname.clone())
            .collect();
        hosts.sort();
        hosts.dedup();
        if hosts.is_empty() {
            continue;
        }
        let summary = analytics
            .summary(&app.accounts, &account.id, &hosts, range)
            .await;
        all.push((account, summary));
    }
    if json {
        let value: Vec<serde_json::Value> = all
            .iter()
            .map(|(account, summary)| match summary {
                Ok(summary) => serde_json::json!({ "accountId": account.id, "summary": summary }),
                Err(err) => serde_json::json!({ "accountId": account.id, "error": explain(err) }),
            })
            .collect();
        out!(
            "{}",
            serde_json::json!({ "accounts": value, "uptime": uptimes })
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if all.is_empty() {
        out!("This machine serves no routes yet. Add one with `teitunnel route add`.")?;
        return Ok(ExitCode::SUCCESS);
    }
    for (account, summary) in &all {
        out!("{}", account.name)?;
        match summary {
            Ok(summary) => {
                out!(
                    "  {:<40} {:>10} {:>7} {:>9} {:>8}",
                    "HOSTNAME",
                    "REQUESTS",
                    "5XX",
                    "P95",
                    "UPTIME"
                )?;
                for host in &summary.hosts {
                    let up = uptimes
                        .iter()
                        .find(|u| u.route.hostname == host.hostname && u.route.path.is_none())
                        .and_then(|u| u.uptime_day);
                    out!(
                        "  {:<40} {:>10} {:>7} {:>9} {:>8}",
                        host.hostname,
                        host.requests,
                        percent(host.error_rate),
                        millis(host.p95_ms),
                        percent(up)
                    )?;
                }
            }
            Err(err) => out!("  {}", explain(err))?,
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn describe(summary: &UptimeSummary) -> String {
    match (summary.up, &summary.open_incident) {
        (_, Some(_)) => summary.last_cause.map_or_else(
            || "down".to_owned(),
            |c| format!("down: {}", c.message().english()),
        ),
        (Some(true), None) => "up".to_owned(),
        (Some(false), None) => "failing".to_owned(),
        (None, None) => "not checked yet".to_owned(),
    }
}

/// `teitunnel uptime`: every route this machine serves, with its uptime.
pub(crate) async fn uptime(app: &App, json: bool) -> Result<ExitCode, String> {
    let monitor = app.monitor(Analytics::default(), "cli-read");
    let summaries = monitor
        .summaries(now_ms())
        .await
        .map_err(|e| e.to_string())?;
    if json {
        out!(
            "{}",
            serde_json::to_string(&summaries).map_err(|e| e.to_string())?
        )?;
        return Ok(ExitCode::SUCCESS);
    }
    if summaries.is_empty() {
        out!("This machine serves no routes yet. Add one with `teitunnel route add`.")?;
        return Ok(ExitCode::SUCCESS);
    }
    out!(
        "{:<44} {:>8} {:>8} {:>8} {:>9}  STATUS",
        "ROUTE",
        "24H",
        "7D",
        "30D",
        "P95"
    )?;
    for s in &summaries {
        out!(
            "{:<44} {:>8} {:>8} {:>8} {:>9}  {}",
            s.route.key(),
            percent(s.uptime_day),
            percent(s.uptime_week),
            percent(s.uptime_month),
            millis(s.p95_ms),
            describe(s)
        )?;
    }
    if summaries.iter().all(|s| s.up.is_none()) {
        out!(
            "Routes are checked every minute while the app, `teitunnel up` or `teitunnel serve` runs."
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

/// How long `up`/`serve` wait before the first check (connectors need a moment).
pub(crate) const FIRST_CHECK_DELAY: Duration = Duration::from_secs(20);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_values() {
        assert_eq!(percent(None), "–");
        assert_eq!(percent(Some(1.0)), "100%");
        assert_eq!(percent(Some(0.9984)), "99.8%");
        assert_eq!(percent(Some(0.0)), "0%");
        assert_eq!(millis(Some(12.4)), "12 ms");
        assert_eq!(bytes(2_500), "2.5 kB");
        assert_eq!(bytes(12), "12 B");
        assert_eq!(per_second(42.4), "42/s");
        assert_eq!(per_second(3.44), "3.4/s");
        assert_eq!(per_second(0.05), "0.05/s");
        assert_eq!(per_second(0.001), "<0.01/s");
        assert_eq!(per_second(0.0), "0/s");
    }
}
