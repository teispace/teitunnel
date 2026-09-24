//! Snapshot plans and their execution against the fake Cloudflare: order of steps,
//! conflicts, incremental uploads, versions and rollback. The failure-at-every-step
//! scenarios live with the others in `executor_tests`.

use std::path::Path;

use super::{
    access::AccessRule,
    executor::{Approval, Context, Engine, Outcome, Progress, StepState},
    fake::{CloudState, FakeCloud, FakeConnectors},
    local::Local,
    planner::{PlanError, plan},
    sites::{Password, SiteAddress, SiteContent, SiteSettings, SiteSpec, SiteState},
    types::{Intent, ObservedRecord, Plan, Snapshot, Step, ZoneRef},
};
use crate::{Secret, domain::Hostname, store::Store};

const CTX: Context<'static> = Context {
    account: "acc",
    machine_name: "Mac",
    tunnel: None,
};

/// Files written to a folder that outlives the test (plans read them when applied).
pub(super) fn content(files: &[(&str, &str)]) -> SiteContent {
    let dir = tempfile::tempdir().unwrap().keep();
    write(&dir, files);
    crate::snapshot::content::collect(&dir).unwrap().content
}

pub(super) fn write(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, text).unwrap();
    }
}

pub(super) fn site(name: &str, hostname: Option<&str>) -> SiteSpec {
    SiteSpec {
        id: format!("s-{name}"),
        name: name.into(),
        script: format!("teitunnel-{name}"),
        address: hostname.map_or(SiteAddress::WorkersDev, |h| SiteAddress::Domain {
            hostname: Hostname::parse(h).unwrap(),
        }),
        access: None,
    }
}

pub(super) fn me() -> AccessRule {
    AccessRule {
        emails: vec!["me@xyz.com".into()],
        email_domains: Vec::new(),
    }
}

pub(super) fn publish(site: SiteSpec, content: SiteContent) -> Intent {
    Intent::PublishSnapshot {
        site,
        settings: SiteSettings::default(),
        content,
    }
}

pub(super) fn update(site: SiteSpec, content: SiteContent, password: Password) -> Intent {
    Intent::UpdateSnapshot {
        site,
        settings: SiteSettings {
            spa: true,
            password,
            overlay: None,
        },
        content,
        previous: Vec::new(),
    }
}

/// An account with one domain, Zero Trust, and a workers.dev subdomain.
pub(super) fn account() -> CloudState {
    CloudState {
        zones: vec![ZoneRef {
            id: "z-xyz".into(),
            name: "xyz.com".into(),
        }],
        access_org: true,
        workers_subdomain: Some("acme".into()),
        ..CloudState::default()
    }
}

fn engine() -> Engine {
    Engine::new(Local::new(Store::open_in_memory().unwrap()))
}

async fn run(engine: &Engine, cloud: &FakeCloud, intent: &Intent) -> (Outcome, Vec<Progress>) {
    let plan = engine.preview(cloud, CTX, intent).await.unwrap();
    let mut progress = Vec::new();
    let outcome = engine
        .apply(
            cloud,
            &FakeConnectors::default(),
            CTX,
            intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: true,
            },
            |p| progress.push(p),
        )
        .await
        .unwrap();
    (outcome, progress)
}

fn kinds(plan: &Plan) -> Vec<&'static str> {
    plan.steps
        .iter()
        .map(|step| match step {
            Step::UploadSnapshotFiles { .. } => "upload",
            Step::CreateSnapshotWorker { .. } => "worker+",
            Step::PublishSnapshotVersion { .. } => "version+",
            Step::RollBackSnapshot { .. } => "version<",
            Step::EnableWorkersDev { .. } => "dev+",
            Step::DisableWorkersDev { .. } => "dev-",
            Step::AttachSnapshotDomain { .. } => "domain+",
            Step::DetachSnapshotDomain { .. } => "domain-",
            Step::DeleteSnapshotWorker { .. } => "worker-",
            Step::AddLoginMethod => "login",
            Step::CreateAccessApp { .. } => "app+",
            Step::UpdateAccessApp { .. } => "app~",
            Step::DeleteAccessApp { .. } => "app-",
            Step::DeleteRecord { .. } => "dns-",
            _ => "other",
        })
        .collect()
}

/// An observation with a Snapshot's Worker as given.
fn observed(state: SiteState, records: Vec<ObservedRecord>) -> Snapshot {
    Snapshot {
        account_id: "acc".into(),
        machine_name: "Mac".into(),
        zones: account().zones,
        tunnel: None,
        tunnel_names: Vec::new(),
        elsewhere: Vec::new(),
        records,
        access: None,
        networks: None,
        balance: None,
        site: Some(state),
    }
}

fn fresh(script: &str) -> SiteState {
    SiteState {
        script: script.into(),
        exists: false,
        active_version: None,
        workers_dev: false,
        subdomain: Some("acme".into()),
        domains: Vec::new(),
        hostname_taken_by: None,
    }
}

#[test]
fn plans_files_then_worker_then_login_then_address() {
    let mut spec = site("demo", Some("preview.xyz.com"));
    spec.access = Some(me());
    let intent = publish(spec, content(&[("index.html", "hi")]));
    let mut snapshot = observed(fresh("teitunnel-demo"), Vec::new());
    snapshot.access = Some(super::access::AccessState {
        organization: Some(true),
        login_methods: Some(0),
        apps: Vec::new(),
    });
    let p = plan(&intent, &snapshot).unwrap();
    assert_eq!(kinds(&p), ["upload", "worker+", "login", "app+", "domain+"]);
    assert!(!p.requires_confirmation);

    let intent = publish(site("demo", None), content(&[("index.html", "hi")]));
    let p = plan(&intent, &observed(fresh("teitunnel-demo"), Vec::new())).unwrap();
    assert_eq!(kinds(&p), ["upload", "worker+", "dev+"]);
    let Step::EnableWorkersDev { address, .. } = &p.steps[2] else {
        unreachable!()
    };
    assert_eq!(address, "teitunnel-demo.acme.workers.dev");
}

#[test]
fn never_takes_a_hostname_that_is_in_use() {
    let intent = publish(
        site("demo", Some("www.xyz.com")),
        content(&[("a.txt", "a")]),
    );
    let record = |owned: bool| ObservedRecord {
        zone_id: "z-xyz".into(),
        owned,
        record: cf_api::DnsRecord {
            id: "r1".into(),
            name: "www.xyz.com".into(),
            kind: "A".into(),
            content: "192.0.2.1".into(),
            proxied: false,
            comment: None,
            ttl: 300,
        },
    };
    // Someone else's record: deleted only with confirmation (Cloudflare won't attach over it).
    let p = plan(
        &intent,
        &observed(fresh("teitunnel-demo"), vec![record(false)]),
    )
    .unwrap();
    assert_eq!(kinds(&p), ["upload", "worker+", "dns-", "domain+"]);
    assert!(p.requires_confirmation);
    // A route's record: refused.
    assert_eq!(
        plan(
            &intent,
            &observed(fresh("teitunnel-demo"), vec![record(true)])
        ),
        Err(PlanError::HostnameRouted("www.xyz.com".into()))
    );
    // Another Worker already serves it.
    let mut taken = fresh("teitunnel-demo");
    taken.hostname_taken_by = Some("their-worker".into());
    assert_eq!(
        plan(&intent, &observed(taken, Vec::new())),
        Err(PlanError::HostnameServed {
            hostname: "www.xyz.com".into(),
            worker: "their-worker".into()
        })
    );
    // A name that's already a Worker.
    let mut exists = fresh("teitunnel-demo");
    exists.exists = true;
    assert_eq!(
        plan(&intent, &observed(exists, Vec::new())),
        Err(PlanError::SnapshotExists("demo".into()))
    );
    // Not one of the account's domains.
    let elsewhere = publish(
        site("demo", Some("www.other.org")),
        content(&[("a.txt", "a")]),
    );
    assert_eq!(
        plan(&elsewhere, &observed(fresh("teitunnel-demo"), Vec::new())),
        Err(PlanError::NoZone("www.other.org".into()))
    );
}

#[test]
fn workers_dev_needs_a_subdomain_and_logins_need_a_domain() {
    let intent = publish(site("demo", None), content(&[("a.txt", "a")]));
    let mut none = fresh("teitunnel-demo");
    none.subdomain = None;
    assert_eq!(
        plan(&intent, &observed(none, Vec::new())),
        Err(PlanError::NoWorkersSubdomain)
    );
    let mut spec = site("demo", None);
    spec.access = Some(me());
    assert_eq!(
        plan(
            &publish(spec, content(&[("a.txt", "a")])),
            &observed(fresh("teitunnel-demo"), Vec::new())
        ),
        Err(PlanError::SnapshotLoginNeedsDomain)
    );
}

#[tokio::test]
async fn publishes_updates_rolls_back_and_deletes() {
    let (engine, cloud) = (engine(), FakeCloud::new(account()));
    let spec = site("demo", Some("preview.xyz.com"));
    let first = content(&[
        ("index.html", "v1"),
        ("app.js", "same"),
        ("logo.svg", "<svg/>"),
    ]);
    let (outcome, _) = run(&engine, &cloud, &publish(spec.clone(), first.clone())).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    let worker = &state.workers["teitunnel-demo"];
    let v1 = worker.active.clone();
    assert_eq!(worker.live().unwrap().assets.len(), 3);
    assert_eq!(
        state
            .worker_domains
            .values()
            .map(|d| d.hostname.as_str())
            .collect::<Vec<_>>(),
        ["preview.xyz.com"]
    );

    // Only the changed file travels; the new version goes live in one switch.
    let second = content(&[
        ("index.html", "v2"),
        ("app.js", "same"),
        ("logo.svg", "<svg/>"),
    ]);
    let hash = Secret::new("pbkdf2-sha256$1$c2FsdA$aGFzaA".to_owned());
    let (outcome, progress) = run(
        &engine,
        &cloud,
        &update(spec.clone(), second, Password::Set { hash }),
    )
    .await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let sent = progress.iter().rev().find_map(|p| match p.state {
        StepState::Transferring {
            files, total_files, ..
        } => Some((files, total_files)),
        _ => None,
    });
    assert_eq!(sent, Some((1, 1)));
    let state = cloud.snapshot();
    let worker = &state.workers["teitunnel-demo"];
    let live = worker.live().unwrap();
    assert_ne!(worker.active, v1);
    assert_eq!(live.metadata["assets"]["config"]["run_worker_first"], true);
    assert_eq!(live.metadata["bindings"][1]["type"], "secret_text");

    // Settings only (keep the password, no SPA): the files are the live version's, and
    // nothing is uploaded again.
    let kept = SiteContent {
        root: None,
        ..content(&[
            ("index.html", "v2"),
            ("app.js", "same"),
            ("logo.svg", "<svg/>"),
        ])
    };
    let (outcome, progress) =
        run(&engine, &cloud, &update(spec.clone(), kept, Password::Keep)).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert!(!progress.iter().any(
        |p| matches!(p.state, StepState::Transferring { total_files, .. } if total_files > 0)
    ));
    let state = cloud.snapshot();
    let live = state.workers["teitunnel-demo"].live().unwrap().clone();
    assert_eq!(live.metadata["bindings"][1]["type"], "secret_text", "kept");

    // Back to the first version; again is nothing to do.
    let rollback = Intent::RollbackSnapshot {
        site: spec.clone(),
        version_id: v1.clone(),
        number: 1,
    };
    let (outcome, _) = run(&engine, &cloud, &rollback).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    assert_eq!(cloud.snapshot().workers["teitunnel-demo"].active, v1);
    assert!(
        engine
            .preview(&cloud, CTX, &rollback)
            .await
            .unwrap()
            .is_empty()
    );

    // Deleting removes the address, then the Worker.
    let delete = Intent::DeleteSnapshot { site: spec };
    let p = engine.preview(&cloud, CTX, &delete).await.unwrap();
    assert_eq!(kinds(&p), ["domain-", "worker-"]);
    let (outcome, _) = run(&engine, &cloud, &delete).await;
    assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    let state = cloud.snapshot();
    assert!(state.workers.is_empty() && state.worker_domains.is_empty());
    // Deleting what's gone plans nothing.
    assert!(
        engine
            .preview(&cloud, CTX, &delete)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn a_file_changed_after_the_preview_is_never_published() {
    let (engine, cloud) = (engine(), FakeCloud::new(account()));
    let files = content(&[("index.html", "reviewed")]);
    let root = files.root.clone().unwrap();
    let intent = publish(site("demo", None), files);
    let plan = engine.preview(&cloud, CTX, &intent).await.unwrap();
    write(&root, &[("index.html", "something else")]);
    let outcome = engine
        .apply(
            &cloud,
            &FakeConnectors::default(),
            CTX,
            &intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .unwrap();
    let Outcome::RolledBack { error, .. } = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(error.key, "core.snapshot.error.changed");
    assert!(cloud.snapshot().workers.is_empty());
}

#[tokio::test]
async fn a_failed_update_puts_the_previous_version_back() {
    let (engine, cloud) = (engine(), FakeCloud::new(account()));
    let spec = site("demo", None);
    run(
        &engine,
        &cloud,
        &publish(spec.clone(), content(&[("a.txt", "1")])),
    )
    .await;
    let before = cloud.snapshot().normalized();
    let v1 = cloud.snapshot().workers["teitunnel-demo"].active.clone();
    let intent = update(spec, content(&[("a.txt", "2")]), Password::Off);
    let plan = engine.preview(&cloud, CTX, &intent).await.unwrap();
    // The new version is uploaded (session, bucket, version) but making it live fails.
    cloud.reset_failures();
    cloud.fail_once(3);
    let outcome = engine
        .apply(
            &cloud,
            &FakeConnectors::default(),
            CTX,
            &intent,
            Approval {
                fingerprint: &plan.fingerprint,
                confirmed: false,
            },
            |_| {},
        )
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::RolledBack { .. }), "{outcome:?}");
    assert_eq!(cloud.snapshot().workers["teitunnel-demo"].active, v1);
    assert_eq!(cloud.snapshot().normalized(), before);
}
