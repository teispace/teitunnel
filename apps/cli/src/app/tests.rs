//! The CLI against a test app (a control server over a scripted host).

use teitunnel_control::{
    Decision, Limits,
    protocol::{AccountInfo, AppInfo, Status, TunnelInfo},
    testing::{self, share as live_share},
};

use super::*;

#[tokio::test]
async fn uses_the_app_only_when_it_runs_and_is_wanted() {
    let dir = tempfile::tempdir().unwrap();
    // Not running: automatic falls back to this terminal, `--app` fails.
    assert!(connect(dir.path(), Where::Auto).await.unwrap().is_none());
    let error = connect(dir.path(), Where::App).await.unwrap_err();
    assert!(error.starts_with("Teitunnel isn't running."), "{error}");

    let _app = testing::serve(dir.path(), Limits::default()).await.unwrap();
    let client = connect(dir.path(), Where::Auto).await.unwrap().unwrap();
    assert_eq!(client.hello().app.version, "9.9.9");
    assert!(connect(dir.path(), Where::App).await.unwrap().is_some());
    // `--here` never asks the app.
    assert!(connect(dir.path(), Where::Here).await.unwrap().is_none());
}

#[tokio::test]
async fn a_share_through_the_app_needs_its_approval() {
    let dir = tempfile::tempdir().unwrap();
    let app = testing::serve(dir.path(), Limits::default()).await.unwrap();
    let client = connect(dir.path(), Where::App).await.unwrap().unwrap();

    let declined = share(&client, "5173", None, false, false, &HostHeaderChoice::Off)
        .await
        .unwrap_err();
    assert_eq!(declined, "Not allowed in Teitunnel. Nothing changed.");
    assert!(app.host.started.lock().unwrap().is_empty());

    app.host.answer(Decision::Once);
    let started = share(
        &client,
        "5173",
        Some(Duration::from_secs(90)),
        false,
        false,
        &HostHeaderChoice::Set {
            value: "localhost:5173".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(started, ExitCode::SUCCESS);
    let request = app.host.started.lock().unwrap()[0].clone();
    assert_eq!(request.origin, "5173");
    assert_eq!(request.stop_after_seconds, Some(90));
    assert_eq!(
        request.host_header,
        HostHeader::Set {
            value: "localhost:5173".into()
        }
    );
    // The CLI introduces itself by name.
    let asked = app.host.asked.lock().unwrap()[0].clone();
    assert_eq!(
        asked.requester,
        teitunnel_control::Requester::Client(client_info())
    );
}

#[test]
fn describes_shares_and_status() {
    let mut s = live_share("quiet-river");
    s.requests = Some(12);
    s.expires_at = Some(10 * 60_000);
    assert_eq!(
        share_line(&s, 0),
        "https://quiet-river.trycloudflare.com\thttp://localhost:3000\tin the app, 12 requests, ends in 10 min"
    );
    let status = Status {
        app: AppInfo {
            name: "Teitunnel".into(),
            version: "0.2.0".into(),
        },
        accounts: vec![AccountInfo {
            id: "a1".into(),
            name: "Personal".into(),
        }],
        tunnels: vec![TunnelInfo {
            account_id: "a1".into(),
            id: "t1".into(),
            name: "mac-mini".into(),
            is_default: true,
            state: "healthy".into(),
        }],
        shares: Vec::new(),
    };
    assert_eq!(
        status_lines(&status, 0),
        [
            "Teitunnel 0.2.0 is running.",
            "Personal:",
            "  tunnel mac-mini: healthy, default",
            "No shares running.",
        ]
    );
}

#[tokio::test]
async fn status_says_when_the_app_isnt_running() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(status(dir.path(), true).await.unwrap(), ExitCode::SUCCESS);
}
