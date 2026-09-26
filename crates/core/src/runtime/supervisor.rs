use std::{
    collections::HashMap,
    sync::{Arc, Mutex, Weak},
    time::{Duration, SystemTime},
};

use cloudflared::{CommandSpec, LogEvent};
use tokio::{
    runtime::Handle,
    sync::{broadcast, watch},
    task::JoinHandle,
};

use super::{
    connector::{self, Context},
    logbuf::LogBuffer,
    network::{self, Disruption, Observer},
    policy::RestartPolicy,
    registry::PidRegistry,
    state::{ConnectorId, ConnectorState, RuntimeEvent},
};

use crate::text::{Text, UserText, english_display, msg};

const EVENT_CAPACITY: usize = 4096;
const DEFAULT_LOG_CAPACITY: usize = 100_000;

/// Everything needed to run one connector.
#[derive(Debug, Clone)]
pub struct ConnectorSpec {
    /// Stable identifier.
    pub id: ConnectorId,
    /// The cloudflared invocation.
    pub command: CommandSpec,
    /// The `--metrics` port in `command`, used for `/ready` polling.
    pub metrics_port: u16,
    /// Restart and timing policy.
    pub policy: RestartPolicy,
    /// How many log events to keep in memory.
    pub log_capacity: usize,
}

impl ConnectorSpec {
    /// A spec with the default policy and log capacity.
    pub fn new(id: ConnectorId, command: CommandSpec, metrics_port: u16) -> Self {
        Self {
            id,
            command,
            metrics_port,
            policy: RestartPolicy::default(),
            log_capacity: DEFAULT_LOG_CAPACITY,
        }
    }
}

/// Errors from supervisor operations.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum SupervisorError {
    /// A connector with this id is already running.
    AlreadyRunning(ConnectorId),
    /// No connector with this id.
    NotFound(ConnectorId),
}

impl UserText for SupervisorError {
    fn text(&self) -> Text {
        match self {
            Self::AlreadyRunning(_) => msg::error::connector::already_running(),
            Self::NotFound(_) => msg::error::connector::not_found(),
        }
    }
}

english_display!(SupervisorError);

type Connectors = Mutex<HashMap<ConnectorId, Handle_>>;

fn nudge_all(connectors: &Connectors) {
    for handle in connectors
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .values()
    {
        handle.nudge.send_modify(|n| *n = n.wrapping_add(1));
    }
}

fn disrupted(connectors: &Connectors, at: &Mutex<Option<u64>>, what: Disruption) {
    tracing::info!(?what, "reconnecting connectors");
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
    *at.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(now);
    nudge_all(connectors);
}

struct Handle_ {
    state: watch::Receiver<ConnectorState>,
    stop: watch::Sender<bool>,
    nudge: watch::Sender<u64>,
    logs: LogBuffer,
    task: JoinHandle<()>,
}

/// Owns every Session-mode connector in this app instance.
#[derive(Clone)]
pub struct Supervisor {
    connectors: Arc<Connectors>,
    events: broadcast::Sender<RuntimeEvent>,
    registry: PidRegistry,
    runtime: Handle,
    /// When the last wake or network change happened (ms since the epoch).
    disrupted_at: Arc<Mutex<Option<u64>>>,
    /// The network watch runs (started with the first connector).
    watching: Arc<std::sync::Once>,
}

impl std::fmt::Debug for Supervisor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor")
            .field("connectors", &self.ids())
            .finish_non_exhaustive()
    }
}

impl Supervisor {
    /// A supervisor spawning its actors on `runtime`.
    pub fn new(registry: PidRegistry, runtime: Handle) -> Self {
        let (events, _) = broadcast::channel(EVENT_CAPACITY);
        Self {
            connectors: Arc::default(),
            events,
            registry,
            runtime,
            disrupted_at: Arc::default(),
            watching: Arc::new(std::sync::Once::new()),
        }
    }

    /// Tells every connector the network may have changed under it: one waiting out a
    /// backoff or a crash loop starts at once, and a running one without a connection
    /// 15 seconds later is restarted. Harmless for healthy connectors.
    pub fn nudge(&self) {
        nudge_all(&self.connectors);
    }

    /// When this computer last woke or changed networks (ms since the epoch), so a
    /// slow reconnect right after isn't reported as an outage (see
    /// [`network::SETTLE`]).
    pub fn disrupted_at_ms(&self) -> Option<u64> {
        *self
            .disrupted_at
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Records a disruption and nudges every connector.
    pub fn disrupted(&self, what: Disruption) {
        disrupted(&self.connectors, &self.disrupted_at, what);
    }

    /// Watches for sleep and network changes while this supervisor lives.
    fn watch_network(&self) {
        let connectors: Weak<Connectors> = Arc::downgrade(&self.connectors);
        let disrupted_at = Arc::clone(&self.disrupted_at);
        self.runtime.spawn(async move {
            let mut observer = Observer::new(SystemTime::now());
            let mut tick = tokio::time::interval(network::TICK);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                // Ends with the last clone of the supervisor.
                let Some(connectors) = connectors.upgrade() else {
                    return;
                };
                if let Some(what) = observer.observe(SystemTime::now(), &network::addresses()) {
                    disrupted(&connectors, &disrupted_at, what);
                }
            }
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ConnectorId, Handle_>> {
        self.connectors
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Subscribes to state changes and log lines of all connectors.
    pub fn subscribe(&self) -> broadcast::Receiver<RuntimeEvent> {
        self.events.subscribe()
    }

    /// Starts a connector.
    ///
    /// # Errors
    /// [`SupervisorError::AlreadyRunning`] if the id is in use by a live connector.
    pub fn start(&self, spec: ConnectorSpec) -> Result<(), SupervisorError> {
        let mut connectors = self.lock();
        if connectors
            .get(&spec.id)
            .is_some_and(|h| !h.task.is_finished())
        {
            return Err(SupervisorError::AlreadyRunning(spec.id));
        }
        self.watching.call_once(|| self.watch_network());
        let (state_tx, state_rx) = watch::channel(ConnectorState::Stopped);
        let (stop_tx, stop_rx) = watch::channel(false);
        let (nudge_tx, nudge_rx) = watch::channel(0);
        let logs = LogBuffer::new(spec.log_capacity);
        let id = spec.id.clone();
        let task = self.runtime.spawn(connector::run(Context {
            spec,
            events: self.events.clone(),
            registry: self.registry.clone(),
            state: state_tx,
            logs: logs.clone(),
            stop: stop_rx,
            nudge: nudge_rx,
        }));
        connectors.insert(
            id,
            Handle_ {
                state: state_rx,
                stop: stop_tx,
                nudge: nudge_tx,
                logs,
                task,
            },
        );
        Ok(())
    }

    /// Stops a connector gracefully and forgets it.
    ///
    /// # Errors
    /// [`SupervisorError::NotFound`] if there is no such connector.
    pub async fn stop(&self, id: &ConnectorId) -> Result<(), SupervisorError> {
        let handle = self
            .lock()
            .remove(id)
            .ok_or_else(|| SupervisorError::NotFound(id.clone()))?;
        handle.stop.send_replace(true);
        let _ = handle.task.await;
        Ok(())
    }

    /// Stops every connector concurrently (app exit). Each gets its own SIGTERM grace
    /// period, so the total time is bounded by the slowest one, not the sum.
    pub async fn stop_all(&self) {
        let handles: Vec<Handle_> = self.lock().drain().map(|(_, handle)| handle).collect();
        for handle in &handles {
            handle.stop.send_replace(true);
        }
        for handle in handles {
            let _ = handle.task.await;
        }
    }

    /// Current state of a connector.
    pub fn state(&self, id: &ConnectorId) -> Option<ConnectorState> {
        self.lock()
            .get(id)
            .map(|handle| handle.state.borrow().clone())
    }

    /// Ids of all known connectors.
    pub fn ids(&self) -> Vec<ConnectorId> {
        let mut ids: Vec<_> = self.lock().keys().cloned().collect();
        ids.sort();
        ids
    }

    /// The newest `limit` log events of a connector.
    pub fn logs(&self, id: &ConnectorId, limit: usize) -> Option<Vec<Arc<LogEvent>>> {
        self.lock().get(id).map(|handle| handle.logs.tail(limit))
    }

    /// Waits until `id`'s state satisfies `predicate`, returning that state, or `None`
    /// on timeout or if the connector disappears.
    pub async fn wait_for(
        &self,
        id: &ConnectorId,
        timeout: Duration,
        predicate: impl Fn(&ConnectorState) -> bool,
    ) -> Option<ConnectorState> {
        let mut state = self.lock().get(id)?.state.clone();
        tokio::time::timeout(timeout, state.wait_for(|s| predicate(s)))
            .await
            .ok()?
            .ok()
            .map(|s| s.clone())
    }
}
