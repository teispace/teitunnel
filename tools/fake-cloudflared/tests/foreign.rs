//! Discovery of cloudflared processes Teitunnel didn't start, and stopping one.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{os::unix::fs::PermissionsExt, time::Duration};

use teitunnel_core::discovery::cloudflared::{self, ForeignMode};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-cloudflared");

#[tokio::test(flavor = "multi_thread")]
async fn finds_a_foreign_quick_tunnel_and_stops_it() {
    // Named `cloudflared` and detached from this process, like one started in a terminal.
    let dir = tempfile::tempdir().unwrap();
    let binary = dir.path().join("cloudflared");
    std::fs::copy(FAKE, &binary).unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    let port = 22990;
    // Whatever happens, don't leave the detached process behind.
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::process::Command::new("pkill")
                .args(["-f", &self.0])
                .status();
        }
    }
    let _cleanup = Cleanup(format!("127.0.0.1:{port}"));
    std::process::Command::new("sh")
        .arg("-c")
        .arg(format!(
            "'{}' tunnel --url http://localhost:9 --metrics 127.0.0.1:{port} >/dev/null 2>&1 &",
            binary.display()
        ))
        .status()
        .unwrap();

    let mut found = None;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        found = cloudflared::foreign().await.into_iter().find(|c| {
            c.metrics.as_deref() == Some(&format!("127.0.0.1:{port}")) && c.connections == Some(1)
        });
        if found.is_some() {
            break;
        }
    }
    let connector = found.expect("the foreign cloudflared is discovered");
    assert_eq!(
        connector.mode,
        ForeignMode::QuickTunnel {
            origin: "http://localhost:9".into()
        }
    );
    assert_eq!(connector.connections, Some(1));

    assert!(cloudflared::stop(connector.pid).await);
    let gone = cloudflared::foreign()
        .await
        .iter()
        .all(|c| c.pid != connector.pid);
    assert!(gone, "stopped");
    assert!(
        !cloudflared::stop(connector.pid).await,
        "a pid that's gone isn't stopped again"
    );
}
