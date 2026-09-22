//! Quick Share: an anonymous `trycloudflare.com` URL for a local service, one
//! cloudflared process per share (ARCHITECTURE §5.3).

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cloudflared::{Endpoints, QuickTunnelCmd};
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    binary::BinaryManager,
    domain::OriginUrl,
    runtime::{
        ConnectorId, ConnectorSpec, ConnectorState, PortAllocator, RuntimeEvent, Supervisor,
    },
    store::Store,
};

/// How long to wait for cloudflared to get a URL and a live connection.
const URL_TIMEOUT: Duration = Duration::from_secs(20);
const URL_POLL: Duration = Duration::from_millis(250);
const ID_PREFIX: &str = "qs-";

/// Errors from Quick Share operations.
#[derive(Debug, thiserror::Error)]
pub enum QuickShareError {
    /// cloudflared isn't available.
    #[error(transparent)]
    Binary(#[from] cloudflared::Error),
    /// No free metrics port.
    #[error("Too many Quick Shares are running.")]
    NoFreePort,
    /// Unknown share.
    #[error("That Quick Share isn't running.")]
    NotFound,
    /// The supervisor refused.
    #[error(transparent)]
    Runtime(#[from] crate::runtime::SupervisorError),
}

/// Where a share is in its life.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase", tag = "status")]
pub enum ShareStatus {
    /// cloudflared is starting and asking for a URL.
    Starting,
    /// The URL works.
    Live,
    /// Connection lost; cloudflared is reconnecting or restarting.
    Reconnecting,
    /// It failed and won't recover by itself.
    Failed {
        /// What went wrong, for the user.
        message: String,
    },
}

/// A running Quick Share, as the UI sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct QuickShare {
    /// Identifier.
    pub id: String,
    /// The local service being shared.
    pub origin: OriginUrl,
    /// The public URL, once cloudflared has one.
    pub url: Option<String>,
    /// Current status.
    pub status: ShareStatus,
    /// Start time, milliseconds since the Unix epoch.
    pub started_at: u64,
    /// When it stops by itself, milliseconds since the Unix epoch.
    pub stop_at: Option<u64>,
}

/// Live traffic numbers for a share.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ShareStats {
    /// Requests served.
    pub requests: u32,
    /// Requests that failed.
    pub errors: u32,
}

struct Entry {
    share: QuickShare,
    port: u16,
}

/// Starts, tracks and stops Quick Shares. Cheap to clone.
#[derive(Clone)]
pub struct QuickShares {
    supervisor: Supervisor,
    binary: BinaryManager,
    ports: PortAllocator,
    store: Store,
    shares: Arc<Mutex<HashMap<String, Entry>>>,
    changes: broadcast::Sender<String>,
    url_timeout: Duration,
}

impl std::fmt::Debug for QuickShares {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("QuickShares").finish_non_exhaustive()
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

impl QuickShares {
    /// A Quick Share service. Call [`QuickShares::watch_runtime`] once afterwards.
    pub fn new(
        supervisor: Supervisor,
        binary: BinaryManager,
        ports: PortAllocator,
        store: Store,
    ) -> Self {
        let (changes, _) = broadcast::channel(256);
        Self {
            supervisor,
            binary,
            ports,
            store,
            shares: Arc::default(),
            changes,
            url_timeout: URL_TIMEOUT,
        }
    }

    /// Overrides how long to wait for a URL (tests).
    #[must_use]
    pub fn with_url_timeout(mut self, timeout: Duration) -> Self {
        self.url_timeout = timeout;
        self
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Entry>> {
        self.shares
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Ids of shares that changed (status, URL, added or removed).
    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.changes.subscribe()
    }

    fn changed(&self, id: &str) {
        let _ = self.changes.send(id.to_owned());
    }

    fn update(&self, id: &str, f: impl FnOnce(&mut QuickShare)) {
        let updated = self
            .lock()
            .get_mut(id)
            .map(|entry| f(&mut entry.share))
            .is_some();
        if updated {
            self.changed(id);
        }
    }

    /// All running shares, newest first.
    pub fn list(&self) -> Vec<QuickShare> {
        let mut shares: Vec<_> = self.lock().values().map(|e| e.share.clone()).collect();
        shares.sort_by_key(|share| std::cmp::Reverse(share.started_at));
        shares
    }

    /// Starts sharing `origin`. Returns at once; the URL arrives via [`Self::subscribe`].
    ///
    /// # Errors
    /// Fails if cloudflared isn't installed or no port is free.
    pub async fn start(
        &self,
        origin: OriginUrl,
        stop_after: Option<Duration>,
    ) -> Result<QuickShare, QuickShareError> {
        let binary = self.binary.current().await?;
        let port = self.ports.allocate().ok_or(QuickShareError::NoFreePort)?;
        let id = format!("{ID_PREFIX}{}", Uuid::new_v4().simple());
        let started_at = now_ms();
        let share = QuickShare {
            id: id.clone(),
            origin: origin.clone(),
            url: None,
            status: ShareStatus::Starting,
            started_at,
            stop_at: stop_after
                .map(|d| started_at + u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        };
        let command = QuickTunnelCmd {
            origin: origin.to_string(),
            metrics_port: port,
        }
        .build(&binary.path);
        if let Err(err) =
            self.supervisor
                .start(ConnectorSpec::new(ConnectorId(id.clone()), command, port))
        {
            self.ports.release(port);
            return Err(err.into());
        }
        self.lock().insert(
            id.clone(),
            Entry {
                share: share.clone(),
                port,
            },
        );
        self.record_start(&share);
        self.changed(&id);

        tokio::spawn(self.clone().await_url(id.clone(), port));
        if let Some(delay) = stop_after {
            let this = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = this.stop(&id).await;
            });
        }
        Ok(share)
    }

    /// Stops a share and forgets it.
    ///
    /// # Errors
    /// [`QuickShareError::NotFound`] if it isn't running.
    pub async fn stop(&self, id: &str) -> Result<(), QuickShareError> {
        let entry = self.lock().remove(id).ok_or(QuickShareError::NotFound)?;
        let _ = self.supervisor.stop(&ConnectorId(id.to_owned())).await;
        self.ports.release(entry.port);
        self.record_stop(id);
        self.changed(id);
        Ok(())
    }

    /// Stops every share concurrently (app exit), so the total time is bounded by the
    /// slowest share rather than the sum.
    pub async fn stop_all(&self) {
        let ids: Vec<String> = self.lock().keys().cloned().collect();
        let mut stops = tokio::task::JoinSet::new();
        for id in ids {
            let this = self.clone();
            stops.spawn(async move { this.stop(&id).await });
        }
        while stops.join_next().await.is_some() {}
    }

    /// Traffic numbers, scraped on demand (the UI polls while the share is visible).
    ///
    /// # Errors
    /// [`QuickShareError::NotFound`] if the share isn't running.
    pub async fn stats(&self, id: &str) -> Result<ShareStats, QuickShareError> {
        let port = self
            .lock()
            .get(id)
            .map(|e| e.port)
            .ok_or(QuickShareError::NotFound)?;
        let metrics = Endpoints::new(port)?.metrics().await.unwrap_or_default();
        let clamp = |n: u64| u32::try_from(n).unwrap_or(u32::MAX);
        Ok(ShareStats {
            requests: clamp(metrics.total_requests()),
            errors: clamp(metrics.request_errors()),
        })
    }

    /// Polls cloudflared until the share has a URL and a live connection (D-034).
    async fn await_url(self, id: String, port: u16) {
        let Ok(endpoints) = Endpoints::new(port) else {
            return;
        };
        let deadline = tokio::time::Instant::now() + self.url_timeout;
        while tokio::time::Instant::now() < deadline {
            if !self.lock().contains_key(&id) {
                return;
            }
            if let Ok(Some(host)) = endpoints.quick_tunnel_host().await {
                let url = format!("https://{host}");
                if endpoints
                    .ready()
                    .await
                    .is_ok_and(|r| r.ready_connections > 0)
                {
                    self.update(&id, |share| {
                        share.url = Some(url.clone());
                        share.status = ShareStatus::Live;
                    });
                    self.record_url(&id, &url);
                    return;
                }
                self.update(&id, |share| share.url = Some(url));
            }
            tokio::time::sleep(URL_POLL).await;
        }
        self.update(&id, |share| {
            share.status = ShareStatus::Failed {
                message: "Cloudflare didn't provide a URL in time. Check your internet connection and try again.".into(),
            };
        });
    }

    /// Mirrors connector state into share status. Run once for the app's lifetime.
    pub async fn watch_runtime(self) {
        let mut events = self.supervisor.subscribe();
        loop {
            match events.recv().await {
                Ok(RuntimeEvent::State { id, state }) if id.0.starts_with(ID_PREFIX) => {
                    self.apply_state(&id.0, &state);
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    }

    fn apply_state(&self, id: &str, state: &ConnectorState) {
        let current = self.lock().get(id).map(|entry| entry.share.clone());
        let Some(share) = current else { return };
        let next = match state {
            ConnectorState::Degraded | ConnectorState::Crashed { .. } => ShareStatus::Reconnecting,
            ConnectorState::CrashLoop { .. } => ShareStatus::Failed {
                message: "cloudflared keeps exiting. See the log for details.".into(),
            },
            // First connection: `await_url` decides when the URL is live.
            ConnectorState::Healthy { .. }
                if share.status == ShareStatus::Reconnecting && share.url.is_some() =>
            {
                ShareStatus::Live
            }
            _ => return,
        };
        if next != share.status {
            self.update(id, |share| share.status = next);
        }
    }

    fn record_start(&self, share: &QuickShare) {
        let (id, origin, started_at) =
            (share.id.clone(), share.origin.to_string(), share.started_at);
        self.persist(move |conn| {
            conn.execute(
                "INSERT INTO quick_shares (id, origin, started_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, origin, i64::try_from(started_at).unwrap_or(i64::MAX)],
            )
        });
    }

    fn record_url(&self, id: &str, url: &str) {
        let (id, url) = (id.to_owned(), url.to_owned());
        self.persist(move |conn| {
            conn.execute(
                "UPDATE quick_shares SET url = ?2 WHERE id = ?1",
                rusqlite::params![id, url],
            )
        });
    }

    fn record_stop(&self, id: &str) {
        let id = id.to_owned();
        let stopped = i64::try_from(now_ms()).unwrap_or(i64::MAX);
        self.persist(move |conn| {
            conn.execute(
                "UPDATE quick_shares SET stopped_at = ?2 WHERE id = ?1",
                rusqlite::params![id, stopped],
            )
        });
    }

    /// History is best effort: a database hiccup must never break a running share.
    fn persist(
        &self,
        f: impl FnOnce(&rusqlite::Connection) -> rusqlite::Result<usize> + Send + 'static,
    ) {
        let store = self.store.clone();
        tokio::spawn(async move {
            if let Err(err) = store.call(move |conn| Ok(f(conn)?)).await {
                tracing::warn!(error = %err, "couldn't record Quick Share history");
            }
        });
    }
}

/// Renders `url` as an SVG QR code (dark modules use `currentColor`, so it follows the
/// theme).
pub fn qr_svg(url: &str) -> Option<String> {
    use qrcode::{EcLevel, QrCode, render::svg};
    let code = QrCode::with_error_correction_level(url, EcLevel::M).ok()?;
    Some(
        code.render::<svg::Color<'_>>()
            .min_dimensions(200, 200)
            .quiet_zone(true)
            .dark_color(svg::Color("currentColor"))
            .light_color(svg::Color("transparent"))
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_qr_svg() {
        let svg = qr_svg("https://quiet-river-lamp-orbit.trycloudflare.com").unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
        assert!(svg.contains("currentColor"));
    }
}
