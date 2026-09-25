use std::sync::Mutex;

use serde_json::json;
use teitunnel_control::{
    ControlClient, Endpoint, Server,
    protocol::{AgentApproval, AgentInfo, ApplyParams, HostHeader, StartShare, StopShare, View},
};

use super::*;
use crate::{
    binary::Locator,
    engine::Local,
    runtime::{PidRegistry, PortAllocator, QUICK_SHARE_PORTS, Supervisor, TUNNEL_PORTS},
    secrets::MemoryStore,
};

#[derive(Default)]
struct FakeUi {
    answer: Mutex<Option<Decision>>,
    prompts: Mutex<Vec<Prompt>>,
    opened: Mutex<Vec<View>>,
    changes: Mutex<Vec<Changed>>,
}

impl Ui for FakeUi {
    fn confirm(&self, prompt: Prompt) -> BoxFuture<'_, Decision> {
        Box::pin(async move {
            self.prompts.lock().unwrap().push(prompt);
            self.answer.lock().unwrap().unwrap_or(Decision::Deny)
        })
    }

    fn open(&self, view: View) {
        self.opened.lock().unwrap().push(view);
    }

    fn changed(&self, change: Changed) {
        self.changes.lock().unwrap().push(change);
    }
}

struct Fixture {
    host: Arc<CoreHost>,
    ui: Arc<FakeUi>,
    store: Store,
    dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open_in_memory().unwrap();
    let supervisor = Supervisor::new(
        PidRegistry::new(dir.path().join("run")),
        tokio::runtime::Handle::current(),
    );
    // No cloudflared anywhere: nothing can really start.
    let binary = BinaryManager::new(Locator::new(dir.path().join("bin"), None, vec![]));
    let secrets: crate::secrets::Secrets = Arc::new(MemoryStore::default());
    let local = Local::new(store.clone());
    let parts = HostParts {
        version: "1.2.3".into(),
        accounts: Accounts::new(store.clone(), Arc::clone(&secrets)),
        engine: Arc::new(Engine::new(local.clone())),
        machine: MachineTunnels::new(
            supervisor.clone(),
            binary.clone(),
            PortAllocator::new(TUNNEL_PORTS),
            secrets,
            local,
        ),
        quick_shares: QuickShares::new(
            supervisor,
            binary.clone(),
            PortAllocator::new(QUICK_SHARE_PORTS),
            store.clone(),
            dir.path().join("quick-share.yml"),
        )
        .with_inspector(crate::inspect::Inspector::new(None, None, "test")),
        binary,
        runs: dir.path().join("run-cli"),
        machine_name: "test-machine".into(),
        local_domains: Some(crate::local_domains::LocalDomains::new(
            store.clone(),
            crate::inspect::Inspector::new(None, None, "test"),
            crate::local_domains::LocalDomainsConfig::isolated(&dir.path().join("local")),
        )),
        inspector: crate::inspect::Inspector::new(
            Some(store.clone()),
            None,
            crate::domain_shares::APP_OWNER,
        ),
        pauses: Arc::new(crate::pause::Enforcer::new()),
        store: store.clone(),
    };
    let ui = Arc::new(FakeUi::default());
    let host = CoreHost::new(parts, ui.clone());
    Fixture {
        host,
        ui,
        store,
        dir,
    }
}

fn client() -> ClientInfo {
    ClientInfo {
        name: "vscode".into(),
        version: "1.0.0".into(),
    }
}

fn start(origin: &str) -> StartShare {
    StartShare {
        origin: origin.into(),
        stop_after_seconds: None,
        host_header: HostHeader::Auto,
        folder: None,
    }
}

#[tokio::test]
async fn reports_an_empty_app() {
    let f = fixture();
    let status = f.host.status().await.unwrap();
    assert_eq!(status.app.version, "1.2.3");
    assert!(status.accounts.is_empty() && status.tunnels.is_empty() && status.shares.is_empty());
    let error = f.host.routes(RoutesParams::default()).await.unwrap_err();
    assert_eq!(error.code, code::NOT_FOUND);
    assert_eq!(error.message, "No Cloudflare account is connected.");
}

#[tokio::test]
async fn serves_local_domains_the_cli_added_after_a_reload() {
    let f = fixture();
    assert!(f.host.local_domains().await.unwrap().domains.is_empty());
    // The CLI writes the registry, then asks the app to serve it.
    crate::local_domains::registry::save(
        &f.store,
        &crate::local_domains::LocalDomainRow {
            name: localdomains::LocalName::parse_any("cli.localhost").unwrap(),
            target: localdomains::DomainTarget::Port { port: 3000 },
            wildcard: false,
            https: true,
            inspect: false,
            project: None,
            created_at: 1,
        },
    )
    .await
    .unwrap();
    let listed = f.host.local_domains().await.unwrap();
    assert!(!listed.running && !listed.domains[0].serving);
    let reloaded = f.host.reload_local_domains().await.unwrap();
    assert!(reloaded.running, "{:?}", reloaded.error);
    assert!(reloaded.domains[0].serving);
    assert!(reloaded.domains[0].url.starts_with("https://cli.localhost"));
    assert!(
        f.ui.prompts.lock().unwrap().is_empty(),
        "no approval needed"
    );
}

#[tokio::test]
async fn explains_what_it_cant_do() {
    let f = fixture();
    let error = f.host.start_share(start("3000")).await.unwrap_err();
    assert_eq!(
        error.message,
        "cloudflared isn't installed. Open Teitunnel to install it."
    );
    let error = f.host.start_share(start("not a port")).await.unwrap_err();
    assert_eq!(error.code, code::INVALID_PARAMS);
    // A folder is checked again by the app: a real one, not the home folder.
    let folder = |path: &str| StartShare {
        folder: Some(teitunnel_control::protocol::ShareFolder {
            path: path.into(),
            listing: None,
            spa: false,
        }),
        ..start(path)
    };
    let missing = f.dir.path().join("nope").display().to_string();
    let error = f.host.start_share(folder(&missing)).await.unwrap_err();
    assert_eq!(error.code, code::INVALID_PARAMS);
    if let Some(home) = std::env::home_dir() {
        let error = f
            .host
            .start_share(folder(&home.display().to_string()))
            .await
            .unwrap_err();
        assert_eq!(error.code, code::INVALID_PARAMS, "{}", error.message);
    }
    let site = f.dir.path().join("site");
    std::fs::create_dir(&site).unwrap();
    let error = f
        .host
        .start_share(folder(&site.display().to_string()))
        .await
        .unwrap_err();
    assert_eq!(
        error.message,
        "cloudflared isn't installed. Open Teitunnel to install it."
    );
    let error = f
        .host
        .stop_share(StopShare {
            id: "https://nope.trycloudflare.com".into(),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, code::NOT_FOUND);
    let error = f
        .host
        .preview(PreviewParams {
            account: None,
            tunnel: None,
            change: json!({"type": "launchRockets"}),
        })
        .await
        .unwrap_err();
    assert_eq!(error.code, code::INVALID_PARAMS);
    assert!(
        error
            .message
            .starts_with("That isn't a change Teitunnel knows")
    );
}

#[tokio::test]
async fn asks_in_the_persons_words() {
    let f = fixture();
    *f.ui.answer.lock().unwrap() = Some(Decision::Always);
    let decision = f
        .host
        .confirm(ConfirmRequest {
            requester: Requester::Client(client()),
            action: Action::StartShare(start("5173")),
            offer_always: true,
        })
        .await;
    assert_eq!(decision, Decision::Always);
    // A link is never allowed for good, whatever the dialog answered.
    let decision = f
        .host
        .confirm(ConfirmRequest {
            requester: Requester::Link,
            action: Action::StartShare(start("5173")),
            offer_always: false,
        })
        .await;
    assert_eq!(decision, Decision::Once);
    f.host
        .confirm(ConfirmRequest {
            requester: Requester::Client(client()),
            action: Action::StartShare(StartShare {
                folder: Some(teitunnel_control::protocol::ShareFolder {
                    path: "/Users/me/site".into(),
                    listing: None,
                    spa: false,
                }),
                ..start("/Users/me/site")
            }),
            offer_always: false,
        })
        .await;
    f.host
        .confirm(ConfirmRequest {
            requester: Requester::Client(client()),
            action: Action::Apply(ApplyParams {
                account: None,
                tunnel: None,
                change: json!({"type": "removeRoute", "hostname": "a.example.com"}),
                fingerprint: "f".into(),
                confirmed: true,
            }),
            offer_always: false,
        })
        .await;

    let prompts = f.ui.prompts.lock().unwrap();
    assert_eq!(
        prompts[0].title.english(),
        "Allow vscode to make this change?"
    );
    assert_eq!(
        prompts[0].message.english(),
        "vscode wants to share http://localhost:5173 at a public address. Anyone with the address can open it."
    );
    assert_eq!(
        prompts[0].always.as_ref().map(Text::english).as_deref(),
        Some("Always Allow")
    );
    assert_eq!(prompts[1].title.english(), "Share from a link?");
    assert!(
        prompts[1]
            .message
            .english()
            .contains("Only allow it if you opened the link on purpose")
    );
    assert_eq!(prompts[1].allow.english(), "Share");
    assert_eq!(prompts[1].always, None);
    assert_eq!(prompts[1].deny.english(), "Cancel");
    assert_eq!(
        prompts[2].message.english(),
        "vscode wants to share the files in /Users/me/site at a public address. Anyone with the address can open them."
    );
    // Without an account the plan can't be described in detail.
    assert_eq!(
        prompts[3].message.english(),
        "vscode wants to change your routes in Cloudflare."
    );
    assert_eq!(prompts[3].always, None);
}

#[tokio::test]
async fn remembers_approved_programs_in_the_store() {
    let f = fixture();
    assert!(!f.host.is_approved(&client()).await);
    f.host.approve(&client()).await;
    assert!(f.host.is_approved(&client()).await);
    let settings = integrations::load(&f.store).await.unwrap();
    assert_eq!(settings.clients[0].name, "vscode");
}

#[tokio::test]
async fn opens_views_and_publishes_events() {
    let f = fixture();
    f.host.open(View::Doctor).await.unwrap();
    assert_eq!(f.ui.opened.lock().unwrap()[0], View::Doctor);
    let mut events = f.host.subscribe();
    f.host.publish(
        event_for(&Changed::Routes {
            account_id: "a1".into(),
        })
        .unwrap(),
    );
    assert_eq!(event_for(&Changed::Agents), None);
    assert_eq!(
        events.recv().await.unwrap(),
        Event::RoutesChanged {
            account_id: Some("a1".into())
        }
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn pauses_a_share_the_app_serves() {
    let f = fixture();
    let origin = crate::inspect::tests::origin().await;
    Local::new(f.store.clone())
        .record_share(&crate::domain_shares::DomainShare {
            account_id: "a1".into(),
            hostname: "demo.xyz.com".into(),
            origin: origin.clone(),
            owner: crate::domain_shares::APP_OWNER.into(),
            expires_at: None,
            created_at: 1,
            source: None,
            folder: false,
            paused: false,
            schedule: None,
        })
        .await
        .unwrap();
    let inspector = f.host.parts.inspector.clone();
    let tap = inspector
        .start(crate::inspect::TapSpec::new(
            crate::inspect::TapScope::route("a1", "demo.xyz.com", None),
            "demo.xyz.com",
            &origin,
        ))
        .await
        .unwrap();
    let pause = |id: &str| wire::PauseShare {
        id: id.into(),
        account: None,
    };
    f.host
        .pause_share(pause("https://Demo.xyz.com/"), true)
        .await
        .unwrap();
    assert!(inspector.view(&tap.id).unwrap().paused.is_some());
    let shares = f.host.shares().await.unwrap();
    assert!(shares[0].paused);
    assert_eq!(shares[0].status, "paused");
    assert!(f.ui.changes.lock().unwrap().contains(&Changed::Shares));
    f.host
        .pause_share(pause("demo.xyz.com"), false)
        .await
        .unwrap();
    assert!(inspector.view(&tap.id).unwrap().paused.is_none());
    assert!(!f.host.shares().await.unwrap()[0].paused);
    // A route with nothing to pause it (no account connected here) is refused.
    assert!(
        f.host
            .pause_share(pause("other.xyz.com"), true)
            .await
            .is_err()
    );
    inspector.shutdown().await;
}

#[tokio::test]
async fn lists_agents_and_asks_for_their_changes() {
    let f = fixture();
    let agent = AgentInfo {
        name: "claude-code".into(),
        version: Some("2.1".into()),
        mode: "ask".into(),
    };
    f.host.agent_connected(7, agent, &client());
    assert_eq!(f.host.agents()[0].name, "claude-code");
    let question = AgentApproval {
        agent: "someone".into(),
        title: "Add app.example.com".into(),
        details: "• Create a DNS record".into(),
    };
    *f.ui.answer.lock().unwrap() = Some(Decision::Once);
    assert!(f.host.approve_for_agent(7, question.clone()).await);
    *f.ui.answer.lock().unwrap() = Some(Decision::Deny);
    assert!(!f.host.approve_for_agent(7, question).await);
    {
        let prompts = f.ui.prompts.lock().unwrap();
        assert!(
            prompts[0].title.english().contains("claude-code"),
            "the registered name"
        );
        assert!(prompts[0].message.english().contains("Create a DNS record"));
        assert!(
            prompts[0].always.is_none(),
            "an agent is never allowed for good"
        );
    }
    assert!(f.host.pending_approvals().is_empty());
    f.host.agent_disconnected(7);
    assert!(f.host.agents().is_empty());
    assert!(f.ui.changes.lock().unwrap().contains(&Changed::Agents));
}

#[tokio::test]
async fn serves_the_cli_over_the_control_connection() {
    let f = fixture();
    let endpoint = Endpoint::new(f.dir.path());
    let token = endpoint.ensure_token().unwrap();
    let listener = endpoint.listen().await.unwrap();
    let host: Arc<dyn Host> = f.host.clone();
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    tokio::spawn(Server::new(host, token).run(listener, async {
        let _ = stopped.await;
    }));
    let cli = ControlClient::connect(
        &endpoint,
        ClientInfo {
            name: "teitunnel-cli".into(),
            version: "1".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(cli.hello().app.version, "1.2.3");
    assert!(cli.status().await.unwrap().shares.is_empty());
    // Declined by default: the share isn't even attempted.
    let declined = cli.start_share(&start("3000")).await.unwrap_err();
    assert_eq!(declined.code(), Some(code::DECLINED));
    assert!(f.ui.changes.lock().unwrap().is_empty());
    let _ = stop.send(());
}

#[tokio::test(flavor = "multi_thread")]
async fn forwards_inspected_requests_bounded_and_masked() {
    use crate::inspect::{
        Inspector, TapScope, TapSpec,
        tests::{origin, send},
    };
    let f = fixture();
    let inspector = Inspector::new(None, None, "app");
    tokio::spawn(Arc::clone(&f.host).forward_requests(inspector.clone()));
    // It waits for Lens instead of starting it.
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(inspector.running().is_none());

    let mut events = f.host.subscribe();
    let origin = origin().await;
    let tap = inspector
        .start(TapSpec::new(
            TapScope::QuickShare {
                share_id: "qs-ext".into(),
            },
            "demo",
            &origin,
        ))
        .await
        .unwrap();
    send(&tap.address, "POST", "/hook?token=hunter2", &[]).await;
    let event = tokio::time::timeout(Duration::from_secs(5), events.recv())
        .await
        .unwrap()
        .unwrap();
    let Event::RequestArrived {
        share,
        method,
        path,
        status,
        duration_ms,
    } = event
    else {
        panic!("expected requestArrived, got {event:?}");
    };
    assert_eq!((share.as_str(), method.as_str()), ("qs-ext", "POST"));
    assert!(path.starts_with("/hook?token="), "{path}");
    assert!(
        !path.contains("hunter2"),
        "masked like the inspector: {path}"
    );
    assert_eq!(status, Some(200));
    assert!(duration_ms.is_some());

    // A burst arrives in bounded batches, not one event per request.
    let burst: Vec<_> = (0..40)
        .map(|i| {
            let address = tap.address.clone();
            tokio::spawn(async move { send(&address, "GET", &format!("/burst/{i}"), &[]).await })
        })
        .collect();
    for request in burst {
        request.await.unwrap();
    }
    tokio::time::sleep(requests::REQUEST_TICK * 3).await;
    let mut received = 0;
    while let Ok(event) = events.try_recv() {
        assert!(matches!(event, Event::RequestArrived { .. }));
        received += 1;
    }
    assert!(received >= 1, "some of the burst arrives");
    assert!(received < 40, "coalesced: {received}");
    inspector.shutdown().await;
}
