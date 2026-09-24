//! The Snapshot service against the fake Cloudflare: preparing, publishing, versions,
//! rollback, deletion, and the local record kept in step with each outcome.

use super::*;
use crate::{
    engine::{
        Local, ZoneRef,
        fake::{CloudState, FakeCloud, FakeConnectors},
    },
    store::Store,
};

const CTX: Context<'static> = Context {
    account: "acc",
    machine_name: "Mac",
    tunnel: None,
};

fn cloud() -> FakeCloud {
    FakeCloud::new(CloudState {
        zones: vec![ZoneRef {
            id: "z".into(),
            name: "xyz.com".into(),
        }],
        workers_subdomain: Some("acme".into()),
        ..CloudState::default()
    })
}

fn folder(files: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    for (path, text) in files {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
    dir
}

fn options() -> SnapshotOptions {
    SnapshotOptions {
        spa: false,
        password: PasswordInput::Keep,
        access: None,
        expires_in_days: None,
    }
}

struct Setup {
    engine: Engine,
    cloud: FakeCloud,
    preparations: Preparations,
}

impl Setup {
    fn new() -> Self {
        Self {
            engine: Engine::new(Local::new(Store::open_in_memory().unwrap())),
            cloud: cloud(),
            preparations: Preparations::default(),
        }
    }

    async fn go(&self, change: &SnapshotChange) -> Result<Outcome, SnapshotError> {
        let plan = preview(&self.engine, &self.cloud, &self.preparations, CTX, change).await?;
        apply(
            &self.engine,
            &self.cloud,
            &FakeConnectors::default(),
            &self.preparations,
            CTX,
            "app",
            change,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: plan.requires_confirmation,
            },
            |_| {},
        )
        .await
    }
}

#[test]
fn names_become_worker_names() {
    assert_eq!(slug("  My Demo! v2 "), "my-demo-v2");
    assert_eq!(script_for("Launch page"), "teitunnel-launch-page");
    assert_eq!(script_for(&"x".repeat(80)).len(), 63);
    assert_eq!(valid_name(" Demo ").unwrap(), "Demo");
    assert!(matches!(valid_name(""), Err(SnapshotError::InvalidName)));
    assert!(matches!(valid_name("!!!"), Err(SnapshotError::InvalidName)));
    assert!(matches!(
        valid_name(&"a".repeat(41)),
        Err(SnapshotError::InvalidName)
    ));
}

#[test]
fn passwords_never_show_in_debug_output() {
    let input = PasswordInput::Set {
        password: "hunter22".into(),
    };
    assert_eq!(format!("{input:?}"), "Set([redacted])");
}

#[tokio::test]
async fn publishes_a_folder_then_updates_rolls_back_and_deletes_it() {
    let s = Setup::new();
    let dir = folder(&[
        ("index.html", "<h1>v1</h1>"),
        ("app.js", "1"),
        (".env", "SECRET=1"),
    ]);
    let prepared = s.preparations.folder(dir.path()).await.unwrap();
    assert_eq!(prepared.files, 2);
    assert_eq!(prepared.skipped.len(), 1, "the .env file is left out");
    assert!(prepared.single_page);

    let publish = SnapshotChange::Publish {
        prepared: prepared.id.clone(),
        name: "Demo".into(),
        address: AddressInput::WorkersDev,
        options: SnapshotOptions {
            expires_in_days: Some(7),
            ..options()
        },
    };
    let outcome = s.go(&publish).await.unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert!(s.preparations.get(&prepared.id).is_none(), "used up");
    let all = list(&s.engine, Some("acc")).await.unwrap();
    assert_eq!(all.len(), 1);
    let snapshot = &all[0];
    assert_eq!(snapshot.url, "https://teitunnel-demo.acme.workers.dev");
    assert!(snapshot.workers_dev);
    assert_eq!((snapshot.live_version, snapshot.files), (Some(1), 2));
    assert!(snapshot.expires_at.is_some());
    assert!(matches!(
        snapshot.source,
        Some(SnapshotSource::Folder { .. })
    ));
    assert!(s.cloud.snapshot().workers["teitunnel-demo"].workers_dev);

    // The same name again is refused before anything happens.
    let again = s.preparations.folder(dir.path()).await.unwrap();
    let taken = s
        .go(&SnapshotChange::Publish {
            prepared: again.id,
            name: "demo".into(),
            address: AddressInput::WorkersDev,
            options: options(),
        })
        .await;
    assert!(
        matches!(taken, Err(SnapshotError::NameTaken(_))),
        "{taken:?}"
    );

    // Nothing changed: refused.
    let unchanged = s
        .go(&SnapshotChange::Update {
            snapshot: snapshot.id.clone(),
            prepared: None,
            options: options(),
        })
        .await;
    assert!(
        matches!(unchanged, Err(SnapshotError::Unchanged)),
        "{unchanged:?}"
    );

    // New files and a password.
    std::fs::write(dir.path().join("index.html"), "<h1>v2</h1>").unwrap();
    let prepared = s.preparations.folder(dir.path()).await.unwrap();
    let outcome = s
        .go(&SnapshotChange::Update {
            snapshot: snapshot.id.clone(),
            prepared: Some(prepared.id),
            options: SnapshotOptions {
                password: PasswordInput::Set {
                    password: "open sesame".into(),
                },
                ..options()
            },
        })
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let listed = find(&s.engine, "acc", "demo").await.unwrap();
    assert_eq!(listed.live_version, Some(2));
    assert!(listed.password);
    let kept = versions(&s.engine, &listed.id).await.unwrap();
    assert_eq!(
        kept.iter()
            .map(|v| (v.number, v.live, v.password))
            .collect::<Vec<_>>(),
        [(2, true, true), (1, false, false)]
    );

    // Roll back to 1: its settings (no password) come back too.
    let outcome = s
        .go(&SnapshotChange::Rollback {
            snapshot: listed.id.clone(),
            version: 1,
        })
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let listed = find(&s.engine, "acc", "teitunnel-demo.acme.workers.dev")
        .await
        .unwrap();
    assert_eq!(listed.live_version, Some(1));
    assert!(!listed.password);
    assert!(matches!(
        s.go(&SnapshotChange::Rollback {
            snapshot: listed.id.clone(),
            version: 9,
        })
        .await,
        Err(SnapshotError::NoSuchVersion(9))
    ));

    // Delete: gone from Cloudflare and forgotten here.
    delete_now(
        &s.engine,
        &s.cloud,
        &FakeConnectors::default(),
        &s.preparations,
        CTX,
        &listed.id,
    )
    .await
    .unwrap();
    assert!(s.cloud.snapshot().workers.is_empty());
    assert!(list(&s.engine, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn a_publish_that_rolls_back_leaves_nothing_behind() {
    let s = Setup::new();
    let dir = folder(&[("index.html", "hi")]);
    let prepared = s.preparations.folder(dir.path()).await.unwrap();
    let change = SnapshotChange::Publish {
        prepared: prepared.id,
        name: "Launch".into(),
        address: AddressInput::Domain {
            hostname: "launch.xyz.com".into(),
        },
        options: options(),
    };
    let plan = preview(&s.engine, &s.cloud, &s.preparations, CTX, &change)
        .await
        .unwrap();
    // session (0), bucket (1), worker (2), attaching the domain (3) fails.
    s.cloud.fail_once(3);
    let outcome = apply(
        &s.engine,
        &s.cloud,
        &FakeConnectors::default(),
        &s.preparations,
        CTX,
        "app",
        &change,
        Approval {
            fingerprint: &plan.fingerprint,
            confirmed: false,
        },
        |_| {},
    )
    .await
    .unwrap();
    assert!(matches!(outcome, Outcome::RolledBack { .. }), "{outcome:?}");
    assert!(s.cloud.snapshot().workers.is_empty());
    assert!(s.cloud.snapshot().worker_domains.is_empty());
    assert!(list(&s.engine, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn refuses_bad_input_before_planning() {
    let s = Setup::new();
    let dir = folder(&[("index.html", "hi")]);
    let prepared = s.preparations.folder(dir.path()).await.unwrap();
    let publish = |address: AddressInput, options: SnapshotOptions| SnapshotChange::Publish {
        prepared: prepared.id.clone(),
        name: "Demo".into(),
        address,
        options,
    };
    let wildcard = s
        .go(&publish(
            AddressInput::Domain {
                hostname: "*.xyz.com".into(),
            },
            options(),
        ))
        .await;
    assert!(
        matches!(wildcard, Err(SnapshotError::InvalidHostname(_))),
        "{wildcard:?}"
    );
    let short = s
        .go(&publish(
            AddressInput::WorkersDev,
            SnapshotOptions {
                password: PasswordInput::Set {
                    password: "123".into(),
                },
                ..options()
            },
        ))
        .await;
    assert!(
        matches!(short, Err(SnapshotError::PasswordTooShort(_))),
        "{short:?}"
    );
    let gone = s
        .go(&SnapshotChange::Publish {
            prepared: "nope".into(),
            name: "Demo".into(),
            address: AddressInput::WorkersDev,
            options: options(),
        })
        .await;
    assert!(matches!(gone, Err(SnapshotError::NotPrepared)), "{gone:?}");
    assert!(list(&s.engine, None).await.unwrap().is_empty());
}

#[tokio::test]
async fn builds_a_plain_html_project_without_running_anything() {
    let s = Setup::new();
    let dir = folder(&[("index.html", "hi"), ("about.html", "about")]);
    let project = build::detect(dir.path()).unwrap();
    let prepared = s.preparations.build(&project, |_| {}).await.unwrap();
    assert_eq!(prepared.files, 2);
    assert!(!prepared.single_page);
    assert!(matches!(prepared.source, SnapshotSource::Folder { .. }));
}
