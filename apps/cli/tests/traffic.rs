//! `teitunnel traffic` against captures another process wrote: an inspector in this
//! test captures requests into an isolated data folder's history, then the CLI lists,
//! shows, replays, exports and clears them.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    process::Command,
    sync::{Arc, Mutex},
};

use teitunnel_core::{
    inspect::{ExchangeQuery, Inspector, TapScope, TapSpec},
    store::Store,
};

fn cli(data: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
        .args(args)
        .env("TEITUNNEL_DATA_DIR", data)
        .output()
        .unwrap()
}

/// An origin on a thread (it must answer while the CLI runs): records request heads,
/// answers `201 ok`.
fn origin() -> (u16, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let heads = Arc::clone(&seen);
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let mut stream = stream;
            let mut buf = vec![0u8; 16 * 1024];
            let mut head = Vec::new();
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                let n = stream.read(&mut buf).unwrap_or(0);
                if n == 0 {
                    break;
                }
                head.extend_from_slice(&buf[..n]);
            }
            heads
                .lock()
                .unwrap()
                .push(String::from_utf8_lossy(&head).to_lowercase());
            let _ = stream.write_all(
                b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            );
        }
    });
    (port, seen)
}

/// One request to the tap, as cloudflared would send it.
fn send(addr: &str, method: &str, path: &str) {
    let mut stream = TcpStream::connect(addr.trim_start_matches("http://")).unwrap();
    let body = r#"{"event":"paid"}"#;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: hooks.example.com\r\nAuthorization: Bearer s3cr3t-value-123\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    assert!(response.starts_with("HTTP/1.1 201"), "{response}");
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_shows_replays_exports_and_clears_captures_of_another_process() {
    let data = tempfile::tempdir().unwrap();
    let (port, seen) = origin();
    {
        let store = Store::open(&data.path().join("teitunnel.db")).unwrap();
        let inspector = Inspector::new(Some(store), None, "1-1");
        let tap = inspector
            .start(TapSpec::new(
                TapScope::QuickShare {
                    share_id: "qs-clitest".into(),
                },
                "https://hooks.example.com",
                &format!("http://127.0.0.1:{port}"),
            ))
            .await
            .unwrap();
        let address = tap.address.clone();
        tokio::task::spawn_blocking(move || {
            send(&address, "GET", "/health");
            send(&address, "POST", "/webhooks/stripe?token=abc123");
        })
        .await
        .unwrap();
        for _ in 0..100 {
            if inspector
                .list(&ExchangeQuery::default())
                .items
                .iter()
                .filter(|r| r.status.is_some())
                .count()
                == 2
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        // Stopping writes what's left of the history.
        inspector.shutdown().await;
    }

    let run = |args: &'static [&'static str]| {
        let data = data.path().to_owned();
        async move {
            tokio::task::spawn_blocking(move || cli(&data, args))
                .await
                .unwrap()
        }
    };

    let listed = run(&["traffic", "ls"]).await;
    assert!(listed.status.success(), "{listed:?}");
    let lines = String::from_utf8_lossy(&listed.stdout).to_string();
    assert_eq!(lines.lines().count(), 2, "{lines}");
    assert!(
        lines.contains("POST") && lines.contains("/webhooks/stripe"),
        "{lines}"
    );
    assert!(!lines.contains("abc123"), "masked: {lines}");

    let json = run(&["traffic", "ls", "--method", "post", "--json"]).await;
    let json = String::from_utf8_lossy(&json.stdout).to_string();
    let first: serde_json::Value = serde_json::from_str(json.lines().next().unwrap()).unwrap();
    assert_eq!(first["request"]["method"], "POST");
    assert!(!json.contains("s3cr3t-value-123"));
    let id = first["id"].as_str().unwrap().to_owned();
    let short = id[id.len() - 8..].to_owned();

    let curl = {
        let data = data.path().to_owned();
        let short = short.clone();
        tokio::task::spawn_blocking(move || {
            cli(&data, &["traffic", "get", &short, "--format", "curl"])
        })
        .await
        .unwrap()
    };
    let curl = String::from_utf8_lossy(&curl.stdout).to_string();
    assert!(
        curl.starts_with("curl") && curl.contains("/webhooks/stripe"),
        "{curl}"
    );
    assert!(!curl.contains("s3cr3t-value-123"), "{curl}");

    // A replay reaches the service again, without the credential that was masked.
    let before = seen.lock().unwrap().len();
    let replayed = {
        let data = data.path().to_owned();
        tokio::task::spawn_blocking(move || {
            cli(
                &data,
                &["traffic", "replay", &short, "--set-header", "X-Debug: 1"],
            )
        })
        .await
        .unwrap()
    };
    assert!(replayed.status.success(), "{replayed:?}");
    assert!(String::from_utf8_lossy(&replayed.stdout).contains("201"));
    let heads = seen.lock().unwrap().clone();
    assert_eq!(heads.len(), before + 1);
    let last = heads.last().unwrap();
    assert!(
        last.contains("x-debug: 1") && !last.contains("authorization"),
        "{last}"
    );
    assert!(String::from_utf8_lossy(&replayed.stderr).contains("Leaving out authorization"));

    let har = data.path().join("out.har");
    let exported = {
        let data = data.path().to_owned();
        let har = har.clone();
        tokio::task::spawn_blocking(move || {
            cli(
                &data,
                &["traffic", "export", "--har", har.to_str().unwrap()],
            )
        })
        .await
        .unwrap()
    };
    assert!(exported.status.success(), "{exported:?}");
    let har: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&har).unwrap()).unwrap();
    assert_eq!(har["log"]["entries"].as_array().unwrap().len(), 2);

    // The API those requests show, as OpenAPI (credentials and values left out).
    let described = run(&["traffic", "openapi", "--title", "Hooks"]).await;
    assert!(described.status.success(), "{described:?}");
    let document: serde_json::Value = serde_json::from_slice(&described.stdout).unwrap();
    assert_eq!(document["openapi"], "3.1.0");
    assert_eq!(document["info"]["title"], "Hooks");
    let post = &document["paths"]["/webhooks/stripe"]["post"];
    assert!(
        post["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "token")
    );
    assert_eq!(
        post["requestBody"]["content"]["application/json"]["schema"]["properties"]["event"]["type"],
        "string"
    );
    assert_eq!(post["security"], serde_json::json!([{ "bearerAuth": [] }]));
    let text = described.stdout.clone();
    let text = String::from_utf8_lossy(&text);
    assert!(!text.contains("abc123") && !text.contains("paid") && !text.contains("s3cr3t"));
    let yaml = data.path().join("api.yaml");
    let written = {
        let data = data.path().to_owned();
        let yaml = yaml.clone();
        tokio::task::spawn_blocking(move || {
            cli(
                &data,
                &["traffic", "openapi", "--out", yaml.to_str().unwrap()],
            )
        })
        .await
        .unwrap()
    };
    assert!(written.status.success(), "{written:?}");
    assert!(std::fs::read_to_string(&yaml).unwrap().contains("openapi:"));

    assert!(run(&["traffic", "clear"]).await.status.success());
    let empty = run(&["traffic", "ls"]).await;
    assert!(empty.stdout.is_empty());
}

#[test]
fn traffic_needs_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let output = cli(dir.path(), &["traffic", "ls"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("No captured requests"));
}

/// Quick Shares go through the inspector unless told not to.
#[test]
fn shares_are_inspected_by_default() {
    let fake = std::path::Path::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
        .with_file_name("fake-cloudflared");
    if !fake.exists() {
        eprintln!("skipped: build the workspace to get fake-cloudflared");
        return;
    }
    let share = |extra: &[&str]| {
        let dir = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
            .args(["share", "3000", "--no-qr", "--for", "2s"])
            .args(extra)
            .env("TEITUNNEL_DATA_DIR", dir.path())
            .env("TEITUNNEL_CLOUDFLARED", &fake)
            .env("TEITUNNEL_EDGE", "127.0.0.1:9")
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stderr).to_string()
    };
    assert!(share(&[]).contains("Teitunnel's inspector"));
    assert!(!share(&["--no-inspect"]).contains("Teitunnel's inspector"));
}

#[test]
fn mcp_exposure_needs_your_own_domain() {
    let dir = tempfile::tempdir().unwrap();
    let output = cli(dir.path(), &["share", "8000", "--mcp"]);
    assert_eq!(output.status.code(), Some(2), "a usage error");
    assert!(String::from_utf8_lossy(&output.stderr).contains("--on"));
}
