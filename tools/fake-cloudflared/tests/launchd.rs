//! The real launchd, end to end: install an agent that runs the fake cloudflared, see it
//! connect, remove it. Opt-in (`TEITUNNEL_TEST_LAUNCHD=1`) because it touches the user's
//! launchd domain; CI enables it on macOS.
#![cfg(target_os = "macos")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{os::unix::fs::PermissionsExt, time::Duration};

use cloudflared::{LogLevel, Protocol, RunCmd, TokenSource, launchd::LaunchAgent};
use teitunnel_core::service::{Launchd, ServiceManager};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-cloudflared");

#[tokio::test(flavor = "multi_thread")]
async fn installs_runs_and_removes_an_agent() {
    if std::env::var_os("TEITUNNEL_TEST_LAUNCHD").is_none() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("cloudflared");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nFAKE_CFD_SCENARIO=healthy exec '{FAKE}' \"$@\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let token = dir.path().join("token");
    std::fs::write(&token, "test-token").unwrap();
    let port = 23090;
    let command = RunCmd {
        token: TokenSource::File(token),
        metrics_port: port,
        protocol: Protocol::Auto,
        log_level: LogLevel::Info,
        log_dir: None,
    }
    .build(&wrapper);
    let id = format!("test-{}", std::process::id());
    let agent = LaunchAgent::new(&id, &command, dir.path().join("connector.log")).unwrap();
    let launchd = Launchd::for_current_user().unwrap();

    // Whatever happens, remove the agent.
    struct Cleanup(Launchd, String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let (launchd, label) = (self.0.clone(), self.1.clone());
            let _ = std::thread::spawn(move || {
                tokio::runtime::Runtime::new()
                    .unwrap()
                    .block_on(launchd.uninstall(&label))
            })
            .join();
        }
    }
    let _cleanup = Cleanup(launchd.clone(), agent.label.clone());

    launchd.install(&agent).await.unwrap();
    let endpoints = cloudflared::Endpoints::new(port).unwrap();
    let mut ready = false;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        if endpoints
            .ready()
            .await
            .is_ok_and(|r| r.ready_connections > 0)
        {
            ready = true;
            break;
        }
    }
    assert!(ready, "the agent's connector connected");
    let state = launchd.state(&agent.label).await;
    assert!(state.loaded && state.pid.is_some(), "{state:?}");

    launchd.uninstall(&agent.label).await.unwrap();
    assert!(!launchd.state(&agent.label).await.loaded);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        endpoints.ready().await.is_err(),
        "the connector stopped with the agent"
    );
}
