//! Quick Shares through the inspector, with the fake cloudflared (built by a workspace
//! build; skipped without it): cloudflared points at Lens, requests reaching Lens are
//! captured, the Host header changes live without a new address, and turning
//! inspection off restarts the share straight to the service.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{path::PathBuf, time::Duration};

use teitunnel_core::{
    binary::{BinaryManager, Locator},
    domain::OriginUrl,
    engine::Edge,
    inspect::{ExchangeQuery, Inspector},
    quick_share::{HostHeaderChoice, QuickShare, QuickShares, ShareStatus},
    runtime::{ConnectorId, PidRegistry, PortAllocator, QUICK_SHARE_PORTS, Supervisor},
    store::Store,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn fake() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let path = exe.parent()?.parent()?.join("fake-cloudflared");
    path.exists().then_some(path)
}

/// Answers every request with `200 hello` and the Host it got in `x-host`.
async fn origin() -> u16 {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 8192];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let head = String::from_utf8_lossy(&buf[..n]).to_string();
                let host = head
                    .lines()
                    .find_map(|l| {
                        let (name, value) = l.split_once(':')?;
                        name.eq_ignore_ascii_case("host")
                            .then(|| value.trim().to_owned())
                    })
                    .unwrap_or_default();
                let reply = format!(
                    "HTTP/1.1 200 OK\r\nx-host: {host}\r\nContent-Length: 5\r\nConnection: close\r\n\r\nhello"
                );
                let _ = socket.write_all(reply.as_bytes()).await;
            });
        }
    });
    port
}

async fn live(shares: &QuickShares, id: &str) -> QuickShare {
    for _ in 0..300 {
        if let Some(share) = shares.list().into_iter().find(|s| s.id == id)
            && share.status == ShareStatus::Live
        {
            return share;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the share didn't go live");
}

/// The `--url` cloudflared was started with (the fake logs its arguments).
fn cloudflared_url(supervisor: &Supervisor, id: &str) -> Option<String> {
    let logs = supervisor.logs(&ConnectorId(id.to_owned()), 200)?;
    logs.iter().rev().find_map(|event| {
        let args = event.message.strip_prefix("Settings: ")?;
        let mut words = args.split_whitespace();
        words.find(|w| *w == "--url")?;
        words.next().map(str::to_owned)
    })
}

async fn through(url: &str) -> (u16, String) {
    let response = reqwest::Client::builder()
        .no_proxy()
        .build()
        .unwrap()
        .get(format!("{url}/hello"))
        .header("host", "fake.trycloudflare.com")
        .send()
        .await
        .unwrap();
    let status = response.status().as_u16();
    let host = response.headers()["x-host"].to_str().unwrap().to_owned();
    (status, host)
}

#[tokio::test(flavor = "multi_thread")]
async fn quick_shares_go_through_the_inspector() {
    let Some(fake) = fake() else {
        eprintln!("skipped: build the workspace to get fake-cloudflared");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let supervisor = Supervisor::new(
        PidRegistry::new(dir.path().join("run")),
        tokio::runtime::Handle::current(),
    );
    let inspector = Inspector::new(None, None, "app");
    let shares = QuickShares::new(
        supervisor.clone(),
        BinaryManager::new(Locator::new(dir.path().join("bin"), Some(fake), vec![])),
        PortAllocator::new(QUICK_SHARE_PORTS).spread(std::process::id()),
        Store::open_in_memory().unwrap(),
        dir.path().join("quick-share.yml"),
    )
    .with_edge(Edge::Test(([127, 0, 0, 1], 9).into()))
    .with_dns_propagation(Duration::ZERO)
    .with_inspector(inspector.clone());
    tokio::spawn(shares.clone().watch_runtime());

    let port = origin().await;
    let origin = OriginUrl::parse(&port.to_string()).unwrap();
    let share = shares
        .start(origin.clone(), None, &HostHeaderChoice::Off)
        .await
        .unwrap();
    assert!(share.inspected, "inspected by default");
    let share = live(&shares, &share.id).await;
    let tap = inspector.taps().pop().unwrap();
    assert_eq!(tap.id.as_str(), share.id);
    assert_eq!(tap.public_url, share.url);
    assert_eq!(
        cloudflared_url(&supervisor, &share.id).as_deref(),
        Some(tap.address.as_str()),
        "cloudflared points at the tap, not the service"
    );

    // What cloudflared would forward reaches the service and is captured.
    let (status, host) = through(&tap.address).await;
    assert_eq!((status, host.as_str()), (200, "fake.trycloudflare.com"));
    for _ in 0..100 {
        if !inspector.list(&ExchangeQuery::default()).items.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(inspector.list(&ExchangeQuery::default()).items.len(), 1);

    // The Host header fix applies at once, keeping the address.
    let url = share.url.clone();
    let updated = shares
        .set_host_header(&share.id, Some("localhost:5173"))
        .await
        .unwrap();
    assert_eq!(updated.url, url, "same address");
    let (_, host) = through(&tap.address).await;
    assert_eq!(host, "localhost:5173");

    // Turning inspection off restarts cloudflared straight to the service.
    let direct = shares.set_inspected(&share.id, false).await.unwrap();
    assert!(!direct.inspected);
    assert!(inspector.taps().is_empty());
    let direct = live(&shares, &share.id).await;
    let target = cloudflared_url(&supervisor, &direct.id).unwrap();
    assert!(target.ends_with(&format!(":{port}")), "{target}");
    assert!(!direct.inspected);

    // And back on.
    let again = shares.set_inspected(&share.id, true).await.unwrap();
    assert!(again.inspected);
    assert_eq!(inspector.taps().len(), 1);

    // An explicit choice beats the setting.
    let plain = shares
        .start_with(origin, None, &HostHeaderChoice::Off, Some(false))
        .await
        .unwrap();
    assert!(!plain.inspected);

    shares.stop_all().await;
    assert!(
        inspector.taps().is_empty(),
        "stopping a share stops its tap"
    );
    inspector.shutdown().await;
}

#[tokio::test]
async fn idle_shares_stop() {
    let Some(fake) = fake() else {
        eprintln!("skipped: build the workspace to get fake-cloudflared");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let supervisor = Supervisor::new(
        PidRegistry::new(dir.path().join("run")),
        tokio::runtime::Handle::current(),
    );
    let inspector = Inspector::new(None, None, "app");
    let shares = QuickShares::new(
        supervisor,
        BinaryManager::new(Locator::new(dir.path().join("bin"), Some(fake), vec![])),
        PortAllocator::new(QUICK_SHARE_PORTS).spread(std::process::id() + 7),
        Store::open_in_memory().unwrap(),
        dir.path().join("quick-share.yml"),
    )
    .with_edge(Edge::Test(([127, 0, 0, 1], 9).into()))
    .with_inspector(inspector.clone());
    tokio::spawn(shares.clone().watch_idle());
    let port = origin().await;
    let share = shares
        .start(
            OriginUrl::parse(&port.to_string()).unwrap(),
            None,
            &HostHeaderChoice::Off,
        )
        .await
        .unwrap();
    let tap = inspector.taps().pop().unwrap();
    inspector
        .configure(
            &tap.id,
            &teitunnel_core::inspect::TapPatch {
                idle_stop_minutes: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    tokio::time::pause();
    tokio::time::sleep(Duration::from_secs(45)).await;
    assert!(
        shares.list().iter().any(|s| s.id == share.id),
        "not idle yet"
    );
    tokio::time::sleep(Duration::from_secs(60)).await;
    for _ in 0..50 {
        if shares.list().is_empty() && inspector.taps().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(shares.list().is_empty(), "the idle share stopped");
    assert!(inspector.taps().is_empty());
    inspector.shutdown().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn folders_are_shared_through_the_inspector() {
    let Some(fake) = fake() else {
        eprintln!("skipped: build the workspace to get fake-cloudflared");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let site = dir.path().join("site");
    std::fs::create_dir_all(site.join(".git")).unwrap();
    std::fs::write(site.join("index.html"), "<h1>home</h1>").unwrap();
    std::fs::write(site.join(".env"), "SECRET=1").unwrap();
    std::fs::write(site.join(".git/config"), "[core]").unwrap();
    let supervisor = Supervisor::new(
        PidRegistry::new(dir.path().join("run")),
        tokio::runtime::Handle::current(),
    );
    let inspector = Inspector::new(None, None, "app");
    let shares = QuickShares::new(
        supervisor.clone(),
        BinaryManager::new(Locator::new(dir.path().join("bin"), Some(fake), vec![])),
        PortAllocator::new(QUICK_SHARE_PORTS).spread(std::process::id()),
        Store::open_in_memory().unwrap(),
        dir.path().join("quick-share.yml"),
    )
    .with_edge(Edge::Test(([127, 0, 0, 1], 9).into()))
    .with_dns_propagation(Duration::ZERO)
    .with_inspector(inspector.clone());
    tokio::spawn(shares.clone().watch_runtime());

    let folder =
        teitunnel_core::folder_share::FolderShare::resolve(site.to_str().unwrap(), false, true)
            .unwrap();
    let share = shares.start_folder(folder.clone(), None).await.unwrap();
    assert_eq!(share.folder.as_ref(), Some(&folder));
    let share = live(&shares, &share.id).await;
    let tap = inspector.taps().pop().unwrap();
    assert_eq!(share.origin.as_str(), tap.address);
    assert_eq!(
        cloudflared_url(&supervisor, &share.id).as_deref(),
        Some(tap.address.as_str())
    );
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let get = |path: &str| {
        let request = client
            .get(format!("{}{path}", tap.address))
            .header("accept", "text/html");
        async move {
            let response = request.send().await.unwrap();
            (response.status().as_u16(), response.text().await.unwrap())
        }
    };
    assert_eq!(get("/").await, (200, "<h1>home</h1>".into()));
    assert_eq!(get("/orders/7").await.0, 200, "single-page app fallback");
    assert_eq!(get("/.env").await.0, 404);
    assert_eq!(get("/.git/config").await.0, 404);
    // The client resolves the dots; whatever arrives never leaves the folder (here the
    // single-page app's index answers).
    let (_, body) = get("/%2e%2e/%2e%2e/etc/passwd").await;
    assert!(!body.contains("root:"));
    // The inspector can't be turned off for a folder.
    assert!(shares.set_inspected(&share.id, false).await.is_err());
    shares.stop(&share.id).await.unwrap();
    assert!(inspector.taps().is_empty());
    inspector.shutdown().await;
}
