//! `teitunnel comments` against comments a live share kept in this computer's database
//! (written here as a reviewer would through the overlay), and `teitunnel inbox ls`
//! with nothing set up.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

use teitunnel_core::{
    comments::{Author, Comments, Subject},
    store::Store,
};

fn cli(data: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
        .args(args)
        .env("TEITUNNEL_DATA_DIR", data)
        .env_remove("CLOUDFLARE_API_TOKEN")
        .env_remove("TEITUNNEL_API_TOKEN")
        .output()
        .unwrap()
}

fn stdout(output: &std::process::Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn lists_replies_to_and_resolves_comments() {
    let data = tempfile::tempdir().unwrap();
    let comments = Comments::new(Store::open(&data.path().join("teitunnel.db")).unwrap());
    let empty = stdout(&cli(data.path(), &["comments", "ls"]));
    assert!(empty.contains("No comments yet"), "{empty}");

    // What a reviewer writes through the overlay of a live share.
    let thread = comments
        .local_start(
            &Subject::route("acc", "app.xyz.com"),
            "/pricing",
            None,
            "The price is too small",
            &Author::reviewer("Ana").unwrap(),
        )
        .await
        .unwrap();

    let listed = stdout(&cli(data.path(), &["comments", "ls"]));
    assert!(listed.contains("route:acc:app.xyz.com"), "{listed}");
    assert!(listed.contains("1 open, 1 comments, 1 new"), "{listed}");

    let threads = stdout(&cli(data.path(), &["comments", "ls", "app.xyz.com"]));
    assert!(threads.contains(&thread.id), "{threads}");
    assert!(threads.contains("The price is too small"), "{threads}");
    assert!(
        threads.contains("https://app.xyz.com/pricing#__teitunnel-comment="),
        "{threads}"
    );

    let replied = stdout(&cli(
        data.path(),
        &["comments", "reply", "app.xyz.com", &thread.id, "Fixed now"],
    ));
    assert!(replied.contains("(you):"), "{replied}");
    assert!(replied.contains("Fixed now"), "{replied}");

    let resolved = stdout(&cli(
        data.path(),
        &["comments", "resolve", "app.xyz.com", &thread.id],
    ));
    assert!(resolved.starts_with("Resolved"), "{resolved}");
    let open = stdout(&cli(data.path(), &["comments", "ls", "app.xyz.com"]));
    assert!(open.contains("No open comments"), "{open}");
    let all = stdout(&cli(
        data.path(),
        &["comments", "ls", "app.xyz.com", "--all", "--json"],
    ));
    let json: serde_json::Value = serde_json::from_str(&all).unwrap();
    assert_eq!(json[0]["resolved"], true);
    assert_eq!(json[0]["comments"].as_array().unwrap().len(), 2);

    let missing = cli(data.path(), &["comments", "ls", "nothing.xyz.com"]);
    assert!(!missing.status.success());

    let inboxes = stdout(&cli(data.path(), &["inbox", "ls"]));
    assert!(
        inboxes.contains("No offline pages or webhook inboxes."),
        "{inboxes}"
    );
}
