//! Analytics, uptime and alert commands (thin wrappers over `core::analytics`,
//! `core::uptime` and `core::alerts`).

use tauri::{AppHandle, State};
use tauri_specta::Event;
use teitunnel_core::{
    alerts::{self, AlertRules},
    analytics::{AnalyticsRange, AnalyticsSummary, RouteRef, RouteStats},
    domain::Hostname,
    uptime::{UptimeDetail, UptimeSummary},
};

use crate::{
    error::AppError,
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

fn now_ms() -> i64 {
    i64::try_from(teitunnel_core::domain_shares::now_ms()).unwrap_or(i64::MAX)
}

fn hostname(value: &str) -> Result<String, AppError> {
    Hostname::parse(value)
        .map(|h| h.as_str().to_owned())
        .map_err(|err| AppError::invalid("hostname", teitunnel_core::text::UserText::text(&err)))
}

/// Traffic of `hostnames` (in one of the account's domains) side by side, from
/// Cloudflare's edge. Cached, so polling it is cheap.
#[tauri::command]
#[specta::specta]
pub async fn analytics_summary(
    state: State<'_, AppState>,
    account_id: String,
    hostnames: Vec<String>,
    range: AnalyticsRange,
) -> Result<AnalyticsSummary, AppError> {
    let hosts = hostnames
        .iter()
        .map(|h| hostname(h))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(state
        .analytics
        .summary(&state.accounts, &account_id, &hosts, range)
        .await?)
}

/// One route's traffic in detail, from Cloudflare's edge.
#[tauri::command]
#[specta::specta]
pub async fn analytics_route(
    state: State<'_, AppState>,
    account_id: String,
    hostname: String,
    path: Option<String>,
    range: AnalyticsRange,
) -> Result<RouteStats, AppError> {
    let route = RouteRef {
        hostname: self::hostname(&hostname)?,
        path: path.and_then(|p| teitunnel_core::analytics::path_prefix(&p)),
    };
    Ok(state
        .analytics
        .route(&state.accounts, &account_id, &route, range)
        .await?)
}

/// Uptime of every route this Mac serves.
#[tauri::command]
#[specta::specta]
pub async fn uptime_list(state: State<'_, AppState>) -> Result<Vec<UptimeSummary>, AppError> {
    Ok(state.monitor.summaries(now_ms()).await?)
}

/// One route's uptime over `range` (null when this Mac doesn't serve it). `path` is the
/// route's path rule, as in the routes view.
#[tauri::command]
#[specta::specta]
pub async fn uptime_route(
    state: State<'_, AppState>,
    hostname: String,
    path: Option<String>,
    range: AnalyticsRange,
) -> Result<Option<UptimeDetail>, AppError> {
    let route = RouteRef {
        hostname: self::hostname(&hostname)?,
        path: path
            .and_then(|p| teitunnel_core::analytics::path_prefix(&p))
            .filter(|p| p != "/"),
    };
    Ok(state.monitor.detail(&route, range, now_ms()).await?)
}

/// The alert rules.
#[tauri::command]
#[specta::specta]
pub async fn alerts_get(state: State<'_, AppState>) -> Result<AlertRules, AppError> {
    Ok(alerts::load_rules(&state.store).await?)
}

/// Saves the alert rules (values out of range are brought into it) and returns them.
#[tauri::command]
#[specta::specta]
pub async fn alerts_set(
    app: AppHandle,
    state: State<'_, AppState>,
    rules: AlertRules,
) -> Result<AlertRules, AppError> {
    let saved = alerts::save_rules(&state.store, rules).await?;
    let _ = EntityChanged {
        kind: EntityKind::Settings,
        id: None,
    }
    .emit(&app);
    Ok(saved)
}
