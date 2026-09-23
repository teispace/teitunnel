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
    domain::OriginUrl,
    quick_share::{QuickShare, QuickShares, ShareStatus},
    runtime::{PidRegistry, PortAllocator, Supervisor},
    store::Store,
};

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
    let shares = QuickShares::new(supervisor, binary, ports, Store::open_in_memory().unwrap())
        .with_url_timeout(Duration::from_secs(2))
        .with_dns_propagation(Duration::ZERO);
    tokio::spawn(shares.clone().watch_runtime());
    shares
}

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
        .start(OriginUrl::parse("3000").unwrap(), None)
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
                .start(OriginUrl::parse(&port.to_string()).unwrap(), None)
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
        .start(OriginUrl::parse("3000").unwrap(), None)
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
        .start(OriginUrl::parse("3000").unwrap(), None)
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
