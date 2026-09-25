//! The CLI started by a browser as the extension's native messaging host.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    io::{Read, Write},
    process::{Command, Stdio},
};

fn frame(value: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(value).unwrap();
    let mut out = u32::try_from(body.len()).unwrap().to_le_bytes().to_vec();
    out.extend(body);
    out
}

#[test]
fn answers_the_extension_in_frames_without_the_app() {
    let data = tempfile::tempdir().unwrap();
    let mut host = Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
        .arg("chrome-extension://kfefjidmbgiebhjcgkphjigickhclpic/")
        .env("TEITUNNEL_DATA_DIR", data.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = host.stdin.take().unwrap();
    stdin
        .write_all(&frame(
            &serde_json::json!({"id": 7, "method": "shares.list"}),
        ))
        .unwrap();
    drop(stdin);
    let mut out = Vec::new();
    host.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    assert!(host.wait().unwrap().success());
    let length = u32::from_le_bytes(out[..4].try_into().unwrap()) as usize;
    assert_eq!(out.len(), 4 + length, "only framed messages on stdout");
    let reply: serde_json::Value = serde_json::from_slice(&out[4..]).unwrap();
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["code"], "appNotRunning");
}

#[test]
fn installs_for_detected_browsers_only() {
    let home = tempfile::tempdir().unwrap();
    let chrome = if cfg!(target_os = "macos") {
        home.path()
            .join("Library/Application Support/Google/Chrome")
    } else {
        home.path().join(".config/google-chrome")
    };
    std::fs::create_dir_all(&chrome).unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
            .args(args)
            .env("HOME", home.path())
            .env("TEITUNNEL_DATA_DIR", home.path().join("data"))
            .output()
            .unwrap()
    };
    if cfg!(windows) {
        return;
    }
    let installed = run(&["browser", "install"]);
    assert!(installed.status.success(), "{installed:?}");
    let text = String::from_utf8(installed.stdout).unwrap();
    assert!(text.contains("Google Chrome: ready"), "{text}");
    assert!(!text.contains("Firefox"), "not installed there: {text}");
    let status = run(&["browser", "status", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert!(
        status
            .as_array()
            .unwrap()
            .iter()
            .any(|b| b["browser"] == "chrome" && b["installed"] == true)
    );
    assert!(run(&["browser", "uninstall"]).status.success());
    assert!(
        !chrome
            .join("NativeMessagingHosts/com.teispace.teitunnel.json")
            .exists()
    );
}
