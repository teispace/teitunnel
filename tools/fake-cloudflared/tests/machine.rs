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

struct WithServices {
    machine: MachineTunnels,
    supervisor: Supervisor,
    local: Local,
    services: Arc<teitunnel_core::service::ProcessServices>,
    tokens: std::path::PathBuf,
}

/// Connectors that can run as (child-process) services, on their own port range.
fn setup_with_services(dir: &Path, ports: std::ops::Range<u16>) -> WithServices {
    setup_with(
        dir,
        ports,
        teitunnel_core::service::ProcessServices::default(),
    )
}

fn setup_with(
    dir: &Path,
    ports: std::ops::Range<u16>,
    manager: teitunnel_core::service::ProcessServices,
) -> WithServices {
    use teitunnel_core::machine::ServicePaths;
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
    let local = Local::new(Store::open_in_memory().unwrap());
    let services = Arc::new(manager);
    let tokens = dir.join("tokens");
    let machine = MachineTunnels::new(
        supervisor.clone(),
        binary,
        PortAllocator::new(ports),
        Arc::new(MemoryStore::default()),
        local.clone(),
    )
    .with_services(
        services.clone(),
        ServicePaths {
            tokens: tokens.clone(),
            logs: dir.join("logs"),
        },
    );
    WithServices {
        machine,
        supervisor,
        local,
        services,
        tokens,
    }
}

fn unused_api() -> cf_api::Client {
    cf_api::Client::with_base("http://127.0.0.1:9", cf_api::ApiToken::new("x")).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn switches_to_always_on_and_back_without_a_gap() {
    let dir = tempfile::tempdir().unwrap();
    let WithServices {
        machine,
        supervisor,
        local,
        services,
        tokens,
    } = setup_with_services(dir.path(), 23010..23020);
    local.set_machine_tunnel("acc", "t2", "Mac").await.unwrap();
    machine
        .start("acc", "t2", Secret::new("run-token".into()))
        .await
        .unwrap();
    healthy(&supervisor, "t2").await;
    let unused = unused_api();

    // Session → Always-on: the service connects, then the app's connector stops.
    machine
        .set_always_on(&unused, "acc", None, true)
        .await
        .unwrap();
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
    // The service's output lands in its log file, which the app reads.
    let mut logged = false;
    for _ in 0..30 {
        if !machine.logs("t2", 50).is_empty() {
            logged = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(logged, "the Always-on connector's log is readable");
    // A second scrape makes the first interval; reading with `since` returns only newer.
    machine.sample_once().await;
    let traffic = machine.traffic("t2", None).expect("sampled");
    assert_eq!(traffic.series.len(), 1);
    assert_eq!(traffic.total_requests, 7);
    assert_eq!(traffic.connections, 1);
    assert_eq!(traffic.locations, ["ams01"]);
    let last = traffic.series.at[0];
    assert!(machine.traffic("t2", Some(last)).unwrap().series.is_empty());

    // Always-on → Session: the app's connector connects, then the service goes.
    machine
        .set_always_on(&unused, "acc", None, false)
        .await
        .unwrap();
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
    assert!(
        machine.traffic("t2", None).is_some(),
        "traffic history survives the mode switch"
    );
    machine.stop("t2").await.unwrap();
    assert!(machine.traffic("t2", None).is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn moves_connectors_onto_a_new_binary_without_a_gap() {
    let dir = tempfile::tempdir().unwrap();
    let WithServices {
        machine,
        supervisor,
        local,
        services,
        ..
    } = setup_with_services(dir.path(), 23020..23030);
    let api = unused_api();
    assert!(
        !machine
            .restart_on_current_binary(&api, "acc")
            .await
            .unwrap()
    );

    local.set_machine_tunnel("acc", "t3", "Mac").await.unwrap();
    machine
        .start("acc", "t3", Secret::new("run-token".into()))
        .await
        .unwrap();
    healthy(&supervisor, "t3").await;
    // Session: restarted in place, connected again.
    assert!(
        machine
            .restart_on_current_binary(&api, "acc")
            .await
            .unwrap()
    );
    healthy(&supervisor, "t3").await;

    // Always-on: a temporary app connector bridges while the service is reinstalled.
    machine
        .set_always_on(&api, "acc", None, true)
        .await
        .unwrap();
    services.calls.lock().unwrap().clear();
    assert!(
        machine
            .restart_on_current_binary(&api, "acc")
            .await
            .unwrap()
    );
    assert_eq!(
        *services.calls.lock().unwrap(),
        ["install com.teispace.teitunnel.connector.t3"],
        "reinstalled once, never uninstalled"
    );
    assert!(machine.is_always_on("t3"));
    assert!(
        supervisor.state(&connector_id("t3")).is_none(),
        "the bridge is gone"
    );
    machine.sample_once().await;
    assert!(matches!(
        machine.state("t3"),
        Some(ConnectorState::Healthy { .. })
    ));
    let port = local
        .machine_tunnel("acc")
        .await
        .unwrap()
        .unwrap()
        .metrics_port
        .unwrap();
    assert!(
        cloudflared::Endpoints::new(port)
            .unwrap()
            .ready()
            .await
            .is_ok_and(|r| r.ready_connections > 0),
        "the remembered port is the new service's"
    );
    machine.stop("t3").await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn a_manager_that_cant_capture_output_gets_a_self_logging_connector() {
    let dir = tempfile::tempdir().unwrap();
    let WithServices {
        machine,
        supervisor,
        local,
        services,
        ..
    } = setup_with(
        dir.path(),
        23030..23040,
        teitunnel_core::service::ProcessServices::self_logging(),
    );
    let api = unused_api();
    local.set_machine_tunnel("acc", "t4", "Mac").await.unwrap();
    machine
        .start("acc", "t4", Secret::new("run-token".into()))
        .await
        .unwrap();
    healthy(&supervisor, "t4").await;
    machine
        .set_always_on(&api, "acc", None, true)
        .await
        .unwrap();

    let spec = services.installed.lock().unwrap()[0].clone();
    let args: Vec<String> = spec
        .args
        .iter()
        .map(|a| a.to_string_lossy().into_owned())
        .collect();
    let dir_arg = args
        .iter()
        .position(|a| a == "--log-directory")
        .map(|i| args[i + 1].clone())
        .expect("the connector writes its own log");
    assert!(dir_arg.ends_with("t4"), "{dir_arg}");
    assert!(spec.log_file.ends_with("t4/cloudflared.log"));
    machine.stop("t4").await.unwrap();
}
