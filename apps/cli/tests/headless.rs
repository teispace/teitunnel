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
