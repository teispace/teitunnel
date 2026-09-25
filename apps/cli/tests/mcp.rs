//! `teitunnel mcp` end to end over stdio, as an AI client runs it: the handshake,
//! Quick Shares with the fake cloudflared (recorded for the app, stopped by the app's
//! SIGTERM while the server keeps serving), and routes through plan → apply against
//! the fake Cloudflare API, recorded in Activity under the client's name.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value
)]

use std::{
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

use serde_json::{Value, json};

const TOKEN: &str = "e2e-mcp-token-0123456789";

/// A binary built next to this one by a workspace build.
fn sibling(name: &str) -> Option<PathBuf> {
    let path = Path::new(env!("CARGO_BIN_EXE_teitunnel-cli")).with_file_name(name);
    path.exists().then_some(path)
}

struct Session {
    child: Child,
    stdin: Option<ChildStdin>,
    lines: mpsc::Receiver<String>,
    next_id: u64,
}

impl Session {
    fn start(command: &mut Command) -> Self {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        let mut session = Self {
            stdin: child.stdin.take(),
            child,
            lines,
            next_id: 1,
        };
        let init = session.request(
            "initialize",
            json!({
                "protocolVersion": "2025-11-25",
                "capabilities": {},
                "clientInfo": { "name": "e2e-agent", "version": "9.9" }
            }),
        );
        assert_eq!(init["serverInfo"]["name"], "teitunnel", "{init}");
        session.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        session
    }

    fn send(&mut self, message: &Value) {
        let stdin = self.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let line = self
                .lines
                .recv_timeout(Duration::from_secs(60))
                .expect("an answer");
            let message: Value = serde_json::from_str(&line).expect("stdout carries only JSON-RPC");
            if message["id"] == json!(id) {
                assert!(message.get("error").is_none(), "{message}");
                return message["result"].clone();
            }
        }
    }

    /// Calls a tool; returns its structured result.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        );
        assert_ne!(result["isError"], json!(true), "{name}: {result}");
        result["structuredContent"].clone()
    }

    fn finish(mut self) -> std::process::ExitStatus {
        drop(self.stdin.take());
        self.child.wait().unwrap()
    }
}

#[test]
fn shares_a_port_and_lets_the_app_stop_it() {
    let Some(cloudflared) = sibling("fake-cloudflared") else {
        eprintln!("skipped: build the workspace first (fake-cloudflared)");
        return;
    };
    let data = tempfile::tempdir().unwrap();
    let mut session = Session::start(
        Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
            .args(["mcp", "--mode", "full"])
            .env("TEITUNNEL_DATA_DIR", data.path())
            .env("TEITUNNEL_CLOUDFLARED", &cloudflared)
            .env_remove("CLOUDFLARE_API_TOKEN")
            .env_remove("TEITUNNEL_API_TOKEN"),
    );
    let tools = session.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    // Teitunnel's 26, the sharing extras' 4 and traffic_openapi (M12-06, M12-12), the
    // protection provider's 5 (M12-04), the reservation provider's 3 (M12-11), the
    // comments provider's 3 (M12-06), expose_mcp_server (M12-02), the offline page
    // and inbox provider's 3, the inspection provider's 2, route_health, and the local
    // domain provider's 3.
    assert_eq!(names.len(), 52, "{names:?}");
    for tool in [
        "set_offline_page",
        "set_webhook_inbox",
        "comments_list",
        "comments_reply",
        "comments_resolve",
        "get_protection",
        "protect_hostname",
        "list_reservations",
        "reserve_hostname",
        "release_hostname",
        "expose_mcp_server",
        "inspection_settings",
        "configure_inspection",
        "route_health",
        "list_local_domains",
        "add_local_domain",
        "remove_local_domain",
    ] {
        assert!(names.contains(&tool), "{tool}");
    }

    // Without an account, what needs one says so.
    let routes = session.request(
        "tools/call",
        json!({ "name": "list_routes", "arguments": {} }),
    );
    assert_eq!(routes["isError"], true);

    let shared = session.call("share_port", json!({ "target": "3000" }));
    assert_eq!(shared["outcome"], "shared", "{shared}");
    let url = shared["share"]["url"].as_str().unwrap().to_owned();
    assert!(url.contains("trycloudflare.com"), "{url}");

    // The app sees it with the terminals' shares…
    let runs = data.path().join("run-cli");
    let recorded = teitunnel_core::cli_shares::list(&runs);
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].url, url);

    // …and its Stop (a SIGTERM to the owner) stops the share, not the server.
    let stopped = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(teitunnel_core::cli_shares::stop(&runs, &recorded[0].owner));
    assert!(stopped);
    assert!(teitunnel_core::cli_shares::list(&runs).is_empty());
    let shares = session.call("list_shares", json!({}));
    assert_eq!(shares["total"], 0, "{shares}");

    // Closing stdin ends the server cleanly.
    assert!(session.finish().success());
}

#[test]
fn asks_the_person_in_the_app_when_it_runs() {
    let Some(cloudflared) = sibling("fake-cloudflared") else {
        eprintln!("skipped: build the workspace first (fake-cloudflared)");
        return;
    };
    let data = tempfile::tempdir().unwrap();
    // The app, as its control connection answers.
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let app = runtime
        .block_on(teitunnel_control::testing::serve(
            data.path(),
            teitunnel_control::Limits::default(),
        ))
        .unwrap();
    let mut session = Session::start(
        Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
            .args(["mcp", "--mode", "ask"])
            .env("TEITUNNEL_DATA_DIR", data.path())
            .env("TEITUNNEL_CLOUDFLARED", &cloudflared)
            .env_remove("CLOUDFLARE_API_TOKEN")
            .env_remove("TEITUNNEL_API_TOKEN"),
    );
    // Listed in the app while connected.
    for _ in 0..100 {
        if !app.host.agents.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    {
        let agents = app.host.agents.lock().unwrap();
        assert_eq!(agents[0].1.name, "e2e-agent");
        assert_eq!(agents[0].1.mode, "ask");
    }

    // The person says no in the app: nothing is shared, even though the client can't ask.
    let declined = session.call("share_port", json!({ "target": "3000" }));
    assert_eq!(declined["outcome"], "declined", "{declined}");
    assert!(teitunnel_core::cli_shares::list(&data.path().join("run-cli")).is_empty());
    // …then yes.
    app.host.agent_answers.lock().unwrap().push_back(true);
    let shared = session.call("share_port", json!({ "target": "3000" }));
    assert_eq!(shared["outcome"], "shared", "{shared}");
    {
        let questions = app.host.agent_questions.lock().unwrap();
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[1].agent, "e2e-agent");
        assert!(questions[1].title.contains("3000"), "{:?}", questions[1]);
    }
    assert!(session.finish().success());
    drop(app);
}

#[test]
fn changes_routes_through_reviewed_plans() {
    let (Some(api), Some(cloudflared)) = (sibling("fake-cloudflare"), sibling("fake-cloudflared"))
    else {
        eprintln!("skipped: build the workspace first (fake-cloudflare, fake-cloudflared)");
        return;
    };
    let mut fake = Command::new(api).stdout(Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    BufReader::new(fake.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let addr = line.trim().trim_start_matches("listening on ").to_owned();
    let data = tempfile::tempdir().unwrap();
    let mut session = Session::start(
        Command::new(env!("CARGO_BIN_EXE_teitunnel-cli"))
            .arg("mcp")
            .env("TEITUNNEL_DATA_DIR", data.path())
            .env("CLOUDFLARE_API_TOKEN", TOKEN)
            .env("TEITUNNEL_API_BASE", format!("http://{addr}"))
            .env("TEITUNNEL_EDGE", &addr)
            .env("TEITUNNEL_CLOUDFLARED", &cloudflared),
    );
    let plan = session.call(
        "plan_change",
        json!({ "change": { "type": "addRoute", "hostname": "app.xyz.com", "origin": "3000" } }),
    );
    let steps = plan["plan"]["steps"].as_array().unwrap();
    assert!(!steps.is_empty(), "{plan}");
    let args = json!({ "planId": plan["plan"]["planId"], "fingerprint": plan["plan"]["fingerprint"], "verify": false });

    // Ask mode, and this client can't be asked: nothing happens without `confirmed`.
    let unconfirmed = session.call("apply_plan", args.clone());
    assert_eq!(unconfirmed["outcome"], "needsApproval", "{unconfirmed}");
    let mut confirmed = args;
    confirmed["confirmed"] = json!(true);
    let applied = session.call("apply_plan", confirmed);
    assert_eq!(applied["outcome"], "applied", "{applied}");

    let routes = session.call("list_routes", json!({}));
    assert_eq!(routes["routes"][0]["hostname"], "app.xyz.com", "{routes}");
    let activity = session.call("recent_activity", json!({ "agentsOnly": true }));
    assert_eq!(activity["entries"][0]["by"], "e2e-agent", "{activity}");
    let undo = session.call("undo_last", json!({}));
    assert!(
        undo["plan"]["summary"]
            .as_str()
            .unwrap()
            .contains("app.xyz.com"),
        "{undo}"
    );

    // Reservations, through the provider: approval first, then listed with the owner.
    let reserve = json!({ "hostname": "alice.xyz.com", "until": "2099-12-31" });
    let unconfirmed = session.call("reserve_hostname", reserve.clone());
    assert_eq!(unconfirmed["outcome"], "needsApproval", "{unconfirmed}");
    let mut confirmed = reserve;
    confirmed["confirmed"] = json!(true);
    let reserved = session.call("reserve_hostname", confirmed);
    assert_eq!(reserved["outcome"], "applied", "{reserved}");
    let listed = session.call("list_reservations", json!({}));
    assert_eq!(
        listed["reservations"][0]["hostname"], "alice.xyz.com",
        "{listed}"
    );
    assert_eq!(listed["reservations"][0]["mine"], true);
    assert_eq!(listed["reservations"][0]["until"], "2100-01-01T00:00Z");
    let released = session.call(
        "release_hostname",
        json!({ "hostname": "alice.xyz.com", "confirmed": true }),
    );
    assert_eq!(released["outcome"], "applied", "{released}");

    assert!(session.finish().success());
    let _ = fake.kill();
    let _ = fake.wait();
}
