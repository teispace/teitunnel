//! Nightly: a real Quick Share through Cloudflare. Ignored by default (network, real
//! cloudflared); run with `cargo nextest run -p teitunnel-core --run-ignored only`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{io::Write, net::TcpListener, time::Duration};

use teitunnel_core::{
    binary::{BinaryManager, Locator},
    domain::OriginUrl,
    quick_share::{QuickShares, ShareStatus},
    runtime::{PidRegistry, PortAllocator, QUICK_SHARE_PORTS, Supervisor},
    store::Store,
};

/// A tiny HTTP origin answering every request with a known body.
fn origin() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            // Read the request head first; replying and closing without reading makes
            // the kernel reset the connection, which cloudflared reports as a 502.
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n")
                && std::io::Read::read(&mut stream, &mut byte).unwrap_or(0) == 1
            {
                head.push(byte[0]);
            }
            let body = "teitunnel nightly";
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    port
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "real network and cloudflared"]
async fn real_quick_share_serves_the_origin() {
    let dir = tempfile::tempdir().unwrap();
    let binary = BinaryManager::new(Locator::new(dir.path().join("bin"), None, vec![]));
    binary
        .install_latest(|_| {})
        .await
        .expect("install cloudflared");

    let supervisor = Supervisor::new(
        PidRegistry::new(dir.path().join("run")),
        tokio::runtime::Handle::current(),
    );
    let shares = QuickShares::new(
        supervisor,
        binary,
        PortAllocator::new(QUICK_SHARE_PORTS),
        Store::open_in_memory().unwrap(),
    );
    tokio::spawn(shares.clone().watch_runtime());

    let port = origin();
    let share = shares
        .start(OriginUrl::parse(&port.to_string()).unwrap(), None)
        .await
        .unwrap();

    let mut url = None;
    for _ in 0..120 {
        if let Some(s) = shares.list().into_iter().find(|s| s.id == share.id) {
            if s.status == ShareStatus::Live {
                url = s.url;
                break;
            }
            assert!(
                !matches!(s.status, ShareStatus::Failed { .. }),
                "{:?}",
                s.status
            );
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    let url = url.expect("share went live");

    // New trycloudflare hostnames can take a little while to resolve.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let mut body = String::new();
    for _ in 0..30 {
        match client.get(&url).send().await {
            Ok(response) if response.status().is_success() => {
                body = response.text().await.unwrap_or_default();
                break;
            }
            Ok(response) => eprintln!("attempt: HTTP {}", response.status()),
            Err(err) => eprintln!("attempt: {err:?}"),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    shares.stop_all().await;
    assert_eq!(body, "teitunnel nightly", "fetching {url}");
}
