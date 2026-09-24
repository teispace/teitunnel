//! `teitunnel local-domain` end to end, without the app and without touching the real
//! system: an isolated data folder, the CA key in a file there, a file standing in for
//! the trust stores, free ports and no multicast DNS.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::Path,
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

fn command(data: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"));
    command
        .env("TEITUNNEL_DATA_DIR", data)
        .env("TEITUNNEL_LOCAL_CA_FILE", data.join("ca-key.pem"))
        .env("TEITUNNEL_TEST_TRUST_FILE", data.join("trust.json"))
        .env("TEITUNNEL_LOCAL_HTTPS_PORT", "0")
        .env("TEITUNNEL_LOCAL_HTTP_PORT", "0")
        .env("TEITUNNEL_LOCAL_DNS_PORT", "0")
        .env("TEITUNNEL_LOCAL_NO_MDNS", "1");
    command
}

fn cli(data: &Path, args: &[&str]) -> Output {
    command(data).args(args).output().unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Kills the child when dropped (a failed assertion mustn't leave a server running).
struct Serving(Child);

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[test]
fn adds_lists_trusts_serves_and_removes_without_the_app() {
    let dir = tempfile::tempdir().unwrap();
    let data = dir.path();

    let added = cli(data, &["local-domain", "add", "shop", "3000", "--no-serve"]);
    assert!(added.status.success(), "{}", text(&added));
    assert!(
        text(&added).contains("Saved shop.localhost"),
        "{}",
        text(&added)
    );

    let bad = cli(
        data,
        &["local-domain", "add", "sh_op.test", "3000", "--no-serve"],
    );
    assert!(!bad.status.success());
    let dup = cli(
        data,
        &["local", "add", "shop.localhost", "4000", "--no-serve"],
    );
    assert!(!dup.status.success());
    assert!(
        text(&dup).contains("already a local domain"),
        "{}",
        text(&dup)
    );

    let listed = cli(data, &["local-domain", "ls", "--json"]);
    assert!(listed.status.success(), "{}", text(&listed));
    let list: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
    assert_eq!(list["domains"][0]["name"], "shop.localhost");
    assert_eq!(list["domains"][0]["origin"], "http://localhost:3000");
    assert_eq!(list["domains"][0]["serving"], false);

    // Trust goes to the stand-in file, never the system's stores.
    let trusted = cli(data, &["local-domain", "trust"]);
    assert!(trusted.status.success(), "{}", text(&trusted));
    assert!(text(&trusted).contains("Browsers trust your local domains."));
    assert!(data.join("trust.json").exists());
    let key = std::fs::read_to_string(data.join("ca-key.pem")).unwrap();
    assert!(
        key.contains("PRIVATE KEY"),
        "the CA key is where it was told to be"
    );
    let status = cli(data, &["local-domain", "status", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["trust"]["trusted"], true);

    // Served from this terminal: plain HTTP redirects to HTTPS on the port it got.
    let mut child = command(data)
        .args(["local-domain", "serve"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stderr = child.stderr.take().unwrap();
    let serving = Serving(child);
    let mut lines = BufReader::new(stderr).lines();
    let (mut https, mut http) = (None, None);
    let deadline = Instant::now() + Duration::from_secs(20);
    while (https.is_none() || http.is_none()) && Instant::now() < deadline {
        let Some(Ok(line)) = lines.next() else { break };
        if let Some(port) = line.strip_prefix("HTTPS on port ") {
            https = port.trim().parse::<u16>().ok();
        }
        if let Some(port) = line.strip_prefix("Plain HTTP on port ") {
            http = port.trim().parse::<u16>().ok();
        }
    }
    let (https, http) = (https.expect("HTTPS port"), http.expect("HTTP port"));
    let mut stream = TcpStream::connect(("127.0.0.1", http)).unwrap();
    write!(
        stream,
        "GET /cart HTTP/1.1\r\nHost: shop.localhost\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 308"), "{response}");
    assert!(
        response
            .to_lowercase()
            .contains(&format!("location: https://shop.localhost:{https}/cart")),
        "{response}"
    );
    drop(serving);

    let removed = cli(data, &["local-domain", "rm", "shop"]);
    assert!(removed.status.success(), "{}", text(&removed));
    let listed = cli(data, &["local-domain", "ls"]);
    assert!(
        text(&listed).contains("No local domains"),
        "{}",
        text(&listed)
    );

    let forgotten = cli(data, &["local-domain", "untrust", "--forget"]);
    assert!(forgotten.status.success(), "{}", text(&forgotten));
    assert!(!data.join("ca-key.pem").exists());
}
