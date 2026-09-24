//! Quick Share: an anonymous `trycloudflare.com` URL for a local service, one
//! cloudflared process per share (ARCHITECTURE §5.3).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use cloudflared::{Endpoints, NEUTRAL_CONFIG, QuickTunnelCmd};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{
    binary::BinaryManager,
    dev_server::{self, DevServer},
    domain::{Hostname, OriginUrl, RouteOrigin},
    engine::{Edge, Failure, Verification},
    inspect::{Inspector, TapScope, TapSpec},
    runtime::{
        ConnectorId, ConnectorSpec, ConnectorState, PortAllocator, RuntimeEvent, Supervisor,
    },
    store::Store,
};

use crate::text::{Text, UserText, english_display, msg};

/// How long to wait for a URL and a live connection.
const URL_TIMEOUT: Duration = Duration::from_secs(30);
/// How long the check after going live retries a failure that may pass (the edge still
/// learning about a new connection).
const CHECK_PATIENCE: Duration = Duration::from_secs(10);
const CHECK_RETRY: Duration = Duration::from_secs(2);
/// New trycloudflare.com names take ~3–4 s to reach public DNS (measured, D-037), and
/// an early lookup is cached as NXDOMAIN for up to 30 minutes. So a share is only
/// shown as live (and openable) this long after its hostname first appears.
const DNS_PROPAGATION: Duration = Duration::from_secs(6);
const URL_POLL: Duration = Duration::from_millis(250);
const ID_PREFIX: &str = "qs-";

/// Errors from Quick Share operations.
#[derive(Debug, thiserror::Error)]
pub enum QuickShareError {
    /// cloudflared isn't available.
    #[error(transparent)]
    Binary(#[from] cloudflared::Error),
    /// No free metrics port.
    NoFreePort,
    /// Unknown share.
    NotFound,
    /// Not a Host header (`localhost:5173`, `app.local`).
    InvalidHostHeader,
    /// The neutral cloudflared config file couldn't be written.
    Config(std::io::Error),
    /// The supervisor refused.
    #[error(transparent)]
    Runtime(#[from] crate::runtime::SupervisorError),
    /// The inspector couldn't put a tap in front of the service.
    Inspector(Text),
}

impl UserText for QuickShareError {
    fn text(&self) -> Text {
        match self {
            Self::Inspector(text) => text.clone(),
            Self::Binary(err) => err.text(),
            Self::Runtime(err) => err.text(),
            Self::NoFreePort => msg::error::quick_share::no_free_port(),
            Self::NotFound => msg::error::quick_share::not_found(),
            Self::InvalidHostHeader => msg::error::quick_share::invalid_host_header(),
            Self::Config(err) => msg::error::quick_share::config(err),
        }
    }
}

english_display!(QuickShareError);

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
        message: Text,
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
    ///
    /// Exported to TypeScript as `number` (values stay far below 2^53). The `u32` is
    /// only a type hint for specta, which exports `u64` as bigint and `f64` as nullable.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub started_at: u64,
    /// When it stops by itself, milliseconds since the Unix epoch.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub stop_at: Option<u64>,
    /// The Host header sent to the service, if any.
    pub host_header: Option<HostHeader>,
    /// The check through Cloudflare once the share is live (`None` until then).
    pub check: Option<Verification>,
    /// Requests go through the inspector (its tap has the share's id).
    pub inspected: bool,
}

/// A Host header a share sends to its service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct HostHeader {
    /// The value, e.g. `localhost:5173`.
    pub value: String,
    /// Set by Teitunnel because the service is this dev server, which refuses unknown
    /// addresses (`None`: the user asked for it).
    pub auto_for: Option<DevServer>,
}

/// Which Host header a new share sends.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "mode",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum HostHeaderChoice {
    /// The service's own address when it's a dev server that needs it and where that's
    /// safe ([`dev_server::default_host_header`]); otherwise none.
    #[default]
    Auto,
    /// Leave the Host header as the visitor's browser sent it.
    Off,
    /// Send this value.
    Set {
        /// E.g. `localhost:5173`.
        value: String,
    },
}

impl HostHeaderChoice {
    /// The header to send for `origin`.
    ///
    /// # Errors
    /// [`QuickShareError::InvalidHostHeader`] for a value that isn't a host.
    pub async fn resolve(&self, origin: &str) -> Result<Option<HostHeader>, QuickShareError> {
        match self {
            Self::Off => Ok(None),
            Self::Set { value } => Ok(Some(HostHeader {
                value: dev_server::parse_host_header(value)
                    .ok_or(QuickShareError::InvalidHostHeader)?,
                auto_for: None,
            })),
            Self::Auto => {
                let Ok(origin) = RouteOrigin::parse(origin) else {
                    return Ok(None);
                };
                Ok(dev_server::default_host_header(&origin)
                    .await
                    .map(|(value, server)| HostHeader {
                        value,
                        auto_for: Some(server),
                    }))
            }
        }
    }
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
    /// The inspector's tap cloudflared sends requests to, when inspected.
    tap: Option<lens::TapId>,
    /// Bumped when the share restarts with other settings, so work for the previous
    /// cloudflared (waiting for its URL, checking it) stops touching the share.
    generation: u64,
}

/// Starts, tracks and stops Quick Shares. Cheap to clone.
#[derive(Clone)]
pub struct QuickShares {
    supervisor: Supervisor,
    binary: BinaryManager,
    ports: PortAllocator,
    store: Store,
    /// The neutral cloudflared config every share runs with (see [`QuickTunnelCmd`]).
    config: PathBuf,
    edge: Edge,
    shares: Arc<Mutex<HashMap<String, Entry>>>,
    changes: broadcast::Sender<String>,
    url_timeout: Duration,
    dns_propagation: Duration,
    /// This process's inspector; without one, shares go straight to their service.
    inspector: Option<Inspector>,
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
    /// `config` is a file Teitunnel owns (in its data folder, not a shared temporary
    /// one) that it writes the neutral cloudflared config to.
    pub fn new(
        supervisor: Supervisor,
        binary: BinaryManager,
        ports: PortAllocator,
        store: Store,
        config: PathBuf,
    ) -> Self {
        let (changes, _) = broadcast::channel(256);
        Self {
            supervisor,
            binary,
            ports,
            store,
            config,
            edge: Edge::Cloudflare,
            shares: Arc::default(),
            changes,
            url_timeout: URL_TIMEOUT,
            dns_propagation: DNS_PROPAGATION,
            inspector: None,
        }
    }

    /// Sends new shares through `inspector` (as its settings say, unless a share asks
    /// otherwise). Run [`QuickShares::watch_idle`] too, for idle stops.
    #[must_use]
    pub fn with_inspector(mut self, inspector: Inspector) -> Self {
        self.inspector = Some(inspector);
        self
    }

    /// The inspector shares go through, if any.
    pub fn inspector(&self) -> Option<&Inspector> {
        self.inspector.as_ref()
    }

    /// Where the check after going live connects (a test server standing in for
    /// Cloudflare's edge in tests).
    #[must_use]
    pub fn with_edge(mut self, edge: Edge) -> Self {
        self.edge = edge;
        self
    }

    /// Overrides the DNS propagation wait (tests with fake hostnames).
    #[must_use]
    pub fn with_dns_propagation(mut self, wait: Duration) -> Self {
        self.dns_propagation = wait;
        self
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

    /// The command for one share's cloudflared, after (re)writing the neutral config it
    /// points at. `url` is the service, or the inspector's tap in front of it (which then
    /// sends the Host header itself).
    async fn command(
        &self,
        binary: &Path,
        url: &str,
        port: u16,
        host_header: Option<&HostHeader>,
    ) -> Result<cloudflared::CommandSpec, QuickShareError> {
        if let Some(dir) = self.config.parent() {
            tokio::fs::create_dir_all(dir)
                .await
                .map_err(QuickShareError::Config)?;
        }
        tokio::fs::write(&self.config, NEUTRAL_CONFIG)
            .await
            .map_err(QuickShareError::Config)?;
        Ok(QuickTunnelCmd {
            origin: url.to_owned(),
            metrics_port: port,
            config: self.config.clone(),
            host_header: host_header.map(|h| h.value.clone()),
        }
        .build(binary))
    }

    /// Starts a tap in front of `share`'s service; returns its id and address.
    async fn start_tap(
        &self,
        inspector: &Inspector,
        share: &QuickShare,
    ) -> Result<(lens::TapId, String), QuickShareError> {
        let mut spec = TapSpec::new(
            TapScope::QuickShare {
                share_id: share.id.clone(),
            },
            share.url.as_deref().unwrap_or(share.origin.as_str()),
            share.origin.as_str(),
        );
        spec.host_header = share.host_header.as_ref().map(|h| h.value.clone());
        spec.public_url.clone_from(&share.url);
        let tap = inspector
            .start(spec)
            .await
            .map_err(|e| QuickShareError::Inspector(e.text()))?;
        Ok((tap.id, tap.address))
    }

    /// Where cloudflared sends requests for `share` (a new tap when inspected), and the
    /// Host header cloudflared itself sets (none when the tap sets it).
    async fn target(
        &self,
        share: &QuickShare,
        inspect: bool,
    ) -> Result<(String, Option<HostHeader>, Option<lens::TapId>), QuickShareError> {
        match (&self.inspector, inspect) {
            (Some(inspector), true) => {
                let (tap, url) = self.start_tap(inspector, share).await?;
                Ok((url, None, Some(tap)))
            }
            _ => Ok((share.origin.to_string(), share.host_header.clone(), None)),
        }
    }

    /// Starts sharing `origin`. Returns at once; the URL arrives via [`Self::subscribe`].
    /// It goes through the inspector when there is one and its settings say so.
    ///
    /// # Errors
    /// Fails if cloudflared isn't installed, no port is free, or the Host header isn't
    /// one.
    pub async fn start(
        &self,
        origin: OriginUrl,
        stop_after: Option<Duration>,
        host_header: &HostHeaderChoice,
    ) -> Result<QuickShare, QuickShareError> {
        self.start_with(origin, stop_after, host_header, None).await
    }

    /// Like [`Self::start`], choosing whether the share is inspected (`None`: the
    /// inspector's setting).
    ///
    /// # Errors
    /// As [`Self::start`], or the inspector couldn't start a tap.
    pub async fn start_with(
        &self,
        origin: OriginUrl,
        stop_after: Option<Duration>,
        host_header: &HostHeaderChoice,
        inspect: Option<bool>,
    ) -> Result<QuickShare, QuickShareError> {
        let host_header = host_header.resolve(origin.as_str()).await?;
        let binary = self.binary.current().await?;
        let port = self.ports.allocate().ok_or(QuickShareError::NoFreePort)?;
        let id = format!("{ID_PREFIX}{}", Uuid::new_v4().simple());
        let started_at = now_ms();
        let inspect = self.inspector.as_ref().is_some_and(|inspector| {
            inspect.unwrap_or_else(|| inspector.settings().inspect_quick_shares)
        });
        let mut share = QuickShare {
            id: id.clone(),
            origin: origin.clone(),
            url: None,
            status: ShareStatus::Starting,
            started_at,
            stop_at: stop_after
                .map(|d| started_at + u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
            host_header,
            check: None,
            inspected: inspect,
        };
        let (url, cloudflared_host, tap) = match self.target(&share, inspect).await {
            Ok(target) => target,
            Err(err) => {
                self.ports.release(port);
                return Err(err);
            }
        };
        let started = match self
            .command(&binary.path, &url, port, cloudflared_host.as_ref())
            .await
        {
            Ok(command) => self
                .supervisor
                .start(ConnectorSpec::new(ConnectorId(id.clone()), command, port))
                .map_err(QuickShareError::from),
            Err(err) => Err(err),
        };
        if let Err(err) = started {
            self.ports.release(port);
            self.stop_tap(tap.as_ref()).await;
            return Err(err);
        }
        share.inspected = tap.is_some();
        self.lock().insert(
            id.clone(),
            Entry {
                share: share.clone(),
                port,
                tap,
                generation: 0,
            },
        );
        self.record_start(&share);
        self.changed(&id);

        tokio::spawn(self.clone().await_url(id.clone(), port, 0));
        if let Some(delay) = stop_after {
            let this = self.clone();
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                let _ = this.stop(&id).await;
            });
        }
        Ok(share)
    }

    async fn stop_tap(&self, tap: Option<&lens::TapId>) {
        if let (Some(inspector), Some(tap)) = (&self.inspector, tap) {
            inspector.stop(tap).await;
        }
    }

    /// Changes the Host header a share sends to its service (or none), the fix for a dev
    /// server that refuses the public address. An inspected share changes at once and
    /// keeps its address; otherwise cloudflared restarts with the header, and a Quick
    /// Share's address comes with its cloudflared, so the share gets a new URL. Either
    /// way it's checked again once live.
    ///
    /// # Errors
    /// [`QuickShareError::NotFound`], an invalid header, or cloudflared failing to start.
    pub async fn set_host_header(
        &self,
        id: &str,
        host_header: Option<&str>,
    ) -> Result<QuickShare, QuickShareError> {
        let host_header = host_header
            .map(|value| {
                dev_server::parse_host_header(value)
                    .map(|value| HostHeader {
                        value,
                        auto_for: None,
                    })
                    .ok_or(QuickShareError::InvalidHostHeader)
            })
            .transpose()?;
        let live_tap = {
            let shares = self.lock();
            let entry = shares.get(id).ok_or(QuickShareError::NotFound)?;
            entry.tap.clone()
        };
        if let (Some(inspector), Some(tap)) = (&self.inspector, live_tap) {
            inspector
                .set_host_header(&tap, host_header.as_ref().map(|h| h.value.clone()))
                .map_err(|e| QuickShareError::Inspector(e.text()))?;
            let generation = self.generation(id).ok_or(QuickShareError::NotFound)?;
            self.update(id, |share| {
                share.host_header = host_header;
                share.check = None;
            });
            let share = self
                .lock()
                .get(id)
                .map(|entry| entry.share.clone())
                .ok_or(QuickShareError::NotFound)?;
            let this = self.clone();
            let id = id.to_owned();
            tokio::spawn(async move { this.check(&id, generation, CHECK_PATIENCE).await });
            return Ok(share);
        }
        self.restart(id, |share| share.host_header = host_header, None)
            .await
    }

    /// Turns inspection of a running share on or off. cloudflared restarts pointing at
    /// the inspector or straight at the service, so the share gets a new URL.
    ///
    /// # Errors
    /// [`QuickShareError::NotFound`], no inspector in this process (to turn it on), or
    /// cloudflared failing to start.
    pub async fn set_inspected(&self, id: &str, on: bool) -> Result<QuickShare, QuickShareError> {
        let current = self
            .lock()
            .get(id)
            .map(|entry| entry.share.clone())
            .ok_or(QuickShareError::NotFound)?;
        if current.inspected == on {
            return Ok(current);
        }
        if on && self.inspector.is_none() {
            return Err(QuickShareError::Inspector(
                crate::text::msg::error::inspect::unknown_tap(),
            ));
        }
        self.restart(id, |_| {}, Some(on)).await
    }

    /// Restarts a share's cloudflared after `change`, inspected or not (`None`: as it
    /// was), with a new URL.
    async fn restart(
        &self,
        id: &str,
        change: impl FnOnce(&mut QuickShare),
        inspect: Option<bool>,
    ) -> Result<QuickShare, QuickShareError> {
        let binary = self.binary.current().await?;
        let (share, port, generation, old_tap) = {
            let mut shares = self.lock();
            let entry = shares.get_mut(id).ok_or(QuickShareError::NotFound)?;
            entry.generation += 1;
            let share = &mut entry.share;
            change(share);
            share.url = None;
            share.check = None;
            share.status = ShareStatus::Starting;
            (
                share.clone(),
                entry.port,
                entry.generation,
                entry.tap.take(),
            )
        };
        self.changed(id);
        let connector = ConnectorId(id.to_owned());
        let _ = self.supervisor.stop(&connector).await;
        self.stop_tap(old_tap.as_ref()).await;
        let inspect = inspect.unwrap_or(share.inspected);
        let started = async {
            let (url, cloudflared_host, tap) = self.target(&share, inspect).await?;
            let command = self
                .command(&binary.path, &url, port, cloudflared_host.as_ref())
                .await;
            let command = match command {
                Ok(command) => command,
                Err(err) => {
                    self.stop_tap(tap.as_ref()).await;
                    return Err(err);
                }
            };
            if let Err(err) = self
                .supervisor
                .start(ConnectorSpec::new(connector, command, port))
            {
                self.stop_tap(tap.as_ref()).await;
                return Err(QuickShareError::from(err));
            }
            Ok(tap)
        }
        .await;
        let tap = match started {
            Ok(tap) => tap,
            Err(err) => {
                self.update(id, |share| {
                    share.status = ShareStatus::Failed {
                        message: err.text(),
                    };
                });
                return Err(err);
            }
        };
        let inspected = tap.is_some();
        if let Some(entry) = self.lock().get_mut(id) {
            entry.tap = tap;
            entry.share.inspected = inspected;
        }
        self.changed(id);
        tokio::spawn(self.clone().await_url(id.to_owned(), port, generation));
        Ok(QuickShare { inspected, ..share })
    }

    /// Stops shares whose inspector tap has been idle for its limit (the setting, or the
    /// share's own). Run once for the app's lifetime.
    pub async fn watch_idle(self) {
        let Some(inspector) = self.inspector.clone() else {
            return;
        };
        let mut events = inspector.subscribe();
        loop {
            match events.recv().await {
                Ok(crate::inspect::InspectEvent::Idle {
                    scope: TapScope::QuickShare { share_id },
                    ..
                }) => {
                    let _ = self.stop(&share_id).await;
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            }
        }
    }

    /// Checks a live share through Cloudflare again (after the user fixed something)
    /// and returns the result, which is also on the share.
    ///
    /// # Errors
    /// [`QuickShareError::NotFound`] if it isn't running or has no URL yet.
    pub async fn recheck(&self, id: &str) -> Result<Verification, QuickShareError> {
        let generation = self.generation(id).ok_or(QuickShareError::NotFound)?;
        self.check(id, generation, Duration::ZERO)
            .await
            .ok_or(QuickShareError::NotFound)
    }

    fn generation(&self, id: &str) -> Option<u64> {
        self.lock().get(id).map(|entry| entry.generation)
    }

    /// Fetches the share once through Cloudflare's edge, as a visitor would, retrying
    /// what may pass for up to `patience`, and records the result on the share (unless
    /// it restarted meanwhile). Also asks the service itself whether it streams events,
    /// which a Quick Share can't carry.
    async fn check(&self, id: &str, generation: u64, patience: Duration) -> Option<Verification> {
        let share = self.lock().get(id).map(|entry| entry.share.clone())?;
        let url = share.url.as_deref()?;
        let hostname = Hostname::parse(url.trim_start_matches("https://")).ok()?;
        let origin = RouteOrigin::parse(share.origin.as_str()).ok();
        let deadline = tokio::time::Instant::now() + patience;
        let mut result = loop {
            let result = crate::engine::probe(self.edge, &hostname, origin.as_ref()).await;
            let transient = result.failure.as_ref().is_some_and(Failure::is_transient);
            if !transient || tokio::time::Instant::now() + CHECK_RETRY > deadline {
                break result;
            }
            tokio::time::sleep(CHECK_RETRY).await;
        };
        if share.host_header.is_some()
            && let Some(Failure::HostRejected { rejection }) = &mut result.failure
        {
            rejection.host_header = None;
        }
        result.event_stream =
            result.event_stream || dev_server::serves_event_stream(share.origin.as_str()).await;
        let current = self.generation(id) == Some(generation);
        if current {
            let recorded = result.clone();
            self.update(id, |share| share.check = Some(recorded));
        }
        current.then_some(result)
    }

    /// Stops a share and forgets it.
    ///
    /// # Errors
    /// [`QuickShareError::NotFound`] if it isn't running.
    pub async fn stop(&self, id: &str) -> Result<(), QuickShareError> {
        let entry = self.lock().remove(id).ok_or(QuickShareError::NotFound)?;
        let _ = self.supervisor.stop(&ConnectorId(id.to_owned())).await;
        self.stop_tap(entry.tap.as_ref()).await;
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

    /// Polls until the share has a URL and a live connection (D-034), then allows for DNS
    /// propagation before declaring it live (D-037). It never queries DNS itself: an
    /// early query would get the NXDOMAIN cached. Once live, the share is checked
    /// through Cloudflare's edge.
    async fn await_url(self, id: String, port: u16, generation: u64) {
        let Ok(endpoints) = Endpoints::new(port) else {
            return;
        };
        let deadline = tokio::time::Instant::now() + self.url_timeout;
        let mut host_seen: Option<tokio::time::Instant> = None;
        while tokio::time::Instant::now() < deadline {
            if self.generation(&id) != Some(generation) {
                return;
            }
            if let Ok(Some(host)) = endpoints.quick_tunnel_host().await {
                let url = format!("https://{host}");
                let connected = endpoints
                    .ready()
                    .await
                    .is_ok_and(|r| r.ready_connections > 0);
                let seen = *host_seen.get_or_insert_with(tokio::time::Instant::now);
                if connected && seen.elapsed() >= self.dns_propagation {
                    self.update(&id, |share| {
                        share.url = Some(url.clone());
                        share.status = ShareStatus::Live;
                    });
                    self.record_url(&id, &url);
                    let tap = self.lock().get(&id).and_then(|entry| entry.tap.clone());
                    if let (Some(inspector), Some(tap)) = (&self.inspector, tap) {
                        inspector.set_public_url(&tap, Some(url.clone()));
                    }
                    self.check(&id, generation, CHECK_PATIENCE).await;
                    return;
                }
                self.update(&id, |share| share.url = Some(url));
            }
            tokio::time::sleep(URL_POLL).await;
        }
        if self.generation(&id) != Some(generation) {
            return;
        }
        self.update(&id, |share| {
            share.status = ShareStatus::Failed {
                message: msg::quick_share::no_url(),
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
                message: msg::quick_share::crash_loop(),
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

/// Renders `url` as a QR code of Unicode half blocks for a terminal. Light modules are
/// drawn as blocks, as `qrencode -t utf8` does, so it scans on dark backgrounds (phone
/// cameras also read the inverted code on light ones).
pub fn qr_terminal(url: &str) -> Option<String> {
    use qrcode::{EcLevel, QrCode, render::unicode::Dense1x2};
    let code = QrCode::with_error_correction_level(url, EcLevel::L).ok()?;
    Some(
        code.render::<Dense1x2>()
            .dark_color(Dense1x2::Light)
            .light_color(Dense1x2::Dark)
            .quiet_zone(true)
            .build(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_qr_for_a_terminal() {
        let qr = qr_terminal("https://quiet-river-lamp-orbit.trycloudflare.com").unwrap();
        let lines: Vec<&str> = qr.lines().collect();
        assert!(lines.len() > 10);
        assert!(
            lines
                .iter()
                .all(|l| l.chars().count() == lines[0].chars().count()),
            "square"
        );
        assert!(
            lines[0].chars().all(|c| c == '\u{2588}'),
            "quiet zone is light"
        );
    }

    #[test]
    fn renders_qr_svg() {
        let svg = qr_svg("https://quiet-river-lamp-orbit.trycloudflare.com").unwrap();
        assert!(svg.starts_with("<?xml") || svg.starts_with("<svg"));
        assert!(svg.contains("currentColor"));
    }
}
