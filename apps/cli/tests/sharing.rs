//! Sharing extras end to end against the fakes: a folder shared on a domain at
//! a name made from the project (`{project}`), served by the inspector without its
//! secrets, paused and resumed from another command, scheduled, and the name reused
//! with `--on` alone.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    time::{Duration, Instant},
};

use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};

const TOKEN: &str = "e2e-sharing-token-0123456789";

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

fn cli(data: &Path, fake: &Fake, cloudflared: &Path, folder: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"));
    command
        .current_dir(folder)
        .env("TEITUNNEL_DATA_DIR", data)
        .env("CLOUDFLARE_API_TOKEN", TOKEN)
        .env("TEITUNNEL_API_BASE", format!("http://{}", fake.addr))
        .env("TEITUNNEL_EDGE", &fake.addr)
        .env("TEITUNNEL_CLOUDFLARED", cloudflared)
        .env("TEITUNNEL_MACHINE_NAME", "sharing-machine")
        .stdin(Stdio::null());
    command
}

fn text(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

fn json(output: &Output) -> serde_json::Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(output)))
}

/// A GET straight to the inspector's tap (as cloudflared would send it).
fn get(tap: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(tap.trim_start_matches("http://")).unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: shop.xyz.com\r\nAccept: text/html\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut response = String::new();
    let _ = stream.read_to_string(&mut response);
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    (status, response)
}

fn wait_for(what: &str, check: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

/// Starts a share and returns it with the JSON line it prints once live.
fn live(command: &mut Command) -> (Child, serde_json::Value) {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    let value = serde_json::from_str(first.trim()).unwrap_or_else(|e| {
        let _ = child.kill();
        let mut err = String::new();
        let _ = child.stderr.take().unwrap().read_to_string(&mut err);
        panic!("{e}: {first:?} {err}")
    });
    (child, value)
}

fn stop(child: Child) {
    kill(
        Pid::from_raw(i32::try_from(child.id()).unwrap()),
        Signal::SIGTERM,
    )
    .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "{}", text(&output));
}

#[test]
fn a_folder_on_a_project_name_paused_resumed_and_scheduled() {
    let (Some(api), Some(cloudflared)) = (sibling("fake-cloudflare"), sibling("fake-cloudflared"))
    else {
        eprintln!("skipped: build the workspace first (fake-cloudflare, fake-cloudflared)");
        return;
    };
    let fake = fake_cloudflare(&api);
    let data = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::write(project.path().join("package.json"), r#"{"name": "shop"}"#).unwrap();
    let site = project.path().join("dist");
    std::fs::create_dir_all(&site).unwrap();
    std::fs::write(site.join("index.html"), "<h1>shop</h1>").unwrap();
    std::fs::write(site.join(".env"), "SECRET=1").unwrap();
    let run = |args: &[&str]| {
        cli(data.path(), &fake, &cloudflared, project.path())
            .args(args)
            .output()
            .unwrap()
    };

    let (share, shown) = live(cli(data.path(), &fake, &cloudflared, project.path()).args([
        "share",
        "./dist",
        "--on",
        "{project}.xyz.com",
        "--json",
    ]));
    assert_eq!(shown["hostname"], "shop.xyz.com", "{shown}");
    let shares = json(&run(&["shares", "--json"]));
    let domain = &shares["domains"][0];
    assert_eq!(domain["hostname"], "shop.xyz.com", "{shares}");
    assert_eq!(domain["folder"], true);
    assert!(
        domain["source"].as_str().unwrap().ends_with("dist"),
        "{shares}"
    );
    let tap = domain["origin"].as_str().unwrap().to_owned();
    assert_eq!(get(&tap, "/").0, 200);
    assert!(get(&tap, "/").1.contains("<h1>shop</h1>"));
    assert_eq!(get(&tap, "/.env").0, 404, "secrets are never served");

    // Paused from another command: the address stays, visitors get the paused page.
    let paused = run(&["shares", "--pause", "https://shop.xyz.com"]);
    assert!(paused.status.success(), "{}", text(&paused));
    wait_for("the paused page", || get(&tap, "/").0 == 503);
    assert!(get(&tap, "/").1.contains("Paused"));
    let listed = json(&run(&["shares", "--json"]));
    assert_eq!(listed["domains"][0]["paused"], true, "{listed}");
    let resumed = run(&["shares", "--resume", "shop.xyz.com"]);
    assert!(resumed.status.success(), "{}", text(&resumed));
    wait_for("the site again", || get(&tap, "/").0 == 200);

    // A schedule, shown and listed.
    let scheduled = run(&[
        "schedule",
        "shop.xyz.com",
        "mon-fri",
        "09:00-18:00",
        "--tz",
        "Europe/Berlin",
    ]);
    assert!(scheduled.status.success(), "{}", text(&scheduled));
    assert!(text(&scheduled).contains("mon,tue,wed,thu,fri 09:00-18:00 (Europe/Berlin)"));
    let schedules = json(&run(&["schedules", "--json"]));
    assert_eq!(schedules[0]["hostname"], "shop.xyz.com", "{schedules}");
    let bad = run(&["schedule", "shop.xyz.com", "someday", "09:00-18:00"]);
    assert!(!bad.status.success());

    stop(share);
    let shares = json(&run(&["shares", "--json"]));
    assert!(shares["domains"].as_array().unwrap().is_empty(), "{shares}");
    let schedules = json(&run(&["schedules", "--json"]));
    assert!(
        schedules.as_array().unwrap().is_empty(),
        "the schedule ends with its share: {schedules}"
    );

    // The name used in this folder is remembered.
    let (again, shown) = live(
        cli(data.path(), &fake, &cloudflared, project.path())
            .args(["share", "./dist", "--on", "--json"]),
    );
    assert_eq!(shown["hostname"], "shop.xyz.com", "{shown}");
    stop(again);

    // Nothing serves a route's paused page without the app, `up` or `serve`.
    let refused = run(&["shares", "--pause", "other.xyz.com"]);
    assert!(!refused.status.success());
    assert!(
        text(&refused).contains("teitunnel up"),
        "{}",
        text(&refused)
    );
}
