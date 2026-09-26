use super::*;
use crate::{
    engine::{
        Change, Context, Engine, Local, Outcome,
        fake::{CloudState, FakeCloud, FakeConnectors},
    },
    store::Store,
    text::Text,
};

#[tokio::test]
async fn applies_routes_and_snapshots_and_records_what_it_created() {
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let (conns, keychain) = (
        FakeConnectors::default(),
        crate::secrets::MemoryStore::default(),
    );
    let cloud = FakeCloud::new(CloudState {
        zones: vec![crate::engine::ZoneRef {
            id: "z".into(),
            name: "example.com".into(),
        }],
        workers_subdomain: Some("acme".into()),
        ..CloudState::default()
    });
    let site = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(site.path().join("dist")).unwrap();
    std::fs::write(site.path().join("dist/index.html"), "<h1>Docs</h1>").unwrap();
    let mut project = loaded(
        "version: 1\nroutes:\n  - hostname: app.example.com\n    origin: 3000\nsnapshots:\n  - name: docs\n    source: { folder: dist }\n",
    );
    project.dir = site.path().to_path_buf();
    let store = engine.local().store().clone();

    let first = plan(&engine, &cloud, &conns, CTX, &project, &[])
        .await
        .unwrap();
    let applied = apply_routes(
        &engine,
        &cloud,
        &conns,
        CTX,
        &store,
        &first,
        false,
        |_, _| {},
    )
    .await
    .unwrap();
    assert!(applied.failure.is_none());
    assert_eq!(applied.created.len(), 1);
    let file = project.file().unwrap();
    let resolved = resolve(file, &project.vars).unwrap();
    let publish = || {
        publish_snapshot(
            &engine,
            &cloud,
            &conns,
            CTX,
            &keychain,
            &first.snapshots[0],
            &resolved.snapshots[0].0,
            false,
            |_| {},
        )
    };
    assert_eq!(publish().await.unwrap(), SnapshotResult::Published);
    assert_eq!(
        publish().await.unwrap(),
        SnapshotResult::UpToDate,
        "idempotent"
    );
    let known = registry::list(&store).await.unwrap();
    assert_eq!(known[0].created_routes[0].hostname, "app.example.com");

    // Applied: a second plan changes no route; the Snapshot exists.
    let again = plan(&engine, &cloud, &conns, CTX, &project, &[])
        .await
        .unwrap();
    assert!(again.routes.is_empty());
    assert!(again.snapshots[0].exists);
    assert!(
        again.items.iter().all(|i| i.state == ItemState::Applied),
        "{:?}",
        again.items
    );
}

#[test]
fn the_published_schema_knows_every_key() {
    let schema: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../apps/web/public/schema/teitunnel.v1.json"
    ))
    .unwrap();
    let keys = |pointer: &str| -> Vec<String> {
        let mut keys: Vec<String> = schema
            .pointer(pointer)
            .and_then(serde_json::Value::as_object)
            .unwrap()
            .keys()
            .cloned()
            .collect();
        keys.sort();
        keys
    };
    assert_eq!(
        keys("/properties"),
        [
            "$schema",
            "account",
            "localDomains",
            "project",
            "protection",
            "routes",
            "shares",
            "snapshots",
            "version"
        ]
    );
    assert_eq!(
        keys("/$defs/route/properties"),
        [
            "hostname",
            "login",
            "origin",
            "originRequest",
            "path",
            "signIn",
            "skipLogin",
            "tunnel"
        ]
    );
    assert_eq!(
        keys("/$defs/share/properties"),
        [
            "expires",
            "hostHeader",
            "hostname",
            "inspect",
            "login",
            "port",
            "url"
        ]
    );
    assert_eq!(
        keys("/$defs/snapshot/properties"),
        [
            "expires", "hostname", "login", "name", "password", "source", "spa"
        ]
    );
    let mut origin = crate::domain::ORIGIN_OPTION_KEYS.to_vec();
    origin.sort_unstable();
    assert_eq!(keys("/$defs/originRequest/properties"), origin);
    // The example in the tests is valid against the parser.
    assert!(!parse(FULL).has_errors());
}

const FULL: &str = r#"# yaml-language-server: $schema=https://teitunnel.teispace.com/schema/teitunnel.v1.json
version: 1
project: shop
account: Acme
routes:
  - hostname: shop.example.com
    origin: 3000
  - hostname: api.example.com
    origin: http://localhost:4000
    path: ^/v1
    login: [me@example.com, "@team.io"]
    originRequest:
      httpHostHeader: localhost:4000
      connectTimeout: 10
shares:
  - port: 5173
    hostname: "{branch}-{project}.example.com"
    expires: 2h
    inspect: false
    hostHeader: localhost:5173
  - url: http://127.0.0.1:8080
snapshots:
  - name: docs
    source: { folder: dist }
    hostname: docs.example.com
    spa: true
    password: { env: DOCS_PASSWORD }
    expires: 30d
localDomains:
  - name: shop.localhost
    port: 3000
    wildcard: true
protection:
  - path: /admin
    password: { keychain: shop-admin }
"#;

fn errors(text: &str) -> Vec<(u32, u32, String)> {
    parse(text)
        .diagnostics
        .into_iter()
        .filter(|d| d.severity == DiagnosticSeverity::Error)
        .map(|d| (d.line, d.column, d.message.english()))
        .collect()
}

#[test]
fn reads_every_section() {
    let parsed = parse(FULL);
    let file = parsed
        .file
        .as_ref()
        .unwrap_or_else(|| panic!("{:?}", parsed.diagnostics));
    assert_eq!(file.version, 1);
    assert_eq!(file.project.as_deref(), Some("shop"));
    assert_eq!(file.routes.len(), 2);
    let api = &file.routes[1];
    assert_eq!(api.path.as_deref(), Some("^/v1"));
    assert_eq!(api.line, 8);
    let login = api.login.as_ref().unwrap();
    assert_eq!(login.emails, ["me@example.com"]);
    assert_eq!(login.email_domains, ["team.io"]);
    let options = api.origin_request.as_ref().unwrap();
    assert_eq!(options.http_host_header.as_deref(), Some("localhost:4000"));
    assert_eq!(options.connect_timeout, Some(10));
    let share = &file.shares[0];
    assert_eq!(share.origin, "http://localhost:5173");
    assert_eq!(share.expires_after, Some(7200));
    assert!(!share.inspect);
    assert_eq!(
        share.host_header,
        HostHeaderDecl::Set("localhost:5173".into())
    );
    assert_eq!(file.shares[1].hostname, None);
    let docs = &file.snapshots[0];
    assert_eq!(docs.password, Some(SecretRef::Env("DOCS_PASSWORD".into())));
    assert_eq!(docs.expires_in_days, Some(30));
    assert_eq!(docs.source, SnapshotSourceDecl::Folder("dist".into()));
    assert!(file.local_domains[0].wildcard);
    // A newer version's section: kept out, reported, not an error.
    assert_eq!(file.unknown_keys, ["protection"]);
    assert_eq!(parsed.diagnostics.len(), 1, "{:?}", parsed.diagnostics);
    let warning = &parsed.diagnostics[0];
    assert_eq!(warning.severity, DiagnosticSeverity::Warning);
    assert_eq!((warning.line, warning.column), (33, 1));
    assert!(warning.message.english().contains("protection"));
}

#[test]
fn points_at_each_problem() {
    let text = "version: 1\nroutes:\n  - hostname: not a host\n    origin: 3000\n  - origin: ftp://x\n    hostname: ok.example.com\nshares:\n  - port: 99999\n";
    let found = errors(text);
    assert_eq!(found.len(), 3, "{found:?}");
    assert_eq!((found[0].0, found[0].1), (3, 15));
    assert_eq!((found[1].0, found[1].1), (5, 13));
    assert_eq!((found[2].0, found[2].1), (8, 11));
    assert!(found[2].2.contains("99999"), "{found:?}");
}

#[test]
fn reports_syntax_errors_and_missing_or_unsupported_versions() {
    let found = errors("version: 1\nroutes: [\n");
    assert_eq!(found.len(), 1);
    assert!(found[0].0 >= 2);
    assert!(errors("routes: []\n")[0].2.contains("version: 1"));
    assert!(errors("version: 2\n")[0].2.contains('2'));
    assert!(errors("- a\n- b\n")[0].2.contains("mapping"));
    assert!(!parse("version: 1\n").has_errors());
}

#[test]
fn refuses_secrets_anywhere() {
    for (text, line) in [
        (
            "version: 1\nsnapshots:\n  - name: docs\n    source: { folder: dist }\n    password: hunter2\n",
            5,
        ),
        (
            "version: 1\nprotection:\n  token: \"Y2hhbmdlLW1lLXRoaXMtaXMtbm90LXJlYWw0MDAx\"\n",
            3,
        ),
        ("version: 1\nfuture:\n  - apiKey: abc\n", 3),
        (
            "version: 1\nnotes: \"eyJhIjoiMTIzNDU2Nzg5MCIsInQiOiJhYmNkZWYtMTIzNC01Njc4IiwicyI6Ik1USXpORFUyIn0=\"\n",
            2,
        ),
    ] {
        let parsed = parse(text);
        assert!(parsed.file.is_none(), "{text}");
        let error = parsed
            .diagnostics
            .iter()
            .find(|d| d.severity == DiagnosticSeverity::Error)
            .unwrap();
        assert_eq!(error.line, line, "{text}: {:?}", parsed.diagnostics);
        let english = error.message.english();
        assert!(english.contains("secret"), "{english}");
    }
    // References are fine.
    assert!(!parse("version: 1\nsnapshots:\n  - name: docs\n    source: { folder: dist }\n    password: { keychain: docs-preview }\n").has_errors());
}

#[test]
fn checks_values_with_the_cores_validators() {
    let cases = [
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    path: \"(\"\n",
            "path",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    login: [\"not-an-email@\"]\n",
            "login",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    originRequest: { connectTimeout: 0 }\n",
            "originRequest",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    skipLogin: [/webhooks]\n",
            "needs login",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    signIn: github\n",
            "signIn needs login",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    login: [\"@example.com\"]\n    signIn: okta\n",
            "unknown signIn",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    login: [\"github:teispace\"]\n    signIn: google\n",
            "GitHub teams with Google",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    login: [\"github:-bad\"]\n",
            "GitHub organization",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 3000\n    login: [me@example.com]\n    skipLogin: [\"/a b\"]\n",
            "plain path",
        ),
        (
            "shares:\n  - port: 3000\n    hostname: \"{team}.example.com\"\n",
            "placeholder",
        ),
        ("shares:\n  - port: 3000\n    expires: soon\n", "duration"),
        (
            "shares:\n  - port: 3000\n    url: http://localhost:3000\n",
            "both",
        ),
        (
            "shares:\n  - port: 3000\n    login: [me@example.com]\n",
            "hostname",
        ),
        (
            "localDomains:\n  - name: shop.example.com\n    port: 3000\n",
            "local",
        ),
        (
            "snapshots:\n  - name: docs\n    source: { folder: ../outside }\n",
            "relative",
        ),
        (
            "snapshots:\n  - name: docs\n    source: { zip: dist }\n",
            "source",
        ),
        (
            "routes:\n  - hostname: a.example.com\n    origin: 1\n  - hostname: a.example.com\n    origin: 2\n",
            "twice",
        ),
    ];
    for (body, what) in cases {
        let found = errors(&format!("version: 1\n{body}"));
        assert!(!found.is_empty(), "{what}: no error");
        assert!(
            found.iter().all(|(line, _, _)| *line > 1),
            "{what}: {found:?}"
        );
    }
}

#[test]
fn reads_github_teams_and_how_people_log_in() {
    let parsed = parse(
        "version: 1\nroutes:\n  - hostname: a.example.com\n    origin: 3000\n    login: [\"github:teispace/Softup Dev\", \"me@example.com\"]\n  - hostname: b.example.com\n    origin: 4000\n    login: \"@example.com\"\n    signIn: google\n",
    );
    let file = parsed
        .file
        .as_ref()
        .unwrap_or_else(|| panic!("{:?}", parsed.diagnostics));
    let github = file.routes[0].login.as_ref().unwrap();
    assert_eq!(github.github, ["teispace/Softup Dev"]);
    assert_eq!(github.emails, ["me@example.com"]);
    assert_eq!(github.sign_in, crate::engine::SignIn::Github);
    let google = file.routes[1].login.as_ref().unwrap();
    assert_eq!(
        (google.email_domains.as_slice(), google.sign_in),
        (
            ["example.com".to_owned()].as_slice(),
            crate::engine::SignIn::Google
        )
    );
}

#[test]
fn durations_and_local_names() {
    assert_eq!(parse_duration("90s"), Some(90));
    assert_eq!(parse_duration("30"), Some(1800));
    assert_eq!(parse_duration("2h"), Some(7200));
    assert_eq!(parse_duration("7d"), Some(604_800));
    assert_eq!(parse_duration("91d"), None);
    assert_eq!(parse_duration("0"), None);
    assert!(valid_local_name("shop.localhost"));
    assert!(valid_local_name("api.shop.test"));
    assert!(!valid_local_name("localhost"));
    assert!(!valid_local_name("shop.example.com"));
}

#[test]
fn finds_the_file_up_to_the_repository_root() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".git")).unwrap();
    std::fs::write(dir.path().join(FILE_NAME), "version: 1\n").unwrap();
    let nested = dir.path().join("packages/web");
    std::fs::create_dir_all(&nested).unwrap();
    assert_eq!(find(&nested), Some(dir.path().join(FILE_NAME)));
    let other = tempfile::tempdir().unwrap();
    assert!(matches!(load(other.path()), Err(ProjectError::NotFound(_))));
    let loaded = load(dir.path()).unwrap();
    assert!(!loaded.parsed.has_errors());
    std::fs::write(dir.path().join(FILE_NAME), "x".repeat(300 * 1024)).unwrap();
    assert!(matches!(load(dir.path()), Err(ProjectError::TooLarge(_))));
}

const CTX: Context<'static> = Context {
    account: "acc",
    machine_name: "Mac",
    tunnel: None,
};

fn cloud() -> FakeCloud {
    FakeCloud::new(CloudState {
        zones: vec![crate::engine::ZoneRef {
            id: "z".into(),
            name: "example.com".into(),
        }],
        ..CloudState::default()
    })
}

fn loaded(text: &str) -> Loaded {
    let parsed = parse(text);
    assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
    Loaded {
        path: "/w/shop/teitunnel.yml".into(),
        dir: "/w/shop".into(),
        name: "shop".into(),
        parsed,
        vars: template::Vars {
            branch: Some("feat-pay".into()),
            user: Some("ada".into()),
            project: Some("shop".into()),
        },
    }
}

async fn apply_all(engine: &Engine, cloud: &FakeCloud, conns: &FakeConnectors, plan: &ProjectPlan) {
    for action in &plan.routes {
        let outcome = apply_route(engine, cloud, conns, CTX, action, false, |_| {})
            .await
            .unwrap();
        assert!(matches!(outcome, Outcome::Applied { .. }), "{outcome:?}");
    }
}

#[tokio::test]
async fn plans_applies_and_is_idempotent() {
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let (cloud, conns) = (cloud(), FakeConnectors::default());
    let project = loaded(
        "version: 1\nroutes:\n  - hostname: \"{branch}.example.com\"\n    origin: 3000\n  - hostname: api.example.com\n    origin: 4000\n    originRequest: { httpHostHeader: localhost:4000 }\nshares:\n  - port: 5173\n    hostname: \"{user}-{project}.example.com\"\n  - port: 8080\nlocalDomains:\n  - name: shop.localhost\n    port: 3000\n",
    );
    let first = plan(&engine, &cloud, &conns, CTX, &project, &[])
        .await
        .unwrap();
    assert_eq!(first.routes.len(), 2);
    assert_eq!(first.routes[0].hostname, "feat-pay.example.com");
    assert!(matches!(first.routes[0].change, Change::AddRoute { .. }));
    assert!(!first.routes[0].plan.steps.is_empty());
    let states: Vec<ItemState> = first.items.iter().map(|i| i.state).collect();
    assert_eq!(
        states,
        [
            ItemState::Missing,
            ItemState::Missing,
            ItemState::Missing,
            ItemState::Missing,
            ItemState::Missing
        ]
    );
    assert_eq!(first.local_domains.len(), 1);
    assert_eq!(first.local_domains[0].name, "shop.localhost");
    assert!(!first.local_domains[0].exists);
    assert_eq!(first.shares.len(), 2);
    assert_eq!(
        first.shares[0].hostname.as_deref(),
        Some("ada-shop.example.com")
    );
    assert_eq!(first.shares[1].hostname, None);
    // Nothing changed by planning.
    assert_eq!(cloud.mutations(), 0);

    apply_all(&engine, &cloud, &conns, &first).await;
    let api = cloud.snapshot();
    let ingress = &api
        .tunnels
        .values()
        .next()
        .unwrap()
        .config
        .as_ref()
        .unwrap()
        .ingress;
    assert_eq!(ingress.len(), 3, "two routes and the catch-all");

    // Applied: a second plan has no route changes, and running shares aren't restarted.
    let again = plan(
        &engine,
        &cloud,
        &conns,
        CTX,
        &project,
        &["http://localhost:8080".to_owned()],
    )
    .await
    .unwrap();
    assert!(again.routes.is_empty(), "{:?}", again.routes);
    assert_eq!(again.items[0].state, ItemState::Applied);
    assert_eq!(again.items[1].state, ItemState::Applied);
    assert_eq!(
        again.items[4].state,
        ItemState::Missing,
        "local domains aren't applied by routes"
    );

    // Local domains go into this computer's registry, marked as the project's.
    let store = engine.local().store().clone();
    assert_eq!(apply_local_domains(&store, &again).await.unwrap(), 1);
    let rows = crate::local_domains::registry::list(&store).await.unwrap();
    assert_eq!(rows[0].name.as_str(), "shop.localhost");
    assert_eq!(rows[0].project.as_deref(), Some("/w/shop/teitunnel.yml"));
    assert!(rows[0].https);
    let third = plan(
        &engine,
        &cloud,
        &conns,
        CTX,
        &project,
        &["http://localhost:8080".to_owned()],
    )
    .await
    .unwrap();
    assert_eq!(third.items[4].state, ItemState::Applied);
    assert!(third.local_domains.is_empty());
    assert_eq!(again.shares.len(), 1, "the Quick Share runs already");
    assert_ne!(again.fingerprint, first.fingerprint);
}

#[tokio::test]
async fn a_changed_file_updates_what_differs() {
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let (cloud, conns) = (cloud(), FakeConnectors::default());
    let before = loaded(
        "version: 1\nroutes:\n  - hostname: app.example.com\n    origin: 3000\n    originRequest: { noTLSVerify: true }\n",
    );
    let first = plan(&engine, &cloud, &conns, CTX, &before, &[])
        .await
        .unwrap();
    apply_all(&engine, &cloud, &conns, &first).await;

    // The origin moved and the option was dropped from the file.
    let after = loaded("version: 1\nroutes:\n  - hostname: app.example.com\n    origin: 3001\n");
    let second = plan(&engine, &cloud, &conns, CTX, &after, &[])
        .await
        .unwrap();
    assert_eq!(second.items[0].state, ItemState::Differs);
    assert!(matches!(
        second.routes[0].change,
        Change::UpdateRoute { .. }
    ));
    apply_all(&engine, &cloud, &conns, &second).await;
    let rule = cloud
        .snapshot()
        .tunnels
        .values()
        .next()
        .unwrap()
        .config
        .clone()
        .unwrap()
        .ingress[0]
        .clone();
    assert_eq!(rule.service, "http://localhost:3001");
    assert!(rule.origin_request.get("noTLSVerify").is_none(), "{rule:?}");
    assert!(
        plan(&engine, &cloud, &conns, CTX, &after, &[])
            .await
            .unwrap()
            .routes
            .is_empty()
    );
}

#[tokio::test]
async fn refuses_a_route_that_changed_since_it_was_shown() {
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let (cloud, conns) = (cloud(), FakeConnectors::default());
    let project = loaded("version: 1\nroutes:\n  - hostname: app.example.com\n    origin: 3000\n");
    let shown = plan(&engine, &cloud, &conns, CTX, &project, &[])
        .await
        .unwrap();
    // Someone else adds a DNS record at the hostname meanwhile.
    cloud.state.lock().unwrap().records.insert(
        "z".into(),
        vec![cf_api::DnsRecord {
            id: "r1".into(),
            name: "app.example.com".into(),
            kind: "A".into(),
            content: "192.0.2.1".into(),
            proxied: false,
            ttl: 1,
            comment: None,
        }],
    );
    engine.invalidate("acc");
    let err = apply_route(
        &engine,
        &cloud,
        &conns,
        CTX,
        &shown.routes[0],
        false,
        |_| {},
    )
    .await
    .unwrap_err();
    assert!(matches!(err, ProjectError::Changed), "{err:?}");
    assert_eq!(cloud.mutations(), 0);
}

#[tokio::test]
async fn unknown_tunnels_and_placeholders_without_values_are_errors() {
    let engine = Engine::new(Local::new(Store::open_in_memory().unwrap()));
    let (cloud, conns) = (cloud(), FakeConnectors::default());
    let project = loaded(
        "version: 1\nroutes:\n  - hostname: app.example.com\n    origin: 3000\n    tunnel: staging\n",
    );
    let err = plan(&engine, &cloud, &conns, CTX, &project, &[])
        .await
        .unwrap_err();
    assert!(err.to_string().contains("staging"), "{err}");

    let mut no_git =
        loaded("version: 1\nshares:\n  - port: 3000\n    hostname: \"{branch}.example.com\"\n");
    no_git.vars.branch = None;
    let err = plan(&engine, &cloud, &conns, CTX, &no_git, &[])
        .await
        .unwrap_err();
    assert!(matches!(err, ProjectError::Unresolved(_)), "{err:?}");
}

#[test]
fn snapshot_changes_keep_the_password_they_cant_compare() {
    let parsed = parse(FULL);
    let decl = parsed.file.unwrap().snapshots[0].clone();
    let publish = snapshot_change(
        &decl,
        Some("docs.example.com"),
        "p1".into(),
        None,
        Some(crate::Secret::new("s3cret".into())),
    );
    assert!(matches!(
        publish,
        crate::snapshot::SnapshotChange::Publish {
            options: crate::snapshot::SnapshotOptions {
                password: crate::snapshot::PasswordInput::Set { .. },
                ..
            },
            ..
        }
    ));
    let debug = format!("{publish:?}");
    assert!(!debug.contains("s3cret"), "{debug}");
}

#[test]
fn resolves_secret_references() {
    use crate::secrets::SecretStore as _;
    let store = crate::secrets::MemoryStore::default();
    store
        .set("docs-preview", &crate::Secret::new("pw".into()))
        .unwrap();
    let found = resolve_secret(&SecretRef::Keychain("docs-preview".into()), &store).unwrap();
    assert_eq!(found.expose(), "pw");
    let missing = resolve_secret(
        &SecretRef::Env("TEITUNNEL_TEST_UNSET_VARIABLE".into()),
        &store,
    )
    .unwrap_err();
    let text: Text = crate::text::UserText::text(&missing);
    assert!(text.english().contains("TEITUNNEL_TEST_UNSET_VARIABLE"));
}

#[test]
fn paths_can_skip_a_routes_login() {
    let parsed = parse(
        "version: 1\nroutes:\n  - hostname: a.example.com\n    origin: 3000\n    login: [me@example.com]\n    skipLogin: [webhooks/*, /api/hooks]\n",
    );
    assert!(!parsed.has_errors(), "{:?}", parsed.diagnostics);
    let login = parsed.file.unwrap().routes[0].login.clone().unwrap();
    assert_eq!(login.bypass, ["/api/hooks", "/webhooks"]);
}
