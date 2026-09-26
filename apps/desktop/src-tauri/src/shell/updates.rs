//! App updates through Tauri's updater plugin. The policy (when to
//! check, what installs on quit) is `teitunnel_core::updates`; this adapts the plugin
//! to it and tells the webview when the status changes.
//!
//! An update is downloaded and verified in the background once found, then installed
//! when the user restarts, or when the app quits (not for `.deb`/`.rpm`, which would
//! ask for a password at quit).

use std::{
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_updater::{Update, UpdaterExt};
use tauri_specta::Event;
use teitunnel_core::{
    settings,
    text::msg::updates as m,
    updates::{self, Install, UpdateState, UpdateStatus},
};

use crate::{
    ipc::{EntityChanged, EntityKind},
    state::AppState,
};

/// Update state, managed by the app.
pub struct Updates {
    install: Install,
    /// A check (or download) is running.
    busy: AtomicBool,
    /// "Restart to Update" was chosen.
    restart_requested: AtomicBool,
    inner: Mutex<Inner>,
}

struct Inner {
    state: UpdateState,
    /// When the last check finished (for the UI) and when, monotonically (for the
    /// schedule).
    last_checked: Option<(SystemTime, Instant)>,
    /// The downloaded, verified update.
    ready: Option<(Update, Vec<u8>)>,
}

/// How this copy was installed.
fn install_kind() -> Install {
    if cfg!(any(debug_assertions, feature = "e2e")) {
        return Install::Development;
    }
    use tauri::utils::{config::BundleType, platform::bundle_type};
    match bundle_type() {
        Some(BundleType::App | BundleType::AppImage | BundleType::Nsis | BundleType::Msi) => {
            Install::InPlace
        }
        Some(BundleType::Deb | BundleType::Rpm) => Install::SystemPackage,
        _ => Install::Unknown,
    }
}

impl Updates {
    pub fn new() -> Self {
        Self {
            install: install_kind(),
            busy: AtomicBool::new(false),
            restart_requested: AtomicBool::new(false),
            inner: Mutex::new(Inner {
                state: UpdateState::Idle,
                last_checked: None,
                ready: None,
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set_state<R: Runtime>(&self, app: &AppHandle<R>, state: UpdateState) {
        self.lock().state = state;
        changed(app);
    }

    /// The status the app shows.
    pub async fn status<R: Runtime>(&self, app: &AppHandle<R>) -> UpdateStatus {
        let automatic = automatic(app).await;
        let inner = self.lock();
        UpdateStatus {
            current_version: app.package_info().version.to_string(),
            unsupported: self.install.unsupported(),
            automatic,
            last_checked: inner.last_checked.map(|(at, _)| unix_ms(at)),
            state: inner.state.clone(),
            install_on_quit: self.install.installs_on_quit(),
        }
    }

    /// Checks for an update and downloads it. `manual`: the user asked, so it runs even
    /// with automatic checks off. Does nothing while a check runs or when this copy
    /// can't update itself.
    pub async fn check<R: Runtime>(&self, app: &AppHandle<R>, manual: bool) {
        if self.install.unsupported().is_some() || (!manual && !automatic(app).await) {
            return;
        }
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let ready_version = self.lock().ready.as_ref().map(|(u, _)| u.version.clone());
        // An update that's already downloaded stays ready while the check runs.
        if ready_version.is_none() {
            self.set_state(app, UpdateState::Checking);
        }
        let state = match self.find(app).await {
            Ok(None) => UpdateState::UpToDate,
            Ok(Some(update)) if Some(&update.version) == ready_version.as_ref() => {
                self.lock().state.clone()
            }
            Ok(Some(update)) => self.download(app, update).await,
            Err(message) => UpdateState::Failed { message },
        };
        {
            let mut inner = self.lock();
            inner.state = state;
            inner.last_checked = Some((SystemTime::now(), Instant::now()));
        }
        self.busy.store(false, Ordering::SeqCst);
        changed(app);
    }

    async fn find<R: Runtime>(
        &self,
        app: &AppHandle<R>,
    ) -> Result<Option<Update>, teitunnel_core::text::Text> {
        let updater = app
            .updater()
            .map_err(|err| m::check_failed(err.to_string()))?;
        updater
            .check()
            .await
            .map_err(|err| m::check_failed(err.to_string()))
    }

    async fn download<R: Runtime>(&self, app: &AppHandle<R>, update: Update) -> UpdateState {
        let version = update.version.clone();
        self.set_state(
            app,
            UpdateState::Downloading {
                version: version.clone(),
                progress: None,
            },
        );
        let mut downloaded = 0u64;
        let mut shown = -1i64;
        let result = update
            .download(
                |chunk, total| {
                    downloaded += chunk as u64;
                    let progress = updates::progress(downloaded, total);
                    // Tell the UI in whole percents, not on every chunk.
                    #[allow(clippy::cast_possible_truncation)]
                    let percent = progress.map_or(-1, |p| (p * 100.0) as i64);
                    if percent != shown {
                        shown = percent;
                        self.set_state(
                            app,
                            UpdateState::Downloading {
                                version: version.clone(),
                                progress,
                            },
                        );
                    }
                },
                || {},
            )
            .await;
        match result {
            Ok(bytes) => {
                tracing::info!(version = %update.version, "update downloaded and verified");
                let state = UpdateState::Ready {
                    version: update.version.clone(),
                    notes: update.body.clone().filter(|n| !n.trim().is_empty()),
                };
                self.lock().ready = Some((update, bytes));
                state
            }
            Err(err) => {
                tracing::warn!(error = %err, "update download failed");
                UpdateState::Failed {
                    message: m::download_failed(err.to_string()),
                }
            }
        }
    }

    /// "Restart to Update": quits the usual way (stopping the app's connectors and
    /// shares), installs the update at the end of it, and starts the new version.
    pub fn restart<R: Runtime>(&self, app: &AppHandle<R>) {
        if self.lock().ready.is_none() {
            return;
        }
        self.restart_requested.store(true, Ordering::SeqCst);
        if let Some(state) = app.try_state::<AppState>() {
            state.quit_confirmed.store(true, Ordering::SeqCst);
        }
        app.exit(0);
    }

    /// The last step of quitting: installs a ready update if the user asked to restart,
    /// or if this kind of install takes one without asking. Returns whether to start
    /// the app again. On Windows the installer takes over from here (and restarts the
    /// app itself when asked to).
    pub fn finish_on_exit(&self) -> bool {
        let restart = self.restart_requested.load(Ordering::SeqCst);
        if !restart && !self.install.installs_on_quit() {
            return false;
        }
        let Some((update, bytes)) = self.lock().ready.take() else {
            return false;
        };
        match update.install(&bytes) {
            Ok(()) => {
                tracing::info!(version = %update.version, "update installed");
                restart
            }
            Err(err) => {
                tracing::error!(error = %err, "installing the update failed");
                // Relaunch the current version rather than leaving the user with nothing.
                restart
            }
        }
    }
}

async fn automatic<R: Runtime>(app: &AppHandle<R>) -> bool {
    match app.try_state::<AppState>() {
        Some(state) => settings::load(&state.store)
            .await
            .map_or(true, |s| s.check_for_updates),
        None => false,
    }
}

fn changed<R: Runtime>(app: &AppHandle<R>) {
    let _ = EntityChanged {
        kind: EntityKind::Updates,
        id: None,
    }
    .emit(app);
}

#[allow(clippy::cast_precision_loss)] // milliseconds since 1970 fit an f64 exactly
fn unix_ms(at: SystemTime) -> f64 {
    at.duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_millis() as f64)
}

/// Checks after launch and then daily while the app runs.
pub fn spawn_schedule<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(updates::FIRST_CHECK_DELAY).await;
        // Wakes hourly so a Mac that slept through the due time checks soon after.
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(60 * 60));
        loop {
            tick.tick().await;
            let Some(updates) = app.try_state::<Updates>() else {
                return;
            };
            let since = updates.lock().last_checked.map(|(_, at)| at.elapsed());
            if updates::check_due(automatic(&app).await, since) {
                updates.check(&app, false).await;
            }
        }
    });
}
