//! Quick Share, local services and binary status commands.

use std::time::Duration;

use serde::Serialize;
use specta::Type;
use tauri::{State, ipc::Channel};
use teitunnel_core::{
    binary::{BinaryStatus, InstallStep as Progress},
    discovery::{self, LocalService},
    domain::OriginUrl,
    quick_share::{QuickShare, ShareStats, qr_svg},
    runtime::ConnectorId,
};

use crate::{error::AppError, state::AppState};

/// Where cloudflared comes from and whether it's new enough.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BinaryInfo {
    /// Absolute path.
    pub path: String,
    /// `managed`, `system` or `override`.
    pub source: String,
    /// Version, if readable.
    pub version: Option<String>,
    /// Whether it supports everything Teitunnel needs.
    pub supported: bool,
}

/// One cloudflared log line, for the log drawer.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    /// Timestamp as printed by cloudflared.
    pub time: Option<String>,
    /// `debug`, `info`, `warn`, `error`, `fatal` or `raw`.
    pub level: String,
    /// The message.
    pub message: String,
    /// The `error` field, if any.
    pub error: Option<String>,
}

fn binary_info(status: &BinaryStatus) -> BinaryInfo {
    BinaryInfo {
        path: status.path.display().to_string(),
        source: serde_json::to_value(status.source)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default(),
        version: status.version.map(|v| v.to_string()),
        supported: status.is_supported(),
    }
}

/// The cloudflared binary in use, or `null` if none is installed.
#[tauri::command]
#[specta::specta]
pub async fn binary_status(state: State<'_, AppState>) -> Result<Option<BinaryInfo>, AppError> {
    Ok(state.binary.refresh().await.ok().as_ref().map(binary_info))
}

/// Install progress, streamed to the webview.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase", tag = "step")]
pub enum InstallProgress {
    /// Downloading: bytes received of total.
    Downloading {
        /// Bytes received.
        received: u32,
        /// Total bytes.
        total: u32,
    },
    /// Checking checksums and the code signature.
    Verifying,
    /// Moving the binary into place.
    Installing,
}

/// Downloads, verifies and installs the latest cloudflared into the app data folder.
#[tauri::command]
#[specta::specta]
pub async fn binary_install(
    state: State<'_, AppState>,
    on_progress: Channel<InstallProgress>,
) -> Result<BinaryInfo, AppError> {
    let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
    let status = state
        .binary
        .install_latest(|progress| {
            let event = match progress {
                Progress::Downloading { received, total } => InstallProgress::Downloading {
                    received: clamp(received),
                    total: clamp(total),
                },
                Progress::Verifying => InstallProgress::Verifying,
                Progress::Installing => InstallProgress::Installing,
            };
            let _ = on_progress.send(event);
        })
        .await
        .map_err(teitunnel_core::Error::from)?;
    Ok(binary_info(&status))
}

/// Services listening on this Mac, likely dev servers first.
#[tauri::command]
#[specta::specta]
pub async fn services_list() -> Result<Vec<LocalService>, AppError> {
    tauri::async_runtime::spawn_blocking(discovery::list_services)
        .await
        .map_err(|err| AppError::internal(format!("Couldn't list local services: {err}")))
}

/// Starts sharing `origin`. The URL arrives via `EntityChanged` for `quickShares`.
#[tauri::command]
#[specta::specta]
pub async fn quick_share_start(
    state: State<'_, AppState>,
    origin: String,
    stop_after_minutes: Option<u32>,
) -> Result<QuickShare, AppError> {
    let origin = OriginUrl::parse(&origin)?;
    let stop_after = stop_after_minutes.map(|m| Duration::from_secs(u64::from(m) * 60));
    Ok(state.quick_shares.start(origin, stop_after).await?)
}

/// Stops a share.
#[tauri::command]
#[specta::specta]
pub async fn quick_share_stop(state: State<'_, AppState>, id: String) -> Result<(), AppError> {
    Ok(state.quick_shares.stop(&id).await?)
}

/// Running shares, newest first.
#[tauri::command]
#[specta::specta]
pub fn quick_share_list(state: State<'_, AppState>) -> Vec<QuickShare> {
    state.quick_shares.list()
}

/// Request counts for a share (polled by the UI while visible).
#[tauri::command]
#[specta::specta]
pub async fn quick_share_stats(
    state: State<'_, AppState>,
    id: String,
) -> Result<ShareStats, AppError> {
    Ok(state.quick_shares.stats(&id).await?)
}

/// The newest cloudflared log lines of a share.
#[tauri::command]
#[specta::specta]
pub fn quick_share_logs(state: State<'_, AppState>, id: String, limit: u32) -> Vec<LogLine> {
    state
        .supervisor
        .logs(
            &ConnectorId(id),
            usize::try_from(limit).unwrap_or(usize::MAX),
        )
        .unwrap_or_default()
        .iter()
        .map(|event| LogLine {
            time: event.time.clone(),
            level: serde_json::to_value(event.level)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default(),
            message: event.message.clone(),
            error: event.error.clone(),
        })
        .collect()
}

/// An SVG QR code for `url` (dark modules use `currentColor`).
#[tauri::command]
#[specta::specta]
pub fn quick_share_qr(url: String) -> Result<String, AppError> {
    qr_svg(&url).ok_or_else(|| AppError::invalid("url", "That URL is too long for a QR code."))
}
