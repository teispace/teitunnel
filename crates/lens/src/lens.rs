//! The Lens runtime: taps, listeners, captures and live events.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex, PoisonError, RwLock,
        atomic::{AtomicU64, Ordering::Relaxed},
    },
    time::Duration,
};

use tokio::sync::{Semaphore, broadcast};
use tokio_util::{sync::CancellationToken, task::TaskTracker};

use crate::{
    CaptureStore, Exchange, ExchangeId, Filter, LensError, LensEvent, ListenOptions, ListenerId,
    ListenerInfo, MemoryStore, MetricsSnapshot, Page, Query, Redaction, Routing, TapConfig, TapId,
    TapInfo,
    events::{Change, Hub},
    gate::Sessions,
    listener::{self, HostTable, ListenerState},
    replay::{self, ReplayOptions},
    tap::{Active, TapRuntime},
    util::now_unix_ms,
};

/// Options for a [`Lens`] runtime.
#[derive(Debug, Clone)]
pub struct LensOptions {
    /// Where captures go (default: an in-memory ring of 1,000 exchanges per tap).
    pub store: Option<Arc<dyn CaptureStore>>,
    /// Capacity of the live event channel.
    pub event_buffer: usize,
    /// Password checks (Argon2) allowed at once, bounding CPU under a login flood.
    pub max_password_checks: usize,
}

impl Default for LensOptions {
    fn default() -> Self {
        Self {
            store: None,
            event_buffer: 4_096,
            max_password_checks: 4,
        }
    }
}

/// A tap started with its own listener ([`Lens::start_tap`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TapHandle {
    /// The tap.
    pub id: TapId,
    /// Its listener.
    pub listener: ListenerId,
    /// Where it listens (point cloudflared here).
    pub addr: std::net::SocketAddr,
}

/// Options for [`Lens::wait_for`].
#[derive(Debug, Clone)]
pub struct WaitOptions {
    /// Give up after this long.
    pub timeout: Duration,
    /// Also accept exchanges that started at or after this time (Unix milliseconds),
    /// including ones already captured. Default: the moment of the call.
    pub since_ms: Option<u64>,
    /// How text filters see secrets.
    pub redaction: Redaction,
}

impl Default for WaitOptions {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            since_ms: None,
            redaction: Redaction::masked(),
        }
    }
}

#[derive(Debug)]
struct ListenerEntry {
    state: Arc<ListenerState>,
    task: tokio::task::JoinHandle<()>,
}

/// State shared by the runtime and every connection.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) hub: Arc<Hub>,
    pub(crate) sessions: Sessions,
    taps: RwLock<HashMap<TapId, Arc<TapRuntime>>>,
    listeners: Mutex<HashMap<ListenerId, ListenerEntry>>,
    next_listener: AtomicU64,
    pub(crate) shutdown: CancellationToken,
    /// Upgraded tunnels and other detached work, awaited on shutdown.
    pub(crate) tracker: TaskTracker,
    pub(crate) password_checks: Semaphore,
}

impl Shared {
    pub(crate) fn tap(&self, id: &TapId) -> Option<Arc<TapRuntime>> {
        self.taps
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }
}

/// The local inspecting reverse proxy.
///
/// Cheap to clone (all clones share one runtime). Must be used inside a Tokio runtime.
///
/// ```no_run
/// # async fn demo() -> Result<(), lens::LensError> {
/// use lens::{Lens, LensOptions, TapConfig, Upstream};
///
/// let lens = Lens::new(LensOptions::default())?;
/// let tap = lens.start_tap(TapConfig::new(Upstream::origin("http://localhost:3000")?)).await?;
/// println!("point cloudflared at http://{}", tap.addr);
/// let mut events = lens.subscribe();
/// # let _ = events.recv().await;
/// # Ok(()) }
/// ```
#[derive(Debug, Clone)]
pub struct Lens {
    shared: Arc<Shared>,
}

impl Lens {
    /// Creates a runtime.
    ///
    /// # Errors
    /// [`LensError::Random`] if the OS can't provide the session-signing key.
    pub fn new(options: LensOptions) -> Result<Self, LensError> {
        let store = options
            .store
            .unwrap_or_else(|| Arc::new(MemoryStore::default()));
        Ok(Self {
            shared: Arc::new(Shared {
                hub: Arc::new(Hub::new(store, options.event_buffer)),
                sessions: Sessions::new()?,
                taps: RwLock::new(HashMap::new()),
                listeners: Mutex::new(HashMap::new()),
                next_listener: AtomicU64::new(1),
                shutdown: CancellationToken::new(),
                tracker: TaskTracker::new(),
                password_checks: Semaphore::new(options.max_password_checks.max(1)),
            }),
        })
    }

    fn runtime(&self, id: &TapId) -> Result<Arc<TapRuntime>, LensError> {
        self.shared
            .tap(id)
            .ok_or_else(|| LensError::UnknownTap(id.clone()))
    }

    /// Registers a tap (without a listener; route to it with [`Lens::listen`]).
    ///
    /// # Errors
    /// Invalid configuration, an unusable upstream, or a duplicate id.
    pub fn add_tap(&self, config: TapConfig) -> Result<TapId, LensError> {
        if self.shared.shutdown.is_cancelled() {
            return Err(LensError::Closed);
        }
        let id = config.id.clone().unwrap_or_else(TapId::random);
        let capacity = config.capture.capacity;
        let active = Active::build(config, None)?;
        let mut taps = self
            .shared
            .taps
            .write()
            .unwrap_or_else(PoisonError::into_inner);
        if taps.contains_key(&id) {
            return Err(LensError::DuplicateTap(id));
        }
        if let Some(capacity) = capacity {
            self.shared.hub.store.configure(&id, capacity);
        }
        taps.insert(id.clone(), Arc::new(TapRuntime::new(id.clone(), active)));
        Ok(id)
    }

    /// Registers a tap and gives it its own loopback listener on a random port.
    ///
    /// # Errors
    /// As [`Lens::add_tap`] and [`Lens::listen`].
    pub async fn start_tap(&self, config: TapConfig) -> Result<TapHandle, LensError> {
        let id = self.add_tap(config)?;
        match self.listen(ListenOptions::tap(id.clone())).await {
            Ok(info) => Ok(TapHandle {
                id,
                listener: info.id,
                addr: info.addr,
            }),
            Err(err) => {
                self.shared
                    .taps
                    .write()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&id);
                Err(err)
            }
        }
    }

    /// Changes a tap's configuration at runtime. In-flight requests finish with the old
    /// configuration; pooled upstream connections are kept when the upstream is
    /// unchanged. Changing credentials signs every visitor out.
    ///
    /// # Errors
    /// [`LensError::UnknownTap`], or the new configuration is invalid (nothing changes).
    pub fn update_tap(
        &self,
        id: &TapId,
        change: impl FnOnce(&mut TapConfig),
    ) -> Result<(), LensError> {
        let runtime = self.runtime(id)?;
        let current = runtime.active();
        let mut config = current.config.clone();
        change(&mut config);
        config.id = Some(id.clone());
        let capacity = config.capture.capacity;
        let active = Active::build(config, Some(&current))?;
        if let Some(capacity) = capacity {
            self.shared.hub.store.configure(id, capacity);
        }
        runtime.replace(active);
        Ok(())
    }

    /// A tap's current configuration.
    ///
    /// # Errors
    /// [`LensError::UnknownTap`].
    pub fn tap_config(&self, id: &TapId) -> Result<TapConfig, LensError> {
        Ok(self.runtime(id)?.active().config.clone())
    }

    /// Summaries of all taps.
    pub fn taps(&self) -> Vec<TapInfo> {
        let mut taps: Vec<TapInfo> = self
            .shared
            .taps
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|tap| tap.info())
            .collect();
        taps.sort_by(|a, b| a.id.cmp(&b.id));
        taps
    }

    /// Removes a tap: closes listeners dedicated to it and drops it from host routes.
    /// Its captures stay until [`Lens::clear`].
    ///
    /// # Errors
    /// [`LensError::UnknownTap`].
    pub async fn remove_tap(&self, id: &TapId) -> Result<(), LensError> {
        let removed = self
            .shared
            .taps
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
        if removed.is_none() {
            return Err(LensError::UnknownTap(id.clone()));
        }
        let mut to_close = Vec::new();
        {
            let listeners = self
                .shared
                .listeners
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for (listener_id, entry) in listeners.iter() {
                let routes = entry.state.routes();
                if !routes.refers_to(id) {
                    continue;
                }
                match routes.without(id) {
                    None => to_close.push(*listener_id),
                    Some(routing) => {
                        if let Ok(table) = HostTable::compile(routing) {
                            entry.state.set_routes(table);
                        }
                    }
                }
            }
        }
        for listener_id in to_close {
            let _ = self.close_listener(listener_id).await;
        }
        Ok(())
    }

    /// Binds a listener.
    ///
    /// # Errors
    /// [`LensError::NotLoopback`] unless opted in, [`LensError::Bind`], invalid routes,
    /// or [`LensError::Closed`] after shutdown.
    pub async fn listen(&self, options: ListenOptions) -> Result<ListenerInfo, LensError> {
        if self.shared.shutdown.is_cancelled() {
            return Err(LensError::Closed);
        }
        for tap in options.routing.taps() {
            self.runtime(&tap)?;
        }
        let id = ListenerId(self.shared.next_listener.fetch_add(1, Relaxed));
        let (state, socket, acceptor) = listener::bind(&self.shared, id, options).await?;
        let addr = state.addr;
        let task = tokio::spawn(listener::run(
            Arc::clone(&self.shared),
            Arc::clone(&state),
            socket,
            acceptor,
        ));
        self.shared
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id, ListenerEntry { state, task });
        Ok(ListenerInfo { id, addr })
    }

    /// Replaces a listener's routing (e.g. add a local HTTPS domain).
    ///
    /// # Errors
    /// [`LensError::UnknownListener`], unknown taps or invalid hosts.
    pub fn set_routing(&self, listener: ListenerId, routing: Routing) -> Result<(), LensError> {
        for tap in routing.taps() {
            self.runtime(&tap)?;
        }
        let table = HostTable::compile(routing)?;
        let listeners = self
            .shared
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = listeners
            .get(&listener)
            .ok_or(LensError::UnknownListener(listener))?;
        entry.state.set_routes(table);
        Ok(())
    }

    /// Listeners and where they listen.
    pub fn listeners(&self) -> Vec<ListenerInfo> {
        let mut out: Vec<ListenerInfo> = self
            .shared
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .map(|entry| ListenerInfo {
                id: entry.state.id,
                addr: entry.state.addr,
            })
            .collect();
        out.sort_by_key(|info| info.id);
        out
    }

    /// Stops accepting, lets in-flight requests finish (up to the grace period), and
    /// closes the socket and the listener's upgraded tunnels.
    ///
    /// # Errors
    /// [`LensError::UnknownListener`].
    pub async fn close_listener(&self, id: ListenerId) -> Result<(), LensError> {
        let entry = self
            .shared
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&id)
            .ok_or(LensError::UnknownListener(id))?;
        entry.state.cancel.cancel();
        let _ = entry.task.await;
        let grace = entry.state.limits.shutdown_grace + Duration::from_secs(1);
        let _ = tokio::time::timeout(grace, entry.state.tracker.wait()).await;
        Ok(())
    }

    /// Shuts everything down: listeners, tunnels, and new work.
    pub async fn shutdown(&self) {
        let ids: Vec<ListenerId> = self
            .shared
            .listeners
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .keys()
            .copied()
            .collect();
        self.shared.shutdown.cancel();
        for id in ids {
            let _ = self.close_listener(id).await;
        }
        self.shared.tracker.close();
        let _ = tokio::time::timeout(Duration::from_secs(5), self.shared.tracker.wait()).await;
    }

    /// One captured exchange.
    pub fn get(&self, id: ExchangeId) -> Option<Arc<Exchange>> {
        self.shared.hub.store.get(id)
    }

    /// A page of captured exchanges, newest first.
    pub fn list(&self, query: &Query, redaction: &Redaction) -> Page {
        self.shared.hub.store.list(query, redaction)
    }

    /// Forgets captures of one tap, or of all taps.
    pub fn clear(&self, tap: Option<&TapId>) {
        self.shared.hub.store.clear(tap);
        let _ = self
            .shared
            .hub
            .events
            .send(LensEvent::Cleared { tap: tap.cloned() });
    }

    /// Live updates (added, updated, completed exchanges; clears).
    pub fn subscribe(&self) -> broadcast::Receiver<LensEvent> {
        self.shared.hub.events.subscribe()
    }

    /// Waits for a finished exchange matching `filter` (for webhook tests and the MCP
    /// `wait_for_request` tool). Returns the oldest match since `options.since_ms`.
    ///
    /// # Errors
    /// [`LensError::WaitTimeout`], or [`LensError::Closed`] on shutdown.
    pub async fn wait_for(
        &self,
        filter: &Filter,
        options: &WaitOptions,
    ) -> Result<Arc<Exchange>, LensError> {
        let since = options.since_ms.unwrap_or_else(now_unix_ms);
        let mut filter = filter.clone();
        filter.since_ms = Some(filter.since_ms.map_or(since, |s| s.max(since)));
        filter.finished_only = true;
        // Subscribe before looking, so nothing slips between the two.
        let mut events = self.subscribe();
        let deadline = tokio::time::Instant::now() + options.timeout;
        let mut look = true;
        loop {
            if look {
                look = false;
                if let Some(found) = self.oldest_match(&filter, &options.redaction) {
                    return Ok(found);
                }
            }
            let event = tokio::select! {
                event = events.recv() => event,
                () = tokio::time::sleep_until(deadline) => return Err(LensError::WaitTimeout),
                () = self.shared.shutdown.cancelled() => return Err(LensError::Closed),
            };
            match event {
                Ok(LensEvent::Exchange {
                    change: Change::Completed,
                    exchange,
                }) if filter.matches(&exchange, &options.redaction) => return Ok(exchange),
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => look = true,
                Err(broadcast::error::RecvError::Closed) => return Err(LensError::Closed),
            }
        }
    }

    fn oldest_match(&self, filter: &Filter, redaction: &Redaction) -> Option<Arc<Exchange>> {
        let mut query = Query {
            filter: filter.clone(),
            limit: Some(crate::store::MAX_PAGE),
            before: None,
        };
        let mut oldest = None;
        loop {
            let page = self.list(&query, redaction);
            if let Some(last) = page.items.last() {
                oldest = Some(Arc::clone(last));
            }
            match page.next {
                Some(next) => query.before = Some(next),
                None => return oldest,
            }
        }
    }

    /// Counters and latency percentiles of a tap.
    ///
    /// # Errors
    /// [`LensError::UnknownTap`].
    pub fn metrics(&self, id: &TapId) -> Result<MetricsSnapshot, LensError> {
        Ok(self.runtime(id)?.metrics.snapshot())
    }

    /// Sends a captured request again (optionally edited, repeated, or to another
    /// upstream). Each replay is captured, marked `replay_of`, and returned once
    /// finished.
    ///
    /// # Errors
    /// [`LensError::UnknownExchange`], [`LensError::UnknownTap`] (the tap was removed),
    /// [`LensError::BodyTruncated`], or invalid edits.
    pub async fn replay(
        &self,
        id: ExchangeId,
        options: &ReplayOptions,
    ) -> Result<Vec<Arc<Exchange>>, LensError> {
        let original = self.get(id).ok_or(LensError::UnknownExchange(id))?;
        let runtime = self.runtime(&original.tap)?;
        replay::run(&self.shared, &runtime, &original, options).await
    }
}
