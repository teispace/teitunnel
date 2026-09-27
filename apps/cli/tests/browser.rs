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

/// Which browsers are detected depends on the computer running the test (their apps
/// are looked for where the system installs them; `teitunnel-core`'s unit tests cover
/// detection with folders of their own). Whatever is found, `install` sets up exactly the
/// detected browsers, and `uninstall` removes every manifest again.
#[test]
fn installs_for_detected_browsers_only() {
    if cfg!(windows) {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
            .args(args)
            .env("HOME", home.path())
            .env("TEITUNNEL_DATA_DIR", home.path().join("data"))
            .output()
            .unwrap()
    };
    let installed = run(&["browser", "install"]);
    assert!(installed.status.success(), "{installed:?}");
    let status = run(&["browser", "status", "--json"]);
    let status: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    let browsers = status.as_array().unwrap();
    assert!(browsers.iter().any(|b| b["browser"] == "chrome"));
    for browser in browsers {
        assert_eq!(browser["installed"], browser["detected"], "{browser}");
        let manifest = std::path::Path::new(browser["manifest"].as_str().unwrap());
        assert!(manifest.starts_with(home.path()), "{browser}");
        assert_eq!(manifest.exists(), browser["detected"] == true, "{browser}");
    }
    assert!(run(&["browser", "uninstall"]).status.success());
    for browser in browsers {
        let manifest = std::path::Path::new(browser["manifest"].as_str().unwrap());
        assert!(!manifest.exists(), "{browser}");
    }
}
