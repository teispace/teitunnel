//! Supervisor integration tests against the fake cloudflared binary (real processes,
//! real signals, real HTTP polling).
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use cloudflared::{QuickTunnelCmd, RunCmd, TokenSource, TunnelToken};
use teitunnel_core::runtime::{
    ConnectorId, ConnectorSpec, ConnectorState, PidRegistry, PortAllocator, RestartPolicy,
    Supervisor,
};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-cloudflared");

struct Harness {
    supervisor: Supervisor,
    run_dir: tempfile::TempDir,
    ports: PortAllocator,
}

impl Harness {
    /// Tests run in parallel processes, so each gets its own block of ports.
    fn new(block: u16) -> Self {
        let run_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(run_dir.path().join("config")).unwrap();
        std::fs::write(
            run_dir.path().join("config/quick-share.yml"),
            cloudflared::NEUTRAL_CONFIG,
        )
        .unwrap();
        let registry = PidRegistry::new(run_dir.path().to_path_buf());
        Self {
            supervisor: Supervisor::new(registry, tokio::runtime::Handle::current()),
            run_dir,
            ports: PortAllocator::new(21000 + block * 10..21000 + block * 10 + 10),
        }
    }

    /// A Quick Share connector running the fake with `scenario`.
    fn spec(&self, id: &str, scenario: &str, policy: RestartPolicy) -> ConnectorSpec {
        let port = self.ports.allocate().unwrap();
        let command = QuickTunnelCmd {
            origin: "http://localhost:3000".into(),
            metrics_port: port,
            config: self.run_dir.path().join("config/quick-share.yml"),
            host_header: None,
        }
        .build(Path::new(&wrapper(self.run_dir.path(), scenario)));
        ConnectorSpec {
            policy,
            log_capacity: 1000,
            ..ConnectorSpec::new(ConnectorId(id.into()), command, port)
        }
    }

    fn pid(&self, id: &str) -> Option<u32> {
        let raw = std::fs::read(self.run_dir.path().join(format!("{id}.json"))).ok()?;
        let value: serde_json::Value = serde_json::from_slice(&raw).ok()?;
        value["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
    }
}

/// The scenario is passed through the environment of a tiny wrapper script, because
/// `CommandSpec` deliberately controls the child's environment.
///
/// Written once per scenario, atomically (a temporary file renamed into place): on
/// Linux, writing a script that another connector is executing fails with "Text file
/// busy".
fn wrapper(dir: &Path, scenario: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = dir.join(format!("fake-{}.sh", scenario.replace(':', "_")));
    if path.exists() {
        return path;
    }
    let staging = dir.join(format!(
        ".fake-{}-{}.sh",
        scenario.replace(':', "_"),
        std::process::id()
    ));
    std::fs::write(
        &staging,
        format!("#!/bin/sh\nFAKE_CFD_SCENARIO={scenario} exec {FAKE} \"$@\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::rename(&staging, &path).unwrap();
    path
}

fn alive(pid: u32) -> bool {
    let mut system = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(pid);
    system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    system
        .process(pid)
        .is_some_and(|p| p.status() != sysinfo::ProcessStatus::Zombie)
}

fn fast() -> RestartPolicy {
    RestartPolicy {
        initial_backoff: Duration::from_millis(50),
        max_backoff: Duration::from_millis(200),
        crash_loop_threshold: 3,
        crash_window: Duration::from_secs(60),
        connect_timeout: Duration::from_secs(1),
        health_interval: Duration::from_millis(200),
        stop_grace: Duration::from_millis(600),
        nudge_check: Some(Duration::from_millis(500)),
        crash_loop_retry: Duration::from_secs(600),
    }
}

fn is_healthy(state: &ConnectorState) -> bool {
    matches!(state, ConnectorState::Healthy { .. })
}

#[tokio::test(flavor = "multi_thread")]
async fn healthy_connector_starts_logs_and_stops_cleanly() {
    let h = Harness::new(1);
    let id = ConnectorId("qs-healthy".into());
    h.supervisor
        .start(h.spec(&id.0, "healthy", fast()))
        .unwrap();

    let state = h
        .supervisor
        .wait_for(&id, Duration::from_secs(5), is_healthy)
        .await;
    assert_eq!(state, Some(ConnectorState::Healthy { connections: 1 }));
    let pid = h.pid(&id.0).expect("pidfile written");
    assert!(alive(pid));
    let logs = h.supervisor.logs(&id, 100).unwrap();
    assert!(
        logs.iter()
            .any(|e| e.message == "Registered tunnel connection")
    );

    h.supervisor.stop(&id).await.unwrap();
    assert!(!alive(pid), "process is gone after stop");
    assert!(h.pid(&id.0).is_none(), "pidfile removed");
    assert!(h.supervisor.state(&id).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn slow_start_is_degraded_then_healthy() {
    let h = Harness::new(2);
    let id = ConnectorId("qs-slow".into());
    h.supervisor
        .start(h.spec(&id.0, "slow_start", fast()))
        .unwrap();
    let degraded = h
        .supervisor
        .wait_for(&id, Duration::from_secs(3), |s| {
            *s == ConnectorState::Degraded
        })
        .await;
    assert_eq!(degraded, Some(ConnectorState::Degraded));
    assert!(
        h.supervisor
            .wait_for(&id, Duration::from_secs(5), is_healthy)
            .await
            .is_some()
    );
    h.supervisor.stop(&id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn crashed_connector_restarts_with_backoff() {
    let h = Harness::new(3);
    let id = ConnectorId("qs-crash".into());
    h.supervisor
        .start(h.spec(&id.0, "crash_after:600", fast()))
        .unwrap();
    let crashed = h
        .supervisor
        .wait_for(&id, Duration::from_secs(5), |s| {
            matches!(s, ConnectorState::Crashed { .. })
        })
        .await;
    let Some(ConnectorState::Crashed {
        attempt, exit_code, ..
    }) = crashed
    else {
        panic!("expected a crash, got {crashed:?}");
    };
    assert_eq!((attempt, exit_code), (1, Some(1)));
    // It comes back by itself.
    assert!(
        h.supervisor
            .wait_for(&id, Duration::from_secs(5), is_healthy)
            .await
            .is_some()
    );
    h.supervisor.stop(&id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_loop_waits_instead_of_retrying() {
    let h = Harness::new(4);
    let id = ConnectorId("qs-loop".into());
    h.supervisor
        .start(h.spec(&id.0, "exit_immediately", fast()))
        .unwrap();
    let state = h
        .supervisor
        .wait_for(&id, Duration::from_secs(10), |s| {
            matches!(s, ConnectorState::CrashLoop { .. })
        })
        .await;
    assert_eq!(
        state,
        Some(ConnectorState::CrashLoop { exit_code: Some(1) })
    );
    let logs = h.supervisor.logs(&id, 100).unwrap();
    assert!(logs.iter().any(|e| e.message.contains("simulated")));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_crash_loop_tries_again_when_the_network_changes() {
    let h = Harness::new(10);
    let id = ConnectorId("qs-offline".into());
    h.supervisor
        .start(h.spec(&id.0, "exit_immediately", fast()))
        .unwrap();
    let looping = |s: &ConnectorState| matches!(s, ConnectorState::CrashLoop { .. });
    assert!(
        h.supervisor
            .wait_for(&id, Duration::from_secs(10), looping)
            .await
            .is_some()
    );
    // Still looping a while later: it waits instead of spinning.
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert!(looping(&h.supervisor.state(&id).unwrap()));
    let before = h.supervisor.disrupted_at_ms();
    h.supervisor
        .disrupted(teitunnel_core::runtime::network::Disruption::Online);
    assert!(h.supervisor.disrupted_at_ms() > before);
    assert!(
        h.supervisor
            .wait_for(&id, Duration::from_secs(5), |s| !looping(s))
            .await
            .is_some(),
        "started again"
    );
    h.supervisor.stop(&id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_nudge_restarts_a_connector_left_without_connections() {
    let h = Harness::new(11);
    let (stuck, fine) = (
        ConnectorId("qs-stuck".into()),
        ConnectorId("qs-fine".into()),
    );
    h.supervisor
        .start(h.spec(&stuck.0, "degraded", fast()))
        .unwrap();
    h.supervisor
        .start(h.spec(&fine.0, "healthy", fast()))
        .unwrap();
    // A Quick Share would get a new address: never restarted while it runs.
    let share = ConnectorId("qs-share".into());
    h.supervisor
        .start(h.spec(
            &share.0,
            "degraded",
            RestartPolicy {
                nudge_check: None,
                ..fast()
            },
        ))
        .unwrap();
    assert!(
        h.supervisor
            .wait_for(&stuck, Duration::from_secs(5), |s| *s
                == ConnectorState::Degraded)
            .await
            .is_some()
    );
    assert!(
        h.supervisor
            .wait_for(&fine, Duration::from_secs(5), is_healthy)
            .await
            .is_some()
    );
    assert!(
        h.supervisor
            .wait_for(&share, Duration::from_secs(5), |s| *s
                == ConnectorState::Degraded)
            .await
            .is_some()
    );
    let (stuck_pid, fine_pid) = (h.pid(&stuck.0).unwrap(), h.pid(&fine.0).unwrap());
    let share_pid = h.pid(&share.0).unwrap();
    h.supervisor.nudge();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.pid(&stuck.0) == Some(stuck_pid) && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_ne!(h.pid(&stuck.0), Some(stuck_pid), "restarted");
    assert!(!alive(stuck_pid));
    assert_eq!(
        h.pid(&fine.0),
        Some(fine_pid),
        "a healthy one is left alone"
    );
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(
        h.pid(&share.0),
        Some(share_pid),
        "a Quick Share is left to reconnect"
    );
    h.supervisor.stop_all().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sigterm_ignored_is_escalated_to_sigkill() {
    let h = Harness::new(5);
    let id = ConnectorId("qs-stubborn".into());
    h.supervisor
        .start(h.spec(&id.0, "ignore_sigterm", fast()))
        .unwrap();
    assert!(
        h.supervisor
            .wait_for(&id, Duration::from_secs(5), is_healthy)
            .await
            .is_some()
    );
    let pid = h.pid(&id.0).unwrap();

    let started = Instant::now();
    h.supervisor.stop(&id).await.unwrap();
    let took = started.elapsed();
    assert!(!alive(pid));
    assert!(
        took >= Duration::from_millis(500),
        "waited for the grace period: {took:?}"
    );
    assert!(took < Duration::from_secs(3), "then killed: {took:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn stop_all_runs_concurrently() {
    let h = Harness::new(6);
    let ids: Vec<_> = (0..3)
        .map(|i| ConnectorId(format!("qs-many-{i}")))
        .collect();
    for id in &ids {
        h.supervisor
            .start(h.spec(&id.0, "ignore_sigterm", fast()))
            .unwrap();
    }
    for id in &ids {
        assert!(
            h.supervisor
                .wait_for(id, Duration::from_secs(5), is_healthy)
                .await
                .is_some()
        );
    }
    let pids: Vec<_> = ids.iter().map(|id| h.pid(&id.0).unwrap()).collect();
    let started = Instant::now();
    h.supervisor.stop_all().await;
    assert!(
        started.elapsed() < Duration::from_millis(1800),
        "grace periods overlap"
    );
    assert!(pids.iter().all(|pid| !alive(*pid)));
    assert!(h.supervisor.ids().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn run_token_travels_in_the_environment() {
    let h = Harness::new(7);
    let port = h.ports.allocate().unwrap();
    let command = RunCmd {
        token: TokenSource::Env(TunnelToken::new("eyJ-test-token".into())),
        metrics_port: port,
        protocol: cloudflared::Protocol::Auto,
        log_level: cloudflared::LogLevel::Info,
        log_dir: None,
    }
    .build(Path::new(FAKE));
    let id = ConnectorId("tunnel-env".into());
    h.supervisor
        .start(ConnectorSpec {
            policy: fast(),
            ..ConnectorSpec::new(id.clone(), command, port)
        })
        .unwrap();
    // The fake exits with status 3 if the token is in argv, and 2 if it's missing.
    assert!(
        h.supervisor
            .wait_for(&id, Duration::from_secs(5), is_healthy)
            .await
            .is_some()
    );
    h.supervisor.stop(&id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn orphans_from_a_previous_run_are_reaped() {
    let h = Harness::new(8);
    let id = ConnectorId("qs-orphan".into());
    h.supervisor
        .start(h.spec(&id.0, "healthy", fast()))
        .unwrap();
    assert!(
        h.supervisor
            .wait_for(&id, Duration::from_secs(5), is_healthy)
            .await
            .is_some()
    );
    let pid = h.pid(&id.0).unwrap();

    // Simulate a force-quit: the supervisor is gone without stopping its children.
    std::mem::forget(h.supervisor);
    let registry = PidRegistry::new(h.run_dir.path().to_path_buf());
    let reaped = registry.reap_orphans().await;
    assert_eq!(reaped, [pid]);
    assert!(!alive(pid));
}
