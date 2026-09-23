//! The CLI as a process, against an isolated (empty) data folder: never the real
//! keychain or Cloudflare.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::process::Command;

fn cli(data: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
        .args(args)
        .env("TEITUNNEL_DATA_DIR", data)
        .output()
        .unwrap()
}

#[test]
fn says_what_to_do_before_the_app_is_set_up() {
    let dir = tempfile::tempdir().unwrap();
    let output = cli(dir.path(), &["routes"]);
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("hasn't been set up on this machine yet"),
        "{stderr}"
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn rejects_unknown_export_formats() {
    let dir = tempfile::tempdir().unwrap();
    let output = cli(dir.path(), &["export", "helm"]);
    assert_eq!(output.status.code(), Some(2), "a usage error");
    assert!(String::from_utf8_lossy(&output.stderr).contains("config-yaml"));
}

#[test]
fn prints_shell_completions() {
    let dir = tempfile::tempdir().unwrap();
    let output = cli(dir.path(), &["completions", "zsh"]);
    assert!(output.status.success());
    let script = String::from_utf8_lossy(&output.stdout);
    assert!(script.starts_with("#compdef teitunnel-cli"), "{script}");
    for command in ["share", "doctor", "route", "export"] {
        assert!(script.contains(command), "{command}");
    }
}

#[test]
fn doctor_needs_the_app_to_be_set_up() {
    let dir = tempfile::tempdir().unwrap();
    let output = cli(dir.path(), &["doctor"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("hasn't been set up"));
    let output = cli(dir.path(), &["doctor", "--yes"]);
    assert_eq!(output.status.code(), Some(2), "--yes needs --fix");
}

#[test]
fn share_rejects_bad_input_before_starting_anything() {
    let dir = tempfile::tempdir().unwrap();
    let output = cli(dir.path(), &["share", "not a port"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let output = cli(dir.path(), &["share", "3000", "--for", "1d"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("isn't a duration"));
}

/// `share` against the fake cloudflared, as the process a terminal would run.
#[cfg(unix)]
mod share {
    use std::{
        io::{BufRead, BufReader},
        path::{Path, PathBuf},
        process::{Child, Command, Stdio},
        time::{Duration, Instant},
    };

    use nix::{
        sys::signal::{Signal, kill},
        unistd::Pid,
    };

    /// The fake cloudflared, built next to this binary by a workspace build.
    fn fake() -> Option<PathBuf> {
        let path =
            Path::new(env!("CARGO_BIN_EXE_teitunnel-cli")).with_file_name("fake-cloudflared");
        path.exists().then_some(path)
    }

    fn spawn(data: &Path, fake: &Path, args: &[&str]) -> Child {
        Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
            .arg("share")
            .args(args)
            .env("TEITUNNEL_DATA_DIR", data)
            .env("TEITUNNEL_CLOUDFLARED", fake)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    }

    fn alive(pid: u32) -> bool {
        kill(Pid::from_raw(i32::try_from(pid).unwrap()), None).is_ok()
    }

    fn wait_until(what: &str, check: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !check() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// The pid of the cloudflared the share runs, from the CLI's registry.
    fn connector_pid(data: &Path) -> u32 {
        std::fs::read_dir(data.join("run-cli"))
            .unwrap()
            .flatten()
            .flat_map(|owner| std::fs::read_dir(owner.path()).unwrap().flatten())
            .find_map(|record| {
                let json: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(record.path()).ok()?).ok()?;
                u32::try_from(json["pid"].as_u64()?).ok()
            })
            .expect("a connector is recorded")
    }

    /// Starts a share and waits for its URL, the one line on stdout.
    fn live_share(data: &Path, fake: &Path) -> (Child, String) {
        let mut child = spawn(data, fake, &["3000", "--no-qr"]);
        let mut url = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut url)
            .unwrap();
        (child, url.trim().to_owned())
    }

    #[test]
    fn shares_until_interrupted_and_leaves_nothing_running() {
        let Some(fake) = fake() else {
            eprintln!("skipped: build the workspace to get fake-cloudflared");
            return;
        };
        let data = tempfile::tempdir().unwrap();
        let (child, url) = live_share(data.path(), &fake);
        assert!(
            url.starts_with("https://") && url.ends_with(".trycloudflare.com"),
            "{url}"
        );
        let connector = connector_pid(data.path());
        assert!(alive(connector));

        kill(
            Pid::from_raw(i32::try_from(child.id()).unwrap()),
            Signal::SIGINT,
        )
        .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("Stopped sharing."));
        wait_until("the connector to stop", || !alive(connector));
    }

    #[test]
    fn a_killed_cli_leaves_nothing_running_after_the_next_one() {
        let Some(fake) = fake() else {
            eprintln!("skipped: build the workspace to get fake-cloudflared");
            return;
        };
        let data = tempfile::tempdir().unwrap();
        let (mut child, _url) = live_share(data.path(), &fake);
        let connector = connector_pid(data.path());
        let owners = || {
            std::fs::read_dir(data.path().join("run-cli"))
                .unwrap()
                .count()
        };
        child.kill().unwrap(); // SIGKILL: no chance to clean up.
        child.wait().unwrap();
        assert_eq!(owners(), 1, "the dead CLI's registry is left behind");

        // The next run reaps whatever it left (cloudflared may also have exited on its
        // own at its next log line, once the pipe was gone); `--for 1s` ends that run.
        let output = spawn(data.path(), &fake, &["3000", "--for", "1s"])
            .wait_with_output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        wait_until("the orphan to stop", || !alive(connector));
        assert_eq!(owners(), 1, "only the last run's (empty) registry remains");
    }
}
