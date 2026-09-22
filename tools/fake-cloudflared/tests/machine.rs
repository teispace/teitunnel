//! This Mac's tunnel connector against the fake cloudflared: token handling, stable
//! metrics port, resume from the keychain.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{os::unix::fs::PermissionsExt, path::Path, sync::Arc, time::Duration};

use cloudflared::Locator;
use teitunnel_core::{
    Secret,
    binary::BinaryManager,
    engine::{Connectors, Local},
    machine::{MachineTunnels, connector_id},
    runtime::{ConnectorState, PidRegistry, PortAllocator, Supervisor},
    secrets::{MemoryStore, SecretStore},
    store::Store,
};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-cloudflared");

fn setup(dir: &Path) -> (MachineTunnels, Supervisor, MemoryStore, Local) {
    let wrapper = dir.join("cloudflared");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nFAKE_CFD_SCENARIO=healthy exec {FAKE} \"$@\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let binary = BinaryManager::new(Locator::new(dir.join("managed"), Some(wrapper), vec![]));
    let supervisor = Supervisor::new(
        PidRegistry::new(dir.join("run")),
        tokio::runtime::Handle::current(),
    );
    let secrets = MemoryStore::default();
    let local = Local::new(Store::open_in_memory().unwrap());
    let machine = MachineTunnels::new(
        supervisor.clone(),
        binary,
        PortAllocator::new(23000..23010),
        Arc::new(secrets.clone()),
        local.clone(),
    );
    (machine, supervisor, secrets, local)
}

async fn healthy(supervisor: &Supervisor, tunnel: &str) {
    let state = supervisor
        .wait_for(&connector_id(tunnel), Duration::from_secs(8), |s| {
            matches!(s, ConnectorState::Healthy { .. })
        })
        .await;
    assert!(
        state.is_some(),
        "not healthy: {:?}",
        supervisor.state(&connector_id(tunnel))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_the_machine_tunnel_with_its_token_in_the_keychain() {
    let dir = tempfile::tempdir().unwrap();
    let (machine, supervisor, secrets, local) = setup(dir.path());
    local.set_machine_tunnel("acc", "t1", "Mac").await.unwrap();

    machine
        .start("acc", "t1", Secret::new("run-token".into()))
        .await
        .unwrap();
    // The fake exits with status 3 if the token is in argv, so Healthy proves it isn't.
    healthy(&supervisor, "t1").await;
    assert!(machine.is_running("t1"));
    assert_eq!(
        secrets
            .get("tunnel:t1")
            .unwrap()
            .map(|s| s.expose().clone()),
        Some("run-token".to_owned())
    );
    let port = local
        .machine_tunnel("acc")
        .await
        .unwrap()
        .unwrap()
        .metrics_port;
    assert!(port.is_some_and(|p| (23000..23010).contains(&p)));

    // Stop, then resume from the keychain on the same port (no API call needed).
    machine.stop("t1").await.unwrap();
    assert!(!machine.is_running("t1"));
    let unused =
        cf_api::Client::with_base("http://127.0.0.1:9", cf_api::ApiToken::new("x")).unwrap();
    assert!(machine.resume(&unused, "acc").await.unwrap());
    healthy(&supervisor, "t1").await;
    assert_eq!(
        local
            .machine_tunnel("acc")
            .await
            .unwrap()
            .unwrap()
            .metrics_port,
        port,
        "the metrics port is stable"
    );

    // Signing out stops it and removes the token.
    machine.forget_account("acc").await;
    assert!(!machine.is_running("t1"));
    assert!(secrets.keys().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_to_resume_without_a_tunnel() {
    let dir = tempfile::tempdir().unwrap();
    let (machine, ..) = setup(dir.path());
    let unused =
        cf_api::Client::with_base("http://127.0.0.1:9", cf_api::ApiToken::new("x")).unwrap();
    assert!(!machine.resume(&unused, "acc").await.unwrap());
}

#[tokio::test(flavor = "multi_thread")]
async fn switches_to_always_on_and_back_without_a_gap() {
    use teitunnel_core::{
        machine::ServicePaths, runtime::PortAllocator as Ports, service::ProcessServices,
    };
    let dir = tempfile::tempdir().unwrap();
    let wrapper = dir.path().join("cloudflared");
    std::fs::write(
        &wrapper,
        format!("#!/bin/sh\nFAKE_CFD_SCENARIO=healthy exec {FAKE} \"$@\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let binary = BinaryManager::new(Locator::new(
        dir.path().join("managed"),
        Some(wrapper),
        vec![],
    ));
    let supervisor = Supervisor::new(
        PidRegistry::new(dir.path().join("run")),
        tokio::runtime::Handle::current(),
    );
    let secrets = MemoryStore::default();
    let local = Local::new(Store::open_in_memory().unwrap());
    let services = Arc::new(ProcessServices::default());
    let tokens = dir.path().join("tokens");
    let machine = MachineTunnels::new(
        supervisor.clone(),
        binary,
        Ports::new(23010..23020),
        Arc::new(secrets.clone()),
        local.clone(),
    )
    .with_services(
        services.clone(),
        ServicePaths {
            tokens: tokens.clone(),
            logs: dir.path().join("logs"),
        },
    );
    local.set_machine_tunnel("acc", "t2", "Mac").await.unwrap();
    machine
        .start("acc", "t2", Secret::new("run-token".into()))
        .await
        .unwrap();
    healthy(&supervisor, "t2").await;
    let unused =
        cf_api::Client::with_base("http://127.0.0.1:9", cf_api::ApiToken::new("x")).unwrap();

    // Session → Always-on: the service connects, then the app's connector stops.
    machine.set_always_on(&unused, "acc", true).await.unwrap();
    assert!(machine.is_always_on("t2"));
    assert!(
        supervisor.state(&connector_id("t2")).is_none(),
        "the app's connector stopped"
    );
    let token_file = tokens.join("t2");
    assert_eq!(std::fs::read_to_string(&token_file).unwrap(), "run-token");
    assert_eq!(
        std::fs::metadata(&token_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(
        local
            .machine_tunnel("acc")
            .await
            .unwrap()
            .unwrap()
            .always_on
    );
    machine.sample_once().await;
    assert!(matches!(
        machine.state("t2"),
        Some(ConnectorState::Healthy { .. })
    ));

    // Always-on → Session: the app's connector connects, then the service goes.
    machine.set_always_on(&unused, "acc", false).await.unwrap();
    assert!(!machine.is_always_on("t2"));
    healthy(&supervisor, "t2").await;
    assert!(
        !token_file.exists(),
        "the token file only exists while the service does"
    );
    assert!(
        !local
            .machine_tunnel("acc")
            .await
            .unwrap()
            .unwrap()
            .always_on
    );
    let calls = services.calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [
            "install com.teispace.teitunnel.connector.t2",
            "uninstall com.teispace.teitunnel.connector.t2"
        ]
    );
    machine.stop("t2").await.unwrap();
}
