//! Teams sharing one account, and CI (M12-11), end to end against the fakes: names
//! reserved in DNS with their owner, refused to someone else unless taken over, and a
//! share on a domain from a machine where nothing else runs its tunnel (a CI job),
//! printed as JSON, then cleaned up.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
};

use nix::{
    sys::signal::{Signal, kill},
    unistd::Pid,
};

const TOKEN: &str = "e2e-teams-token-0123456789";

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

/// Someone's `teitunnel`: their own data folder and owner label, the same account.
fn as_(who: &str, data: &Path, fake: &Fake, cloudflared: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"));
    command
        .env("TEITUNNEL_DATA_DIR", data)
        .env("CLOUDFLARE_API_TOKEN", TOKEN)
        .env("TEITUNNEL_API_BASE", format!("http://{}", fake.addr))
        .env("TEITUNNEL_EDGE", &fake.addr)
        .env("TEITUNNEL_CLOUDFLARED", cloudflared)
        .env("TEITUNNEL_OWNER", who)
        .env("TEITUNNEL_MACHINE_NAME", format!("{who}-machine"))
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

#[test]
fn names_are_reserved_refused_and_taken_over_and_a_ci_share_cleans_up() {
    let (Some(api), Some(cloudflared)) = (sibling("fake-cloudflare"), sibling("fake-cloudflared"))
    else {
        eprintln!("skipped: build the workspace first (fake-cloudflare, fake-cloudflared)");
        return;
    };
    let fake = fake_cloudflare(&api);
    let (alice_dir, bob_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let alice = |args: &[&str]| {
        as_("alice@Alice-MacBook", alice_dir.path(), &fake, &cloudflared)
            .args(args)
            .output()
            .unwrap()
    };
    let bob = |args: &[&str]| {
        as_("bob@ci", bob_dir.path(), &fake, &cloudflared)
            .args(args)
            .output()
            .unwrap()
    };

    let reserved = alice(&["reserve", "demo.xyz.com", "--until", "2099-12-31", "--yes"]);
    assert!(reserved.status.success(), "{}", text(&reserved));
    let listed = json(&alice(&["reservations", "ls", "--json"]));
    assert_eq!(listed["items"][0]["hostname"], "demo.xyz.com", "{listed}");
    assert_eq!(listed["items"][0]["mine"], true);
    assert_eq!(listed["items"][0]["owner"], "alice@Alice-MacBook");
    // Bob sees who holds it.
    let theirs = json(&bob(&["reservations", "--json"]));
    assert_eq!(theirs["items"][0]["mine"], false, "{theirs}");

    // A share never takes a held name (exit code 3), nor does a route without --take-over.
    let refused = bob(&["share", "3000", "--on", "demo.xyz.com", "--json"]);
    assert_eq!(refused.status.code(), Some(3), "{}", text(&refused));
    assert!(
        text(&refused).contains("alice@Alice-MacBook"),
        "{}",
        text(&refused)
    );
    let refused = bob(&["route", "add", "demo.xyz.com", "3000", "--yes"]);
    assert_eq!(refused.status.code(), Some(3), "{}", text(&refused));
    assert!(text(&refused).contains("--take-over"), "{}", text(&refused));

    // A CI job's share: its own connector, the URL as JSON, cleaned up when stopped.
    let mut share = as_("bob@ci", bob_dir.path(), &fake, &cloudflared)
        .args(["share", "3000", "--on", "pr-7.xyz.com", "--json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(share.stdout.take().unwrap())
        .read_line(&mut first)
        .unwrap();
    let live: serde_json::Value = serde_json::from_str(first.trim()).unwrap_or_else(|e| {
        let _ = share.kill();
        let mut err = String::new();
        let _ = std::io::Read::read_to_string(&mut share.stderr.take().unwrap(), &mut err);
        panic!("{e}: {first:?} {err}")
    });
    assert_eq!(live["url"], "https://pr-7.xyz.com");
    assert_eq!(live["hostname"], "pr-7.xyz.com");
    let shares = json(&bob(&["shares", "--json"]));
    assert_eq!(shares["domains"][0]["hostname"], "pr-7.xyz.com", "{shares}");
    kill(
        Pid::from_raw(i32::try_from(share.id()).unwrap()),
        Signal::SIGTERM,
    )
    .unwrap();
    assert!(share.wait().unwrap().success());
    let shares = json(&bob(&["shares", "--json"]));
    assert!(shares["domains"].as_array().unwrap().is_empty(), "{shares}");
    // The job's tunnel goes too (the action's post step).
    let deleted = bob(&["tunnel", "delete", "bob@ci-machine", "--yes"]);
    assert!(deleted.status.success(), "{}", text(&deleted));
    let tunnels = json(&bob(&["tunnels", "--json"]));
    assert!(tunnels.as_array().unwrap().is_empty(), "{tunnels}");

    // Taking over, explicitly.
    let taken = bob(&[
        "route",
        "add",
        "demo.xyz.com",
        "3000",
        "--yes",
        "--take-over",
    ]);
    assert!(taken.status.success(), "{}", text(&taken));
    let listed = json(&alice(&["reservations", "--json"]));
    assert!(
        listed["items"].as_array().unwrap().is_empty(),
        "Bob's route replaced the reservation: {listed}"
    );
    let released = alice(&["release", "demo.xyz.com", "--yes"]);
    assert_eq!(released.status.code(), Some(1), "{}", text(&released));
    assert!(
        text(&released).contains("isn't reserved"),
        "{}",
        text(&released)
    );
}
