use std::sync::Mutex;

use serde_json::json;
use teitunnel_control::{
    ControlClient, Endpoint, Server,
    protocol::{ApplyParams, HostHeader, StartShare, StopShare, View},
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
        ),
        binary,
        runs: dir.path().join("run-cli"),
        machine_name: "test-machine".into(),
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
async fn explains_what_it_cant_do() {
    let f = fixture();
    let error = f.host.start_share(start("3000")).await.unwrap_err();
    assert_eq!(
        error.message,
        "cloudflared isn't installed. Open Teitunnel to install it."
    );
    let error = f.host.start_share(start("not a port")).await.unwrap_err();
    assert_eq!(error.code, code::INVALID_PARAMS);
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
    // Without an account the plan can't be described in detail.
    assert_eq!(
        prompts[2].message.english(),
        "vscode wants to change your routes in Cloudflare."
    );
    assert_eq!(prompts[2].always, None);
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
    f.host.publish(event_for(&Changed::Routes {
        account_id: "a1".into(),
    }));
    assert_eq!(
        events.recv().await.unwrap(),
        Event::RoutesChanged {
            account_id: Some("a1".into())
        }
    );
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
