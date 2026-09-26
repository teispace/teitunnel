//! One connector's actor: spawns cloudflared, reads its logs, polls `/ready`, restarts
//! it after crashes and stops it gracefully.

use std::{
    process::ExitStatus,
    sync::Arc,
    time::{Duration, Instant},
};

use cloudflared::{Endpoints, LogEvent, parse_line};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Child,
    sync::{broadcast, watch},
    task::JoinHandle,
};

use super::{
    logbuf::LogBuffer,
    policy::CrashTracker,
    registry::PidRegistry,
    state::{ConnectorState, RuntimeEvent},
    supervisor::ConnectorSpec,
};

/// How often `/ready` is polled until the first connection is up.
const CONNECTING_POLL: Duration = Duration::from_millis(250);

pub(super) struct Context {
    pub spec: ConnectorSpec,
    pub events: broadcast::Sender<RuntimeEvent>,
    pub registry: PidRegistry,
    pub state: watch::Sender<ConnectorState>,
    pub logs: LogBuffer,
    pub stop: watch::Receiver<bool>,
    /// Bumped on every nudge (see [`super::Supervisor::nudge`]).
    pub nudge: watch::Receiver<u64>,
}

enum Outcome {
    Exited(Option<ExitStatus>),
    StopRequested,
    /// Still without a connection after a nudge: start it again now.
    Restart,
}

impl Context {
    fn set(&self, state: ConnectorState) {
        if *self.state.borrow() != state {
            tracing::debug!(connector = %self.spec.id, ?state, "connector state");
            self.state.send_replace(state.clone());
            let _ = self.events.send(RuntimeEvent::State {
                id: self.spec.id.clone(),
                state,
            });
        }
    }

    fn log(&self, event: LogEvent) {
        let event = Arc::new(event);
        self.logs.push(Arc::clone(&event));
        let _ = self.events.send(RuntimeEvent::Log {
            id: self.spec.id.clone(),
            event,
        });
    }

    fn stop_requested(&self) -> bool {
        *self.stop.borrow()
    }
}

/// Runs the connector until it is stopped or enters a crash loop.
pub(super) async fn run(ctx: Context) {
    let mut stop = ctx.stop.clone();
    let mut nudge = ctx.nudge.clone();
    let mut crashes = CrashTracker::default();
    let policy = ctx.spec.policy;
    loop {
        if ctx.stop_requested() {
            ctx.set(ConnectorState::Stopped);
            return;
        }
        ctx.set(ConnectorState::Starting);
        let outcome = match ctx.spec.command.to_command().spawn() {
            Ok(child) => supervise(&ctx, child).await,
            Err(err) => {
                ctx.log(parse_line(&format!("failed to start cloudflared: {err}")));
                Outcome::Exited(None)
            }
        };
        let exit_code = match outcome {
            Outcome::StopRequested => {
                ctx.set(ConnectorState::Stopped);
                return;
            }
            Outcome::Restart => {
                tracing::info!(connector = %ctx.spec.id, "no connection after a wake or network change; restarting");
                continue;
            }
            Outcome::Exited(status) => status.and_then(|s| s.code()),
        };

        let (attempt, crash_loop) = crashes.record(Instant::now(), &policy);
        if crash_loop {
            tracing::warn!(connector = %ctx.spec.id, "crash loop; waiting for the network to change");
            ctx.set(ConnectorState::CrashLoop { exit_code });
            nudge.mark_unchanged();
            tokio::select! {
                () = tokio::time::sleep(policy.crash_loop_retry) => {}
                () = nudged(&mut nudge) => {}
                () = stopped(&mut stop) => {
                    ctx.set(ConnectorState::Stopped);
                    return;
                }
            }
            crashes.reset();
            continue;
        }
        let delay = policy.backoff(attempt);
        ctx.set(ConnectorState::Crashed {
            attempt,
            retry_in_ms: u64::try_from(delay.as_millis()).unwrap_or(u64::MAX),
            exit_code,
        });
        nudge.mark_unchanged();
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            // Back online or awake: no reason to wait out the backoff.
            () = nudged(&mut nudge) => crashes.reset(),
            () = stopped(&mut stop) => {
                ctx.set(ConnectorState::Stopped);
                return;
            }
        }
    }
}

/// Watches one child process until it exits or a stop is requested.
async fn supervise(ctx: &Context, mut child: Child) -> Outcome {
    let mut stop = ctx.stop.clone();
    let mut nudge = ctx.nudge.clone();
    nudge.mark_unchanged();
    // When a nudge's check is due.
    let mut check_at: Option<Instant> = None;
    let pid = child.id();
    if let Some(pid) = pid {
        ctx.registry.record(&ctx.spec.id, pid);
    }
    let readers = spawn_readers(ctx, &mut child);
    ctx.set(ConnectorState::Connecting);

    let endpoints = Endpoints::new(ctx.spec.metrics_port).ok();
    let started = Instant::now();
    let mut had_connection = false;
    let outcome = loop {
        let poll = if had_connection {
            ctx.spec.policy.health_interval
        } else {
            CONNECTING_POLL
        };
        tokio::select! {
            status = child.wait() => break Outcome::Exited(status.ok()),
            () = stopped(&mut stop) => break Outcome::StopRequested,
            () = nudged(&mut nudge) => {
                if let Some(check) = ctx.spec.policy.nudge_check {
                    check_at.get_or_insert_with(|| Instant::now() + check);
                }
            }
            () = tokio::time::sleep(poll) => {
                let connections = match &endpoints {
                    Some(endpoints) => endpoints.ready().await.map_or(0, |ready| ready.ready_connections),
                    None => 0,
                };
                if check_at.is_some_and(|at| Instant::now() >= at) {
                    check_at = None;
                    if connections == 0 {
                        break Outcome::Restart;
                    }
                }
                if connections > 0 {
                    had_connection = true;
                    ctx.set(ConnectorState::Healthy { connections });
                } else if had_connection || started.elapsed() > ctx.spec.policy.connect_timeout {
                    ctx.set(ConnectorState::Degraded);
                }
            }
        }
    };

    if matches!(outcome, Outcome::StopRequested | Outcome::Restart) {
        ctx.set(ConnectorState::Stopping);
        stop_child(&mut child, pid, ctx.spec.policy.stop_grace).await;
    }
    ctx.registry.remove(&ctx.spec.id);
    // Let the readers drain what the process wrote before exiting (bounded).
    for reader in readers {
        let _ = tokio::time::timeout(Duration::from_millis(500), reader).await;
    }
    outcome
}

/// Resolves once a stop has been requested. The `watch::Ref` is dropped inside, so it
/// never lives across an await point of the caller.
async fn stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|stop| *stop).await;
}

/// Resolves on the next nudge; never, once the supervisor has forgotten the connector.
async fn nudged(nudge: &mut watch::Receiver<u64>) {
    if nudge.changed().await.is_err() {
        std::future::pending::<()>().await;
    }
}

/// SIGTERM the process group, then SIGKILL after `grace`.
async fn stop_child(child: &mut Child, pid: Option<u32>, grace: Duration) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        super::signal::terminate_group(pid);
        if tokio::time::timeout(grace, child.wait()).await.is_ok() {
            return;
        }
        tracing::warn!(pid, "connector ignored SIGTERM; killing");
        super::signal::kill_group(pid);
    }
    #[cfg(not(unix))]
    let _ = (pid, grace);
    let _ = child.start_kill();
    let _ = child.wait().await;
}

fn spawn_readers(ctx: &Context, child: &mut Child) -> Vec<JoinHandle<()>> {
    let mut handles = Vec::with_capacity(2);
    if let Some(stderr) = child.stderr.take() {
        handles.push(spawn_reader(ctx, stderr));
    }
    if let Some(stdout) = child.stdout.take() {
        handles.push(spawn_reader(ctx, stdout));
    }
    handles
}

fn spawn_reader(ctx: &Context, stream: impl AsyncRead + Unpin + Send + 'static) -> JoinHandle<()> {
    let id = ctx.spec.id.clone();
    let events = ctx.events.clone();
    let logs = ctx.logs.clone();
    tokio::spawn(async move {
        let mut lines = BufReader::new(stream).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let event = Arc::new(parse_line(&line));
            logs.push(Arc::clone(&event));
            let _ = events.send(RuntimeEvent::Log {
                id: id.clone(),
                event,
            });
        }
    })
}
