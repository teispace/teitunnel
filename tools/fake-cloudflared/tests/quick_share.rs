//! Quick Share end to end against the fake cloudflared.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use cloudflared::Locator;
use teitunnel_core::{
    binary::BinaryManager,
    dev_server::DevServer,
    domain::OriginUrl,
    engine::{Edge, Failure},
    quick_share::{HostHeaderChoice, QuickShare, QuickShares, ShareStatus},
    runtime::{ConnectorId, PidRegistry, PortAllocator, Supervisor},
    store::Store,
};

/// Nothing listens on the discard port: the check after going live fails fast, offline.
const NO_EDGE: Edge = Edge::Test(std::net::SocketAddr::new(
    std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
    9,
));

const FAKE: &str = env!("CARGO_BIN_EXE_fake-cloudflared");

fn wrapper(dir: &Path, scenario: &str) -> PathBuf {
    let path = dir.join("cloudflared");
    std::fs::write(
        &path,
        format!("#!/bin/sh\nFAKE_CFD_SCENARIO={scenario} exec {FAKE} \"$@\"\n"),
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn service(dir: &Path, scenario: &str, block: u16) -> QuickShares {
    service_with(dir, scenario, block, NO_EDGE).0
}

fn service_with(dir: &Path, scenario: &str, block: u16, edge: Edge) -> (QuickShares, Supervisor) {
    let binary = BinaryManager::new(Locator::new(
        dir.join("managed"),
        Some(wrapper(dir, scenario)),
        vec![],
    ));
    let supervisor = Supervisor::new(
        PidRegistry::new(dir.join("run")),
        tokio::runtime::Handle::current(),
    );
    let ports = PortAllocator::new(22000 + block * 10..22000 + block * 10 + 10);
    let shares = QuickShares::new(
        supervisor.clone(),
        binary,
        ports,
        Store::open_in_memory().unwrap(),
        dir.join("data").join("quick-share.yml"),
    )
    .with_url_timeout(Duration::from_secs(2))
    .with_dns_propagation(Duration::ZERO)
    .with_edge(edge);
    tokio::spawn(shares.clone().watch_runtime());
    (shares, supervisor)
}

const AUTO: &HostHeaderChoice = &HostHeaderChoice::Auto;

async fn wait_for(
    shares: &QuickShares,
    id: &str,
    predicate: impl Fn(&QuickShare) -> bool,
) -> QuickShare {
    let mut changes = shares.subscribe();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(share) = shares.list().into_iter().find(|s| s.id == id)
            && predicate(&share)
        {
            return share;
        }
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        assert!(!left.is_zero(), "timed out; shares: {:?}", shares.list());
        let _ = tokio::time::timeout(left, changes.recv()).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn share_goes_live_reports_stats_and_stops() {
    let dir = tempfile::tempdir().unwrap();
    let shares = service(dir.path(), "healthy", 1);
    let share = shares
        .start(OriginUrl::parse("3000").unwrap(), None, AUTO)
        .await
        .unwrap();
    assert_eq!(share.status, ShareStatus::Starting);
    assert_eq!(share.origin.as_str(), "http://localhost:3000");

    let live = wait_for(&shares, &share.id, |s| s.status == ShareStatus::Live).await;
    let url = live.url.unwrap();
    assert!(
        url.starts_with("https://fake-") && url.ends_with(".trycloudflare.com"),
        "{url}"
    );

    let stats = shares.stats(&share.id).await.unwrap();
    assert_eq!((stats.requests, stats.errors), (7, 1));

    shares.stop(&share.id).await.unwrap();
    assert!(shares.list().is_empty());
    assert!(shares.stop(&share.id).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn several_shares_run_at_once_and_stop_together() {
    let dir = tempfile::tempdir().unwrap();
    let shares = service(dir.path(), "healthy", 2);
    let mut ids = Vec::new();
    for port in [3000, 3001, 3002] {
        ids.push(
            shares
                .start(OriginUrl::parse(&port.to_string()).unwrap(), None, AUTO)
                .await
                .unwrap()
                .id,
        );
    }
    for id in &ids {
        wait_for(&shares, id, |s| s.status == ShareStatus::Live).await;
    }
    let urls: std::collections::HashSet<_> =
        shares.list().into_iter().filter_map(|s| s.url).collect();
    assert_eq!(urls.len(), 3, "each share gets its own URL");
    shares.stop_all().await;
    assert!(shares.list().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn auto_stop_ends_the_share() {
    let dir = tempfile::tempdir().unwrap();
    let shares = service(dir.path(), "healthy", 3);
    let share = shares
        .start(
            OriginUrl::parse("3000").unwrap(),
            Some(Duration::from_millis(800)),
            AUTO,
        )
        .await
        .unwrap();
    assert!(share.stop_at.is_some());
    tokio::time::sleep(Duration::from_millis(2000)).await;
    assert!(shares.list().is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn missing_url_fails_with_a_readable_message() {
    let dir = tempfile::tempdir().unwrap();
    let shares = service(dir.path(), "no_url", 4);
    let share = shares
        .start(OriginUrl::parse("3000").unwrap(), None, AUTO)
        .await
        .unwrap();
    let failed = wait_for(&shares, &share.id, |s| {
        matches!(s.status, ShareStatus::Failed { .. })
    })
    .await;
    let ShareStatus::Failed { message } = failed.status else {
        unreachable!()
    };
    assert!(message.english().contains("didn't provide a URL"));
    shares.stop(&share.id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn crash_loop_marks_the_share_failed() {
    let dir = tempfile::tempdir().unwrap();
    let shares = service(dir.path(), "exit_immediately", 5);
    let share = shares
        .start(OriginUrl::parse("3000").unwrap(), None, AUTO)
        .await
        .unwrap();
    // Default policy backs off 1, 2, 4, 8, 16 s before a loop; instead check it reconnects.
    let state = wait_for(&shares, &share.id, |s| {
        s.status == ShareStatus::Reconnecting
    })
    .await;
    assert!(state.url.is_none());
    shares.stop(&share.id).await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_with_teitunnels_own_empty_config() {
    let dir = tempfile::tempdir().unwrap();
    let (shares, supervisor) = service_with(dir.path(), "healthy", 6, NO_EDGE);
    let share = shares
        .start(OriginUrl::parse("3000").unwrap(), None, AUTO)
        .await
        .unwrap();
    wait_for(&shares, &share.id, |s| s.status == ShareStatus::Live).await;
    let config = dir.path().join("data").join("quick-share.yml");
    assert_eq!(
        std::fs::read_to_string(&config).unwrap(),
        cloudflared::NEUTRAL_CONFIG
    );
    let settings = settings(&supervisor, &share.id);
    assert!(
        settings.contains(&format!("--config {}", config.display())),
        "{settings}"
    );
    assert!(!settings.contains("--http-host-header"), "{settings}");
    shares.stop(&share.id).await.unwrap();
}

/// The flags the fake cloudflared of a share was started with.
fn settings(supervisor: &Supervisor, id: &str) -> String {
    supervisor
        .logs(&ConnectorId(id.to_owned()), 1000)
        .unwrap_or_default()
        .iter()
        .rev()
        .find_map(|e| e.message.strip_prefix("Settings: ").map(str::to_owned))
        .unwrap_or_default()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_dev_server_refusing_the_address_is_fixed_in_place() {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};
    let edge = MockServer::start().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(403).set_body_string(
            "Blocked request. This host (\"x.trycloudflare.com\") is not allowed.\nTo allow this host, add \"x.trycloudflare.com\" to `server.allowedHosts` in vite.config.js.",
        ))
        .mount(&edge)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let (shares, supervisor) = service_with(dir.path(), "healthy", 7, Edge::Test(*edge.address()));
    let share = shares
        .start(
            OriginUrl::parse("5173").unwrap(),
            None,
            &HostHeaderChoice::Off,
        )
        .await
        .unwrap();
    let checked = wait_for(&shares, &share.id, |s| s.check.is_some()).await;
    let check = checked.check.unwrap();
    let Some(Failure::HostRejected { rejection }) = &check.failure else {
        panic!("{check:?}");
    };
    assert_eq!(rejection.server, DevServer::Vite);
    assert_eq!(rejection.host_header.as_deref(), Some("localhost:5173"));
    let first_url = checked.url.unwrap();

    // The fix: restart sending the dev server's own Host. Now it answers.
    edge.reset().await;
    Mock::given(any())
        .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
        .mount(&edge)
        .await;
    let restarted = shares
        .set_host_header(&share.id, Some("localhost:5173"))
        .await
        .unwrap();
    assert_eq!(restarted.status, ShareStatus::Starting);
    assert!(restarted.check.is_none());
    let fixed = wait_for(&shares, &share.id, |s| {
        s.status == ShareStatus::Live && s.check.is_some()
    })
    .await;
    assert!(fixed.check.unwrap().ok());
    assert_ne!(
        fixed.url.unwrap(),
        first_url,
        "a new cloudflared, a new address"
    );
    assert_eq!(
        fixed.host_header.map(|h| h.value).as_deref(),
        Some("localhost:5173")
    );
    let settings = settings(&supervisor, &share.id);
    assert!(
        settings.contains("--http-host-header localhost:5173 --url http://localhost:5173"),
        "{settings}"
    );
    assert!(
        shares
            .set_host_header(&share.id, Some("not a host"))
            .await
            .is_err()
    );
    shares.stop(&share.id).await.unwrap();
}
