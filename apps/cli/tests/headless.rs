//! Headless use, end to end: the API token comes from the environment (never stored),
//! routes are added through the fake Cloudflare API, and `up` runs the connector with the
//! fake cloudflared until interrupted.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};

const TOKEN: &str = "e2e-headless-token-0123456789";

/// A binary built next to this one by a workspace build.
fn sibling(name: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_BIN_EXE_teitunnel-cli")).with_file_name(name);
    path.exists().then_some(path)
}

struct Fake {
    child: Child,
    addr: String,
}

impl Drop for Fake {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn fake_cloudflare(path: &Path) -> Fake {
    let mut child = Command::new(path).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let addr = line.trim().trim_start_matches("listening on ").to_owned();
    Fake { child, addr }
}

fn cli(data: &Path, fake: &Fake, cloudflared: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"));
    command
        .env("TEITUNNEL_DATA_DIR", data)
        .env("CLOUDFLARE_API_TOKEN", TOKEN)
        .env("TEITUNNEL_API_BASE", format!("http://{}", fake.addr))
        .env("TEITUNNEL_EDGE", &fake.addr)
        .env("TEITUNNEL_CLOUDFLARED", cloudflared);
    command
}

fn run(command: &mut Command) -> Output {
    command.stdin(Stdio::null()).output().unwrap()
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn adds_routes_and_runs_them_with_a_token_from_the_environment() {
    let (Some(api), Some(cloudflared)) = (sibling("fake-cloudflare"), sibling("fake-cloudflared"))
    else {
        eprintln!("skipped: build the workspace first (fake-cloudflare, fake-cloudflared)");
        return;
    };
    let fake = fake_cloudflare(&api);
    let data = tempfile::tempdir().unwrap();

    let added = run(cli(data.path(), &fake, &cloudflared).args([
        "route",
        "add",
        "app.xyz.com",
        "3000",
        "--yes",
    ]));
    let out = text(&added);
    assert!(out.contains("app.xyz.com"), "{out}");

    let tunnels = run(cli(data.path(), &fake, &cloudflared).args(["tunnels", "--json"]));
    let list: serde_json::Value = serde_json::from_slice(&tunnels.stdout).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");

    // `up` runs the connector in the foreground until interrupted.
    let mut up = cli(data.path(), &fake, &cloudflared)
        .arg("up")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(up.stderr.take().unwrap()).lines();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut seen = Vec::new();
    loop {
        assert!(Instant::now() < deadline, "no connection: {seen:?}");
        let line = lines.next().expect("up ended early").unwrap();
        seen.push(line.clone());
        if line.ends_with(": connected") {
            break;
        }
    }
    // While it runs, the health check passes.
    let mut healthy = false;
    for _ in 0..40 {
        if run(cli(data.path(), &fake, &cloudflared).args(["routes", "--check"]))
            .status
            .success()
        {
            healthy = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    assert!(healthy, "routes --check never passed while up ran");
    kill(
        Pid::from_raw(i32::try_from(up.id()).unwrap()),
        Signal::SIGINT,
    )
    .unwrap();
    assert!(up.wait().unwrap().success());
    // And fails once nothing serves the route.
    let checked = run(cli(data.path(), &fake, &cloudflared).args(["routes", "--check"]));
    assert!(!checked.status.success());
    assert!(text(&checked).contains("app.xyz.com"), "{}", text(&checked));

    // The token was used, never written down.
    for entry in walk(data.path()) {
        let bytes = std::fs::read(&entry).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(TOKEN),
            "the API token was stored in {}",
            entry.display()
        );
    }
}

#[test]
fn says_how_to_use_a_token_without_the_app() {
    let data = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
        .arg("routes")
        .env("TEITUNNEL_DATA_DIR", data.path())
        .env_remove("CLOUDFLARE_API_TOKEN")
        .env_remove("TEITUNNEL_API_TOKEN")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("CLOUDFLARE_API_TOKEN"));
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}

/// A minimal HTTP/1.1 request (no client library needed): status, headers, body.
fn http(
    addr: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&str>,
) -> (u16, String, String) {
    use std::io::{Read, Write};
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    let mut request = format!("{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n");
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    if let Some(body) = body {
        request.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    request.push_str(body.unwrap_or(""));
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap_or((&response, ""));
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, head.to_owned(), body.to_owned())
}

#[test]
fn serves_a_signed_in_dashboard_and_an_api() {
    let (Some(api), Some(cloudflared)) = (sibling("fake-cloudflare"), sibling("fake-cloudflared"))
    else {
        eprintln!("skipped: build the workspace first (fake-cloudflare, fake-cloudflared)");
        return;
    };
    let fake = fake_cloudflare(&api);
    let data = tempfile::tempdir().unwrap();
    let added = run(cli(data.path(), &fake, &cloudflared).args([
        "route",
        "add",
        "app.xyz.com",
        "3000",
        "--yes",
    ]));
    assert!(text(&added).contains("app.xyz.com"), "{}", text(&added));

    // Refuses to start without a password, and to listen publicly unless told.
    let refused =
        run(cli(data.path(), &fake, &cloudflared).args(["serve", "--listen", "127.0.0.1:0"]));
    assert!(
        text(&refused).contains("--set-password"),
        "{}",
        text(&refused)
    );
    let public = run(cli(data.path(), &fake, &cloudflared)
        .args(["serve", "--listen", "0.0.0.0:0"])
        .env("TEITUNNEL_WEB_PASSWORD", "a long enough password"));
    assert!(
        text(&public).contains("--allow-remote"),
        "{}",
        text(&public)
    );

    let mut serve = cli(data.path(), &fake, &cloudflared)
        .args(["serve", "--listen", "127.0.0.1:0"])
        .env("TEITUNNEL_WEB_PASSWORD", "a long enough password")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(serve.stderr.take().unwrap()).lines();
    let addr = loop {
        let line = lines.next().expect("serve ended early").unwrap();
        if let Some(rest) = line.split("http://").nth(1) {
            break rest
                .split_whitespace()
                .next()
                .unwrap()
                .trim_end_matches('.')
                .to_owned();
        }
    };

    // The page, with a strict content security policy.
    let (status, head, body) = http(&addr, "GET", "/", &[], None);
    assert_eq!(status, 200);
    let head = head.to_ascii_lowercase();
    assert!(
        head.contains("content-security-policy: default-src 'self'"),
        "{head}"
    );
    assert!(head.contains("x-frame-options: deny"));
    assert!(body.contains("/app.js"));

    // Nothing without signing in.
    assert_eq!(http(&addr, "GET", "/api/overview", &[], None).0, 401);
    // Signing in needs the header a cross-site form can't send, and the right password.
    let login = r#"{"password":"a long enough password"}"#;
    assert_eq!(http(&addr, "POST", "/api/login", &[], Some(login)).0, 403);
    let wrong = r#"{"password":"not the password"}"#;
    assert_eq!(
        http(
            &addr,
            "POST",
            "/api/login",
            &[("X-Teitunnel", "1")],
            Some(wrong)
        )
        .0,
        401
    );
    let (status, head, _) = http(
        &addr,
        "POST",
        "/api/login",
        &[("X-Teitunnel", "1")],
        Some(login),
    );
    assert_eq!(status, 200);
    let lower = head.to_ascii_lowercase();
    assert!(
        lower.contains("httponly") && lower.contains("samesite=strict"),
        "{head}"
    );
    let session = head
        .lines()
        .find_map(|l| {
            l.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("set-cookie"))
                .map(|(_, value)| value.trim())
        })
        .and_then(|c| c.split(';').next())
        .unwrap()
        .to_owned();
    let cookie = [("Cookie", session.as_str())];

    let (status, _, body) = http(&addr, "GET", "/api/overview", &cookie, None);
    assert_eq!(status, 200);
    let overview: serde_json::Value = serde_json::from_str(&body).unwrap();
    let account = overview["accounts"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        overview["accounts"][0]["overview"]["routes"][0]["hostname"],
        "app.xyz.com"
    );

    // A change from the browser needs the header too; the plan comes back in English.
    let remove = format!(
        r#"{{"accountId":"{account}","change":{{"type":"removeRoute","hostname":"app.xyz.com","path":null}}}}"#
    );
    assert_eq!(
        http(&addr, "POST", "/api/preview", &cookie, Some(&remove)).0,
        403
    );
    let (status, _, body) = http(
        &addr,
        "POST",
        "/api/preview",
        &[cookie[0], ("X-Teitunnel", "1")],
        Some(&remove),
    );
    assert_eq!(status, 200, "{body}");
    let plan: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(plan["steps"][0]["description"].is_string(), "{plan}");

    // Automation: an API key applies the reviewed plan by its fingerprint.
    let key = run(cli(data.path(), &fake, &cloudflared).args(["api-key", "create", "deploy"]));
    let key = String::from_utf8_lossy(&key.stdout).trim().to_owned();
    assert!(key.starts_with("ttk_"), "{key}");
    let bearer = format!("Bearer {key}");
    let apply = format!(
        r#"{{"accountId":"{account}","change":{{"type":"removeRoute","hostname":"app.xyz.com","path":null}},"fingerprint":"{}"}}"#,
        plan["fingerprint"].as_str().unwrap()
    );
    let (status, _, body) = http(
        &addr,
        "POST",
        "/api/apply",
        &[("Authorization", bearer.as_str())],
        Some(&apply),
    );
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("\"applied\""), "{body}");
    assert_eq!(
        http(
            &addr,
            "POST",
            "/api/apply",
            &[("Authorization", "Bearer ttk_nope")],
            Some(&apply)
        )
        .0,
        401
    );

    kill(
        Pid::from_raw(i32::try_from(serve.id()).unwrap()),
        Signal::SIGINT,
    )
    .unwrap();
    assert!(serve.wait().unwrap().success());
}
