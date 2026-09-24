//! The inspector: Lens (`crates/lens`) in this process, and which share or route each
//! of its taps inspects (M12-02, D-100).
//!
//! One [`Inspector`] per process (the app, `teitunnel share`/`inspect`/`serve`/`mcp`);
//! its Lens starts on first use. A tap sits between cloudflared and a local service:
//! Quick Shares point cloudflared's `--url` at it (on by default), and routes are pointed
//! at it through a plan while inspected ([`routes`]). Captures stay in memory (1,000
//! per tap) and, masked, in the database for a day ([`history`]); nothing leaves the
//! process except through an explicit export, the app's IPC or an agent's traffic tools
//! (masked unless revealed).

pub mod analytics;
pub mod expose;
mod history;
pub mod live;
mod record;
pub mod routes;
pub mod secrets;
mod settings;
mod views;

use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    sync::{
        Arc, Mutex, PoisonError, Weak,
        atomic::{AtomicU32, Ordering},
    },
    time::Duration,
};

pub use history::{
    HISTORY_BYTES, HistoryQuery, MAX_PER_TAP, history, history_after, history_clear, history_get,
};
pub use lens;
use lens::{
    BasicAuth, BearerToken, CaptureStore, Change, Exchange, ExchangeId, Gates, HostHeader, Lens,
    LensError, LensEvent, LensOptions, MetricsSnapshot, OriginConfig, OriginUrl, PasswordGate,
    PathPattern, RandomSource, Redaction, ReplayOptions, RequestEdits, Resign, SecretLink,
    TapConfig, TapId, Upstream, export::ExportFormat, webhook,
};
pub use live::{LIVE_INTERVAL, LiveBatch, follow};
pub use record::{PERSIST_BODY_BYTES, is_masked};
use rusqlite::params;
use serde::Serialize;
pub use settings::{InspectorSettings, InspectorSettingsPatch, MAX_RETENTION_HOURS};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
pub use views::{
    ExchangeDetail, ExchangePage, ExchangeQuery, ExchangeRow, NetworkPreset, ProtectionInput,
    ProtectionResult, ProtectionView, ReplayInput, TapPatch, TapScope, TapView, TrafficFormat,
    WebhookCheck, WebhookSender, WebhookVerdict,
};

use crate::{
    Secret,
    secrets::{SecretError, Secrets},
    store::{Store, StoreError},
    text::{Text, UserText, english_display, msg},
};

/// How often taps are checked for idleness.
const IDLE_CHECK: Duration = Duration::from_secs(15);

/// Why an inspector operation failed.
#[derive(Debug, thiserror::Error)]
pub enum InspectError {
    /// Lens refused (bad configuration, a port couldn't be bound…).
    Lens(#[from] LensError),
    /// No such tap (the share or route isn't inspected any more).
    UnknownTap,
    /// The exchange isn't captured (any more).
    UnknownExchange,
    /// Only HTTP(S) services can be inspected.
    NotWeb,
    /// This hostname isn't one of this machine's routes.
    NotRoute(String),
    /// This route isn't being inspected.
    NotInspected(String),
    /// The exchange was captured by a tap that has stopped.
    TapGone,
    /// A value was rejected (a path pattern, a network…).
    Invalid(String),
    /// The keychain failed.
    Secret(#[from] SecretError),
    /// The database failed.
    Store(#[from] StoreError),
    /// The change to the route failed.
    Engine(#[from] crate::engine::EngineError),
}

impl UserText for InspectError {
    fn text(&self) -> Text {
        use msg::error::inspect as m;
        match self {
            Self::Lens(err) => m::lens(err),
            Self::UnknownTap => m::unknown_tap(),
            Self::UnknownExchange => m::unknown_exchange(),
            Self::NotWeb => m::not_web(),
            Self::NotRoute(hostname) => m::not_route(hostname),
            Self::NotInspected(hostname) => m::not_inspected(hostname),
            Self::TapGone => m::tap_gone(),
            Self::Invalid(detail) => m::invalid(detail),
            Self::Secret(err) => err.text(),
            Self::Store(err) => err.text(),
            Self::Engine(err) => err.text(),
        }
    }
}

english_display!(InspectError);

/// Something the host should act on or tell the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InspectEvent {
    /// A tap started, stopped or changed.
    Taps,
    /// A request hit a watched path.
    Watched {
        /// The tap.
        tap: TapId,
        /// What it inspects.
        scope: TapScope,
        /// Its name.
        name: String,
        /// The exchange.
        exchange: ExchangeId,
        /// Method.
        method: String,
        /// Path.
        path: String,
    },
    /// A tap had no request for its idle limit (the host stops the share).
    Idle {
        /// The tap.
        tap: TapId,
        /// What it inspects.
        scope: TapScope,
        /// Its name.
        name: String,
        /// The limit, in minutes.
        minutes: u32,
    },
}

/// How to reach a service over TLS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginTls {
    /// Check the service's certificate.
    pub verify: bool,
    /// Name for SNI and the certificate check.
    pub server_name: Option<String>,
    /// Speak HTTP/2 to it.
    pub http2: bool,
}

impl Default for OriginTls {
    fn default() -> Self {
        Self {
            verify: true,
            server_name: None,
            http2: false,
        }
    }
}

/// A tap to start.
#[derive(Debug, Clone)]
pub struct TapSpec {
    /// What it inspects.
    pub scope: TapScope,
    /// Display name.
    pub name: String,
    /// The local service, e.g. `http://localhost:3000`.
    pub origin: String,
    /// Host header for the service (`None`: the visitor's).
    pub host_header: Option<String>,
    /// TLS to the service.
    pub tls: OriginTls,
    /// The public URL, if known.
    pub public_url: Option<String>,
    /// Bearer tokens required (`Authorization: Bearer …`).
    pub bearer: Vec<Secret<String>>,
}

impl TapSpec {
    /// A tap for `scope` forwarding to `origin`, with defaults.
    pub fn new(scope: TapScope, name: &str, origin: &str) -> Self {
        Self {
            scope,
            name: name.to_owned(),
            origin: origin.to_owned(),
            host_header: None,
            tls: OriginTls::default(),
            public_url: None,
            bearer: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct TapEntry {
    scope: TapScope,
    name: String,
    origin: String,
    public_url: Option<String>,
    address: SocketAddr,
    started_at: u64,
    watched: Vec<String>,
    idle_stop: Option<Duration>,
    last_activity: tokio::time::Instant,
    idle_reported: bool,
}

/// A tap this process knows about, running or not (captures keep naming it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct KnownTap {
    /// Id.
    pub id: TapId,
    /// What it inspected.
    pub scope: TapScope,
    /// Name.
    pub name: String,
    /// The local service.
    pub origin: String,
    /// Still running in this process.
    pub running: bool,
}

#[derive(Debug)]
struct Inner {
    store: Option<Store>,
    secrets: Option<Secrets>,
    owner: String,
    captures: Arc<history::Captures>,
    retention: Arc<AtomicU32>,
    settings: Mutex<InspectorSettings>,
    lens: Mutex<Option<Lens>>,
    taps: Mutex<HashMap<TapId, TapEntry>>,
    known: Mutex<HashMap<TapId, (TapScope, String, String)>>,
    restored: Mutex<HashSet<ExchangeId>>,
    events: broadcast::Sender<InspectEvent>,
    stop: CancellationToken,
    tasks: Mutex<Vec<tokio::task::JoinHandle<()>>>,
    random: Option<Arc<dyn RandomSource>>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// This process's inspector. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Inspector {
    inner: Arc<Inner>,
}

/// A tap id: the Quick Share's id, or `rt-` + a digest of the route + a random part (a
/// new one each time, so captures of earlier runs never mix with new ones). `None`
/// lets Lens choose.
fn tap_id_for(scope: &TapScope) -> Option<TapId> {
    match scope {
        TapScope::QuickShare { share_id } => TapId::new(share_id).ok(),
        TapScope::Route {
            account_id,
            hostname,
            path,
        } => {
            let mut hash = Sha256::new();
            hash.update(account_id.as_bytes());
            hash.update([0]);
            hash.update(hostname.as_bytes());
            hash.update([0]);
            hash.update(path.as_deref().unwrap_or_default().as_bytes());
            let digest: String = hash.finalize()[..6]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            let mut random = [0u8; 3];
            getrandom::fill(&mut random).ok()?;
            let random: String = random.iter().map(|b| format!("{b:02x}")).collect();
            TapId::new(&format!("rt-{digest}-{random}")).ok()
        }
    }
}

fn duration_minutes(minutes: u32) -> Duration {
    Duration::from_secs(u64::from(minutes) * 60)
}

impl Inspector {
    /// An inspector for this process. `store` keeps settings and history (none: memory
    /// only); `secrets` holds webhook secrets and tokens; `owner` is
    /// [`crate::domain_shares::APP_OWNER`] or [`crate::runtime::this_process`].
    pub fn new(store: Option<Store>, secrets: Option<Secrets>, owner: &str) -> Self {
        let (events, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(Inner {
                store,
                secrets,
                owner: owner.to_owned(),
                captures: Arc::new(history::Captures::new()),
                retention: Arc::new(AtomicU32::new(24)),
                settings: Mutex::new(InspectorSettings::default()),
                lens: Mutex::new(None),
                taps: Mutex::new(HashMap::new()),
                known: Mutex::new(HashMap::new()),
                restored: Mutex::new(HashSet::new()),
                events,
                stop: CancellationToken::new(),
                tasks: Mutex::new(Vec::new()),
                random: None,
            }),
        }
    }

    /// Uses `random` for network simulation and faults (deterministic tests). Call before
    /// the first tap starts.
    #[must_use]
    pub fn with_random(self, random: Arc<dyn RandomSource>) -> Self {
        match Arc::try_unwrap(self.inner) {
            Ok(mut inner) => {
                inner.random = Some(random);
                Self {
                    inner: Arc::new(inner),
                }
            }
            Err(inner) => Self { inner },
        }
    }

    /// Reads the settings and puts the recent history back in memory. Call once at
    /// start (without a database there's nothing to read).
    ///
    /// # Errors
    /// The database can't be read.
    pub async fn load(&self) -> Result<(), InspectError> {
        let Some(store) = &self.inner.store else {
            return Ok(());
        };
        let settings = settings::load(store).await?;
        self.apply_settings(&settings);
        if settings.keep_history {
            let exchanges = history::load_recent(store, settings.retention_hours).await?;
            lock(&self.inner.restored).extend(exchanges.iter().map(|e| e.id));
            self.inner.captures.restore(exchanges);
        }
        let taps = store
            .call(|conn| {
                let mut stmt = conn.prepare("SELECT id, scope, name, origin FROM lens_taps")?;
                let rows = stmt.query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                })?;
                Ok(rows.collect::<Result<Vec<_>, _>>()?)
            })
            .await?;
        let mut known = lock(&self.inner.known);
        for (id, scope, name, origin) in taps {
            if let (Ok(id), Ok(scope)) = (TapId::new(&id), serde_json::from_str(&scope)) {
                known.entry(id).or_insert((scope, name, origin));
            }
        }
        Ok(())
    }

    fn apply_settings(&self, settings: &InspectorSettings) {
        self.inner
            .retention
            .store(settings.retention_hours, Ordering::Relaxed);
        self.inner.captures.set_keep(settings.keep_history);
        *lock(&self.inner.settings) = settings.clone();
    }

    /// The current settings.
    pub fn settings(&self) -> InspectorSettings {
        lock(&self.inner.settings).clone()
    }

    /// Changes the settings (saved when there's a database).
    ///
    /// # Errors
    /// The database can't be written.
    pub async fn update_settings(
        &self,
        patch: InspectorSettingsPatch,
    ) -> Result<InspectorSettings, InspectError> {
        let settings = match &self.inner.store {
            Some(store) => settings::update(store, patch).await?,
            None => {
                let mut settings = self.settings();
                settings.apply(patch);
                settings
            }
        };
        self.apply_settings(&settings);
        Ok(settings)
    }

    /// Who runs this inspector.
    pub fn owner(&self) -> &str {
        &self.inner.owner
    }

    /// The database, if any.
    pub fn store(&self) -> Option<&Store> {
        self.inner.store.as_ref()
    }

    /// The keychain, if any.
    pub fn secrets(&self) -> Option<&Secrets> {
        self.inner.secrets.as_ref()
    }

    /// Live events.
    pub fn subscribe(&self) -> broadcast::Receiver<InspectEvent> {
        self.inner.events.subscribe()
    }

    /// Every exchange change from now on (starting Lens if it isn't running).
    ///
    /// # Errors
    /// Lens couldn't start.
    pub fn live(&self) -> Result<broadcast::Receiver<LensEvent>, InspectError> {
        Ok(self.lens()?.subscribe())
    }

    /// Lens, if it has started.
    pub fn running(&self) -> Option<Lens> {
        lock(&self.inner.lens).clone()
    }

    fn emit(&self, event: InspectEvent) {
        let _ = self.inner.events.send(event);
    }

    /// Lens, started on first use (with the history writer and the watcher). Must be
    /// called inside a Tokio runtime.
    fn lens(&self) -> Result<Lens, InspectError> {
        let mut guard = lock(&self.inner.lens);
        if let Some(lens) = guard.as_ref() {
            return Ok(lens.clone());
        }
        if self.inner.stop.is_cancelled() {
            return Err(LensError::Closed.into());
        }
        let captures: Arc<dyn CaptureStore> = self.inner.captures.clone();
        let lens = Lens::new(LensOptions {
            store: Some(captures),
            random: self.inner.random.clone(),
            ..LensOptions::default()
        })?;
        let mut tasks = lock(&self.inner.tasks);
        if let Some(store) = &self.inner.store {
            let (writer, queue) = history::channel();
            self.inner.captures.attach(writer);
            tasks.push(tokio::spawn(history::run_writer(
                store.clone(),
                queue,
                Arc::clone(&self.inner.retention),
                self.inner.stop.clone(),
            )));
        }
        tasks.push(tokio::spawn(watch(
            Arc::downgrade(&self.inner),
            lens.subscribe(),
            self.inner.stop.clone(),
        )));
        *guard = Some(lens.clone());
        Ok(lens)
    }

    /// The tap inspecting `scope`, if any.
    pub fn tap_for(&self, scope: &TapScope) -> Option<TapId> {
        lock(&self.inner.taps)
            .iter()
            .find(|(_, entry)| entry.scope == *scope)
            .map(|(id, _)| id.clone())
    }

    /// Where a tap listens, as a URL for cloudflared.
    pub fn tap_url(&self, tap: &TapId) -> Option<String> {
        lock(&self.inner.taps)
            .get(tap)
            .map(|entry| format!("http://{}", entry.address))
    }

    /// Starts a tap (replacing one already inspecting the same scope).
    ///
    /// # Errors
    /// [`InspectError::NotWeb`] for a service that isn't HTTP(S); Lens errors.
    pub async fn start(&self, spec: TapSpec) -> Result<TapView, InspectError> {
        if let Some(existing) = self.tap_for(&spec.scope) {
            self.stop(&existing).await;
        }
        let url = OriginUrl::parse(spec.origin.trim().trim_end_matches('/'))
            .map_err(|_| InspectError::NotWeb)?;
        let mut origin = OriginConfig::new(url);
        origin.verify_tls = spec.tls.verify;
        origin.server_name.clone_from(&spec.tls.server_name);
        origin.http2 = spec.tls.http2;
        let mut config = TapConfig::new(Upstream::Origin(origin));
        config.id = tap_id_for(&spec.scope);
        config.name.clone_from(&spec.name);
        config.host_header = spec
            .host_header
            .clone()
            .map_or(HostHeader::Preserve, HostHeader::Custom);
        // The listener is on loopback and fed by cloudflared, which sets the visitor's
        // address in `CF-Connecting-IP`.
        config.trust_cf_connecting_ip = true;
        for token in &spec.bearer {
            config.gates.bearer.push(BearerToken::new(token.expose())?);
        }
        let settings = self.settings();
        let lens = self.lens()?;
        let handle = lens.start_tap(config).await?;
        let id = handle.id.clone();
        let started_at = crate::domain_shares::now_ms();
        let entry = TapEntry {
            scope: spec.scope.clone(),
            name: spec.name.clone(),
            origin: spec.origin.clone(),
            public_url: spec.public_url.clone(),
            address: handle.addr,
            started_at,
            watched: Vec::new(),
            idle_stop: settings.idle_stop_minutes.map(duration_minutes),
            last_activity: tokio::time::Instant::now(),
            idle_reported: false,
        };
        lock(&self.inner.taps).insert(id.clone(), entry);
        lock(&self.inner.known).insert(
            id.clone(),
            (spec.scope.clone(), spec.name.clone(), spec.origin.clone()),
        );
        self.record_tap(&id, &spec, started_at);
        self.emit(InspectEvent::Taps);
        self.view(&id)
    }

    fn record_tap(&self, id: &TapId, spec: &TapSpec, started_at: u64) {
        let Some(store) = self.inner.store.clone() else {
            return;
        };
        let (id, scope, name, origin, url, owner) = (
            id.to_string(),
            serde_json::to_string(&spec.scope).unwrap_or_default(),
            spec.name.clone(),
            spec.origin.clone(),
            spec.public_url.clone(),
            self.inner.owner.clone(),
        );
        let started_at = i64::try_from(started_at).unwrap_or(i64::MAX);
        tokio::spawn(async move {
            let saved = store
                .call(move |conn| {
                    conn.execute(
                        "INSERT INTO lens_taps (id, scope, name, origin, public_url, owner, started_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                         ON CONFLICT (id) DO UPDATE SET scope = ?2, name = ?3, origin = ?4,
                           public_url = ?5, owner = ?6, started_at = ?7, stopped_at = NULL",
                        params![id, scope, name, origin, url, owner, started_at],
                    )?;
                    Ok(())
                })
                .await;
            if let Err(err) = saved {
                tracing::debug!(%err, "couldn't record an inspector tap");
            }
        });
    }

    fn update_tap_row(&self, id: &TapId, sql: &'static str, value: Option<String>) {
        let Some(store) = self.inner.store.clone() else {
            return;
        };
        let id = id.to_string();
        tokio::spawn(async move {
            let _ = store
                .call(move |conn| {
                    conn.execute(sql, params![id, value])?;
                    Ok(())
                })
                .await;
        });
    }

    /// Stops a tap (its captures stay).
    pub async fn stop(&self, tap: &TapId) {
        let removed = lock(&self.inner.taps).remove(tap);
        if removed.is_none() {
            return;
        }
        if let Some(lens) = self.running() {
            let _ = lens.remove_tap(tap).await;
        }
        self.update_tap_row(
            tap,
            "UPDATE lens_taps SET stopped_at = CAST(?2 AS INTEGER) WHERE id = ?1",
            Some(crate::domain_shares::now_ms().to_string()),
        );
        self.emit(InspectEvent::Taps);
    }

    /// Sets the public URL once known (a Quick Share's address arrives later).
    pub fn set_public_url(&self, tap: &TapId, url: Option<String>) {
        if let Some(entry) = lock(&self.inner.taps).get_mut(tap) {
            entry.public_url.clone_from(&url);
            if let Some(url) = &url {
                entry.name.clone_from(url);
            }
        } else {
            return;
        }
        self.update_tap_row(
            tap,
            "UPDATE lens_taps SET public_url = ?2, name = coalesce(?2, name) WHERE id = ?1",
            url,
        );
        self.emit(InspectEvent::Taps);
    }

    /// Changes the Host header the service gets, at once (no restart).
    ///
    /// # Errors
    /// Unknown tap or an invalid header.
    pub fn set_host_header(&self, tap: &TapId, host: Option<String>) -> Result<(), InspectError> {
        self.configure(
            tap,
            TapPatch {
                host_header: Some(host.unwrap_or_default()),
                ..TapPatch::default()
            },
        )
        .map(|_| ())
    }

    /// Running taps.
    pub fn taps(&self) -> Vec<TapView> {
        let mut ids: Vec<(u64, TapId)> = lock(&self.inner.taps)
            .iter()
            .map(|(id, entry)| (entry.started_at, id.clone()))
            .collect();
        ids.sort();
        ids.into_iter()
            .filter_map(|(_, id)| self.view(&id).ok())
            .collect()
    }

    /// Every tap this process knows (running, stopped, or from the history).
    pub fn known_taps(&self) -> Vec<KnownTap> {
        let running: HashSet<TapId> = lock(&self.inner.taps).keys().cloned().collect();
        let mut taps: Vec<KnownTap> = lock(&self.inner.known)
            .iter()
            .map(|(id, (scope, name, origin))| KnownTap {
                id: id.clone(),
                scope: scope.clone(),
                name: name.clone(),
                origin: origin.clone(),
                running: running.contains(id),
            })
            .collect();
        taps.sort_by(|a, b| a.id.cmp(&b.id));
        taps
    }

    /// One tap.
    ///
    /// # Errors
    /// [`InspectError::UnknownTap`].
    pub fn view(&self, tap: &TapId) -> Result<TapView, InspectError> {
        let entry = lock(&self.inner.taps)
            .get(tap)
            .cloned()
            .ok_or(InspectError::UnknownTap)?;
        let lens = self.running().ok_or(InspectError::UnknownTap)?;
        let config = lens.tap_config(tap).map_err(|_| InspectError::UnknownTap)?;
        let metrics = lens.metrics(tap).unwrap_or_default();
        let gates = &config.gates;
        Ok(TapView {
            id: tap.clone(),
            scope: entry.scope,
            name: entry.name,
            origin: entry.origin,
            public_url: entry.public_url,
            address: format!("http://{}", entry.address),
            started_at: entry.started_at,
            capturing: config.capture.enabled,
            paused: config.paused.clone(),
            protection: ProtectionView {
                password: gates.password.is_some(),
                secret_link: gates.secret_link.is_some(),
                basic_user: gates.basic.as_ref().map(|b| b.user().to_owned()),
                bearer_tokens: u32::try_from(gates.bearer.len()).unwrap_or(u32::MAX),
                ip_allow: gates.ip_allow.iter().map(ToString::to_string).collect(),
                ip_deny: gates.ip_deny.iter().map(ToString::to_string).collect(),
                agent_presets: gates.agent_presets.clone(),
                agent_patterns: gates.agent_patterns.clone(),
                bypass: gates.bypass.iter().map(PathPattern::as_text).collect(),
            },
            host_header: match &config.host_header {
                HostHeader::Custom(host) => Some(host.clone()),
                HostHeader::Upstream => Some(config.upstream.describe()),
                HostHeader::Preserve => None,
            },
            sse_keepalive_secs: config
                .sse_keepalive
                .map(|d| u32::try_from(d.as_secs()).unwrap_or(u32::MAX)),
            stubs: config.stubs.clone(),
            header_rules: config.headers.clone(),
            network: config.network,
            faults: config.faults.clone(),
            watched_paths: entry.watched,
            idle_stop_minutes: entry
                .idle_stop
                .map(|d| u32::try_from(d.as_secs() / 60).unwrap_or(u32::MAX)),
            requests: metrics.requests,
        })
    }

    /// Changes a tap's settings at once.
    ///
    /// # Errors
    /// Unknown tap, or a rejected value (nothing changes then).
    pub fn configure(&self, tap: &TapId, patch: TapPatch) -> Result<TapView, InspectError> {
        let lens = self.running().ok_or(InspectError::UnknownTap)?;
        if !lock(&self.inner.taps).contains_key(tap) {
            return Err(InspectError::UnknownTap);
        }
        let watched = match &patch.watched_paths {
            Some(paths) => {
                let paths: Vec<String> = paths
                    .iter()
                    .map(|p| p.trim().to_owned())
                    .filter(|p| !p.is_empty())
                    .collect();
                for path in &paths {
                    PathPattern::parse(path).map_err(|e| InspectError::Invalid(e.to_string()))?;
                }
                Some(paths)
            }
            None => None,
        };
        if let Some(Some(host)) = patch
            .host_header
            .as_ref()
            .map(|h| (!h.trim().is_empty()).then_some(h))
            && crate::dev_server::parse_host_header(host).is_none()
        {
            return Err(InspectError::Invalid(format!("{host} isn't a host name")));
        }
        lens.update_tap(tap, |config| {
            if let Some(on) = patch.capturing {
                config.capture.enabled = on;
            }
            match (patch.paused, &patch.paused_page) {
                (Some(false), _) => config.paused = None,
                (Some(true), page) => {
                    config.paused = Some(page.clone().unwrap_or_default());
                }
                (None, Some(page)) if config.paused.is_some() => {
                    config.paused = Some(page.clone());
                }
                (None, _) => {}
            }
            if let Some(stubs) = &patch.stubs {
                config.stubs.clone_from(stubs);
            }
            if let Some(rules) = &patch.header_rules {
                config.headers.clone_from(rules);
            }
            if let Some(preset) = patch.network_preset {
                config.network = preset.config();
            }
            if let Some(network) = patch.network {
                config.network = network;
            }
            if let Some(faults) = &patch.faults {
                config.faults.clone_from(faults);
            }
            if let Some(secs) = patch.sse_keepalive_secs {
                config.sse_keepalive = (secs > 0).then(|| Duration::from_secs(u64::from(secs)));
            }
            if let Some(host) = &patch.host_header {
                config.host_header = crate::dev_server::parse_host_header(host)
                    .map_or(HostHeader::Preserve, HostHeader::Custom);
            }
        })?;
        if let Some(entry) = lock(&self.inner.taps).get_mut(tap) {
            if let Some(watched) = watched {
                entry.watched = watched;
            }
            if let Some(minutes) = patch.idle_stop_minutes {
                entry.idle_stop = (minutes > 0).then(|| duration_minutes(minutes));
                entry.idle_reported = false;
                entry.last_activity = tokio::time::Instant::now();
            }
        }
        self.emit(InspectEvent::Taps);
        self.view(tap)
    }

    /// Changes a tap's protection. Generated secrets are returned once and not kept
    /// anywhere else.
    ///
    /// # Errors
    /// Unknown tap, or a rejected value (nothing changes then).
    pub async fn protect(
        &self,
        tap: &TapId,
        input: ProtectionInput,
    ) -> Result<ProtectionResult, InspectError> {
        let lens = self.running().ok_or(InspectError::UnknownTap)?;
        let mut gates: Gates = lens
            .tap_config(tap)
            .map_err(|_| InspectError::UnknownTap)?
            .gates;
        if let Some(password) = input.password {
            gates.password = if password.is_empty() {
                None
            } else {
                // Argon2 takes a moment: off the async threads.
                let gate = tokio::task::spawn_blocking(move || PasswordGate::new(&password))
                    .await
                    .map_err(|e| InspectError::Invalid(e.to_string()))??;
                Some(gate)
            };
        }
        let mut secret_link_key = None;
        match input.secret_link {
            Some(true) => {
                let link = SecretLink::generate()?;
                secret_link_key = Some(link.token().expose().clone());
                gates.secret_link = Some(link);
            }
            Some(false) => gates.secret_link = None,
            None => {}
        }
        if let Some((user, password)) = input.basic {
            gates.basic = if user.is_empty() {
                None
            } else {
                Some(BasicAuth::new(&user, &password)?)
            };
        }
        let mut bearer_token = None;
        match input.bearer {
            Some(true) => {
                let token = secrets::generate_token()
                    .map_err(|e| InspectError::Lens(LensError::Random(e.to_string())))?;
                gates.bearer = vec![BearerToken::new(token.expose())?];
                bearer_token = Some(token.expose().clone());
            }
            Some(false) => gates.bearer.clear(),
            None => {}
        }
        let nets = |list: Vec<String>| -> Result<Vec<ipnet::IpNet>, InspectError> {
            list.iter()
                .map(|text| {
                    let text = text.trim();
                    text.parse::<ipnet::IpNet>()
                        .or_else(|_| text.parse::<std::net::IpAddr>().map(ipnet::IpNet::from))
                        .map_err(|_| {
                            InspectError::Invalid(format!("{text} isn't an IP address or range"))
                        })
                })
                .collect()
        };
        if let Some(list) = input.ip_allow {
            gates.ip_allow = nets(list)?;
        }
        if let Some(list) = input.ip_deny {
            gates.ip_deny = nets(list)?;
        }
        if let Some(presets) = input.agent_presets {
            gates.agent_presets = presets;
        }
        if let Some(patterns) = input.agent_patterns {
            gates.agent_patterns = patterns
                .into_iter()
                .map(|p| p.trim().to_owned())
                .filter(|p| !p.is_empty())
                .collect();
        }
        if let Some(bypass) = input.bypass {
            gates.bypass = bypass
                .iter()
                .map(|p| PathPattern::parse(p.trim()))
                .collect::<Result<_, _>>()?;
        }
        lens.update_tap(tap, move |config| config.gates = gates)?;
        self.emit(InspectEvent::Taps);
        Ok(ProtectionResult {
            protection: self.view(tap)?.protection,
            secret_link_key,
            bearer_token,
        })
    }

    /// A page of captured exchanges, newest first (masked).
    pub fn list(&self, query: &ExchangeQuery) -> ExchangePage {
        let page = self
            .inner
            .captures
            .list(&query.query(), &Redaction::masked());
        ExchangePage {
            items: page.items.iter().map(|e| ExchangeRow::of(e)).collect(),
            next: page.next,
        }
    }

    /// A page of raw captures (for statistics and exports in this process); text search
    /// runs on the masked view.
    pub fn list_raw(&self, query: &lens::Query) -> lens::Page {
        self.inner.captures.list(query, &Redaction::masked())
    }

    /// The raw captured exchange (for exports and replays in this process).
    pub fn exchange(&self, id: ExchangeId) -> Option<Arc<Exchange>> {
        self.inner.captures.get(id)
    }

    /// Whether an exchange came from the history on disk (credentials masked).
    pub fn is_restored(&self, id: ExchangeId) -> bool {
        lock(&self.inner.restored).contains(&id)
    }

    fn secret_scope(&self, tap: &TapId) -> Option<String> {
        if let Some(entry) = lock(&self.inner.taps).get(tap) {
            return Some(entry.scope.secret_scope(&entry.origin));
        }
        lock(&self.inner.known)
            .get(tap)
            .map(|(scope, _, origin)| scope.secret_scope(origin))
    }

    /// Where webhook secrets for a tap's captures are kept.
    pub fn webhook_scope(&self, tap: &TapId) -> Option<String> {
        self.secret_scope(tap)
    }

    /// One exchange in full: masked, or `reveal`ed (only after the person asked to see
    /// secrets), with its webhook signature checked when a secret is saved.
    ///
    /// # Errors
    /// [`InspectError::UnknownExchange`].
    pub async fn detail(
        &self,
        id: ExchangeId,
        reveal: bool,
    ) -> Result<ExchangeDetail, InspectError> {
        let exchange = self.exchange(id).ok_or(InspectError::UnknownExchange)?;
        let redaction = if reveal {
            Redaction::revealed()
        } else {
            Redaction::masked()
        };
        let webhook = match webhook::detect(&exchange.request.headers) {
            Some(provider) => {
                let secret = match (self.secret_scope(&exchange.tap), &self.inner.secrets) {
                    (Some(scope), Some(secrets)) => {
                        secrets::webhook_secret(secrets, &scope, provider).await?
                    }
                    _ => None,
                };
                let now = crate::domain_shares::now_ms() / 1_000;
                Some(WebhookCheck {
                    provider: provider.into(),
                    has_secret: secret.is_some(),
                    verification: secret.map(|secret| {
                        webhook::verify_exchange(&exchange, &secret, Some(provider), now).into()
                    }),
                })
            }
            None => None,
        };
        Ok(ExchangeDetail {
            view: exchange.view(&redaction),
            webhook,
            restored: self.is_restored(id),
        })
    }

    /// Sends a captured request to its service again (edited, repeated, re-signed).
    /// A request read back from the history is sent without the credentials that were
    /// masked when it was stored (unless the edits set them).
    ///
    /// # Errors
    /// Unknown exchange, a stopped tap, invalid edits, or a truncated body.
    pub async fn replay(
        &self,
        id: ExchangeId,
        input: ReplayInput,
    ) -> Result<Vec<ExchangeRow>, InspectError> {
        let exchange = self.exchange(id).ok_or(InspectError::UnknownExchange)?;
        let lens = self.running().ok_or(InspectError::TapGone)?;
        if lens.tap_config(&exchange.tap).is_err() {
            return Err(InspectError::TapGone);
        }
        let mut edits = RequestEdits {
            method: input.method.clone().filter(|m| !m.trim().is_empty()),
            path_and_query: input.path.clone().filter(|p| !p.trim().is_empty()),
            set_headers: input.set_headers.clone(),
            remove_headers: input.remove_headers.clone(),
            body: input.body.clone().map(bytes::Bytes::from),
        };
        if self.is_restored(id) {
            for (name, value) in &exchange.request.headers {
                let set = edits
                    .set_headers
                    .iter()
                    .any(|(n, _)| n.eq_ignore_ascii_case(name.as_str()));
                if !set && value.to_str().is_ok_and(is_masked) {
                    edits.remove_headers.push(name.as_str().to_owned());
                }
            }
        }
        let resign = if input.resign {
            let provider = webhook::detect(&exchange.request.headers).ok_or_else(|| {
                InspectError::Invalid("this request has no webhook signature".into())
            })?;
            let scope = self.secret_scope(&exchange.tap);
            let secret = match (scope, &self.inner.secrets) {
                (Some(scope), Some(secrets)) => {
                    secrets::webhook_secret(secrets, &scope, provider).await?
                }
                _ => None,
            }
            .ok_or_else(|| {
                InspectError::Invalid("save the webhook's signing secret first".into())
            })?;
            Some(Resign { provider, secret })
        } else {
            None
        };
        let options = ReplayOptions {
            edits,
            times: input.times.unwrap_or(1).clamp(1, lens::MAX_REPLAYS),
            resign,
            ..ReplayOptions::default()
        };
        let replays = lens.replay(id, &options).await.map_err(|err| match err {
            LensError::UnknownTap(_) => InspectError::TapGone,
            LensError::UnknownExchange(_) => InspectError::UnknownExchange,
            other => InspectError::Lens(other),
        })?;
        Ok(replays.iter().map(|e| ExchangeRow::of(e)).collect())
    }

    /// Exports exchanges (HAR puts them in one file; other formats one after another).
    /// `redact` masks secrets (the default everywhere; unticking it is explicit).
    ///
    /// # Errors
    /// [`InspectError::UnknownExchange`] for an id that isn't captured.
    pub fn export(
        &self,
        ids: &[ExchangeId],
        format: ExportFormat,
        redact: bool,
    ) -> Result<String, InspectError> {
        let redaction = if redact {
            Redaction::masked()
        } else {
            Redaction::revealed()
        };
        let exchanges: Vec<Arc<Exchange>> = ids
            .iter()
            .map(|id| self.exchange(*id).ok_or(InspectError::UnknownExchange))
            .collect::<Result<_, _>>()?;
        Ok(export(&exchanges, format, &redaction))
    }

    /// Forgets captures of one tap, or all (in memory and on disk).
    ///
    /// # Errors
    /// The database can't be written.
    pub async fn clear(&self, tap: Option<&TapId>) -> Result<(), InspectError> {
        match self.running() {
            Some(lens) => lens.clear(tap),
            None => {
                self.inner.captures.clear(tap);
                if let Some(store) = &self.inner.store {
                    history::history_clear(store, tap).await?;
                }
            }
        }
        if tap.is_none() {
            lock(&self.inner.restored).clear();
        }
        Ok(())
    }

    /// A tap's counters and latency percentiles.
    ///
    /// # Errors
    /// [`InspectError::UnknownTap`].
    pub fn metrics(&self, tap: &TapId) -> Result<MetricsSnapshot, InspectError> {
        self.running()
            .ok_or(InspectError::UnknownTap)?
            .metrics(tap)
            .map_err(|_| InspectError::UnknownTap)
    }

    /// Stops every tap and Lens itself, and writes what's left of the history.
    pub async fn shutdown(&self) {
        let lens = lock(&self.inner.lens).take();
        if let Some(lens) = lens {
            lens.shutdown().await;
        }
        self.inner.stop.cancel();
        let tasks: Vec<_> = lock(&self.inner.tasks).drain(..).collect();
        for task in tasks {
            let _ = task.await;
        }
        lock(&self.inner.taps).clear();
    }
}

/// Renders exchanges in `format`.
pub fn export(exchanges: &[Arc<Exchange>], format: ExportFormat, redaction: &Redaction) -> String {
    if format == ExportFormat::Har {
        let refs: Vec<&Exchange> = exchanges.iter().map(AsRef::as_ref).collect();
        return lens::export::har_string(&refs, redaction);
    }
    exchanges
        .iter()
        .map(|e| lens::export::export(format, e, redaction))
        .collect::<Vec<_>>()
        .join(if format == ExportFormat::Markdown {
            "\n---\n\n"
        } else {
            "\n\n"
        })
}

/// Follows Lens's events: notes activity for idle stops, and reports requests to
/// watched paths.
async fn watch(
    inner: Weak<Inner>,
    mut events: broadcast::Receiver<LensEvent>,
    stop: CancellationToken,
) {
    let mut tick = tokio::time::interval(IDLE_CHECK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            () = stop.cancelled() => return,
            event = events.recv() => {
                let Some(inner) = inner.upgrade() else { return };
                match event {
                    Ok(LensEvent::Exchange { change, exchange }) => {
                        observe(&inner, change, &exchange);
                    }
                    Ok(LensEvent::Cleared { .. })
                    | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
            _ = tick.tick() => {
                let Some(inner) = inner.upgrade() else { return };
                check_idle(&inner);
            }
        }
    }
}

fn observe(inner: &Inner, change: Change, exchange: &Exchange) {
    let global: Vec<String> = lock(&inner.settings).watched_paths.clone();
    let mut taps = lock(&inner.taps);
    let Some(entry) = taps.get_mut(&exchange.tap) else {
        return;
    };
    if exchange.replay_of.is_none() {
        entry.last_activity = tokio::time::Instant::now();
        entry.idle_reported = false;
    }
    if change != Change::Completed || exchange.replay_of.is_some() {
        return;
    }
    let path = exchange.request.path();
    let hit = entry
        .watched
        .iter()
        .chain(global.iter())
        .filter_map(|p| PathPattern::parse(p).ok())
        .any(|p| p.matches(path));
    if hit {
        let event = InspectEvent::Watched {
            tap: exchange.tap.clone(),
            scope: entry.scope.clone(),
            name: entry.name.clone(),
            exchange: exchange.id,
            method: exchange.request.method.to_string(),
            path: lens::mask_text(path, &Redaction::masked()).into_owned(),
        };
        drop(taps);
        let _ = inner.events.send(event);
    }
}

fn check_idle(inner: &Inner) {
    let mut idle = Vec::new();
    for (id, entry) in lock(&inner.taps).iter_mut() {
        if let Some(limit) = entry.idle_stop
            && !entry.idle_reported
            && entry.last_activity.elapsed() >= limit
        {
            entry.idle_reported = true;
            idle.push(InspectEvent::Idle {
                tap: id.clone(),
                scope: entry.scope.clone(),
                name: entry.name.clone(),
                minutes: u32::try_from(limit.as_secs() / 60).unwrap_or(u32::MAX),
            });
        }
    }
    for event in idle {
        let _ = inner.events.send(event);
    }
}

#[cfg(test)]
mod tests;
