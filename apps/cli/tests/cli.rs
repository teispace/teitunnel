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
        stderr.contains("hasn't been set up on this Mac yet"),
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
