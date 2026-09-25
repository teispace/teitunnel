use std::time::Duration;

use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use super::{
    detect::{self, Answer},
    *,
};

macro_rules! fixture {
    ($name:literal) => {
        include_bytes!(concat!("fixtures/", $name)).as_slice()
    };
}

fn ok(body: &[u8]) -> Answer<'_> {
    Answer { status: 200, body }
}

fn kind(found: Option<detect::Detected>) -> Option<ExposureKind> {
    found.map(|d| d.kind)
}

#[test]
fn env_files_by_their_variable_names_only() {
    let found = detect::env_file(ok(fixture!("env.txt"))).unwrap();
    assert_eq!(found.kind, ExposureKind::EnvFile);
    let detail = found.detail.unwrap();
    assert_eq!(
        detail,
        "APP_ENV, APP_KEY, DB_CONNECTION, DATABASE_URL, STRIPE_SECRET_KEY, …"
    );
    assert!(!detail.contains("postgres://"), "never a value");
    // A single-page app answering every path, or a 404, isn't a .env file.
    assert_eq!(detect::env_file(ok(fixture!("vite-index.html"))), None);
    assert_eq!(
        detect::env_file(Answer {
            status: 404,
            body: fixture!("env.txt")
        }),
        None
    );
    assert_eq!(detect::env_file(ok(b"Not Found")), None);
}

#[test]
fn git_ds_store_listings_and_backups() {
    assert_eq!(
        kind(detect::git(ok(fixture!("git-config.txt")))),
        Some(ExposureKind::GitRepository)
    );
    assert_eq!(
        kind(detect::git(ok(fixture!("git-head.txt")))),
        Some(ExposureKind::GitRepository)
    );
    assert_eq!(kind(detect::git(ok(fixture!("next-404.html")))), None);
    assert_eq!(
        kind(detect::ds_store(ok(fixture!("ds_store.bin")))),
        Some(ExposureKind::DsStore)
    );
    assert_eq!(
        kind(detect::ds_store(ok(fixture!("vite-index.html")))),
        None
    );
    for listing in [
        fixture!("nginx-autoindex.html"),
        fixture!("python-http-server.html"),
    ] {
        assert_eq!(
            kind(detect::directory_listing(ok(listing))),
            Some(ExposureKind::DirectoryListing)
        );
    }
    assert_eq!(
        kind(detect::directory_listing(ok(fixture!("vite-index.html")))),
        None
    );
    assert_eq!(
        kind(detect::backup(ok(fixture!("backup.zip")))),
        Some(ExposureKind::BackupArchive)
    );
    assert_eq!(
        kind(detect::backup(ok(fixture!("db.sqlite")))),
        Some(ExposureKind::DatabaseFile)
    );
    for dump in [fixture!("mysqldump.sql"), fixture!("pg_dump.sql")] {
        assert_eq!(
            kind(detect::backup(ok(dump))),
            Some(ExposureKind::DatabaseDump)
        );
    }
    assert_eq!(kind(detect::backup(ok(fixture!("vite-index.html")))), None);
}

#[test]
fn debug_pages_of_each_framework() {
    let cases: [(&[u8], ExposureKind); 10] = [
        (fixture!("django-debug-404.html"), ExposureKind::DjangoDebug),
        (
            fixture!("ignition-health-check.json"),
            ExposureKind::LaravelDebug,
        ),
        (fixture!("whoops.html"), ExposureKind::LaravelDebug),
        (
            fixture!("rails-routing-error.html"),
            ExposureKind::RailsDevelopment,
        ),
        (
            fixture!("rails-info-properties.html"),
            ExposureKind::RailsDevelopment,
        ),
        (
            fixture!("better-errors.html"),
            ExposureKind::RailsDevelopment,
        ),
        (
            fixture!("symfony-profiler.html"),
            ExposureKind::SymfonyProfiler,
        ),
        (fixture!("actuator-env.json"), ExposureKind::SpringActuator),
        (fixture!("phpinfo.html"), ExposureKind::PhpInfo),
        (fixture!("express-stack.html"), ExposureKind::StackTrace),
    ];
    for (body, expected) in cases {
        let status = if expected == ExposureKind::DjangoDebug {
            404
        } else {
            200
        };
        assert_eq!(
            kind(detect::debug_page(Answer { status, body })),
            Some(expected),
            "{}",
            String::from_utf8_lossy(&body[..body.len().min(80)])
        );
    }
    assert_eq!(
        kind(detect::debug_page(Answer {
            status: 500,
            body: fixture!("flask-traceback.html")
        })),
        Some(ExposureKind::StackTrace)
    );
    for quiet in [
        fixture!("next-404.html"),
        fixture!("vite-index.html"),
        fixture!("built-index.html"),
    ] {
        assert_eq!(
            kind(detect::debug_page(Answer {
                status: 404,
                body: quiet
            })),
            None
        );
    }
}

#[test]
fn open_admin_panels_and_database_tools() {
    assert_eq!(
        kind(detect::open_admin(ok(fixture!("django-admin-open.html")))),
        Some(ExposureKind::OpenAdmin)
    );
    assert_eq!(
        kind(detect::open_admin(ok(fixture!("django-admin-login.html")))),
        None,
        "a login"
    );
    assert_eq!(
        kind(detect::open_admin(ok(fixture!("adminer.html")))),
        Some(ExposureKind::DatabaseTool)
    );
    assert_eq!(
        kind(detect::open_admin(ok(fixture!("phpmyadmin.html")))),
        Some(ExposureKind::DatabaseTool)
    );
    assert_eq!(
        kind(detect::jupyter(ok(fixture!("jupyter-status.json")))),
        Some(ExposureKind::Jupyter)
    );
    assert_eq!(
        kind(detect::jupyter(Answer {
            status: 403,
            body: fixture!("jupyter-forbidden.html")
        })),
        None,
        "a token is required"
    );
}

#[test]
fn databases_on_the_port() {
    let cases: [(&[u8], &str); 5] = [
        (fixture!("mongo-http.txt"), "MongoDB"),
        (fixture!("elasticsearch.json"), "Elasticsearch"),
        (fixture!("redis.txt"), "Redis"),
        (fixture!("postgres.txt"), "PostgreSQL"),
        (fixture!("mysql-greeting.bin"), "MySQL"),
    ];
    for (body, name) in cases {
        let found = detect::database_on_port(Answer { status: 0, body }).unwrap();
        assert_eq!(found.kind, ExposureKind::DatabasePort);
        assert_eq!(found.detail.as_deref(), Some(name));
    }
    assert_eq!(
        detect::database_on_port(ok(fixture!("vite-index.html"))),
        None
    );
    assert_eq!(
        detect::database_on_port(ok(b"HTTP/1.1 200 OK\r\n\r\nhello")),
        None
    );
}

#[test]
fn source_maps_with_sources() {
    let found = detect::source_map(ok(fixture!("sourcemap.json"))).unwrap();
    assert_eq!(
        (found.kind, found.detail.as_deref()),
        (ExposureKind::SourceMap, Some("2"))
    );
    assert_eq!(
        detect::source_map(ok(fixture!("sourcemap-no-content.json"))),
        None
    );
    assert_eq!(detect::source_map(ok(fixture!("vite-index.html"))), None);
    let html = String::from_utf8_lossy(fixture!("built-index.html"));
    assert_eq!(
        detect::scripts(&html, 5),
        ["/assets/index-DiwrgTda.js", "/vendor.js"],
        "same-origin scripts only"
    );
    let vite = String::from_utf8_lossy(fixture!("vite-index.html"));
    assert!(
        detect::scripts(&vite, 5).is_empty(),
        "dev modules aren't .js files"
    );
}

#[test]
fn severity_orders_findings() {
    let found = collect(vec![
        (
            "/.DS_Store".into(),
            detect::Detected {
                kind: ExposureKind::DsStore,
                detail: None,
            },
        ),
        (
            "/.env".into(),
            detect::Detected {
                kind: ExposureKind::EnvFile,
                detail: None,
            },
        ),
        (
            "/x".into(),
            detect::Detected {
                kind: ExposureKind::StackTrace,
                detail: None,
            },
        ),
        (
            "/.git/HEAD".into(),
            detect::Detected {
                kind: ExposureKind::EnvFile,
                detail: None,
            },
        ),
    ]);
    let kinds: Vec<ExposureKind> = found.iter().map(|f| f.kind).collect();
    assert_eq!(
        kinds,
        [
            ExposureKind::EnvFile,
            ExposureKind::StackTrace,
            ExposureKind::DsStore
        ]
    );
    assert_eq!(found[0].path, "/.env", "the first path seen");
    assert!(!found[0].title.english().is_empty());
}

async fn serve(routes: &[(&str, u16, &'static [u8])]) -> MockServer {
    let server = MockServer::start().await;
    for (at, status, body) in routes {
        Mock::given(method("GET"))
            .and(path(*at))
            .respond_with(ResponseTemplate::new(*status).set_body_bytes(*body))
            .mount(&server)
            .await;
    }
    // Everything else: the app's own 404.
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404).set_body_bytes(fixture!("next-404.html")))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn checks_a_real_server_in_parallel() {
    let server = serve(&[
        ("/", 200, fixture!("built-index.html")),
        ("/.env", 200, fixture!("env.txt")),
        ("/.git/HEAD", 200, fixture!("git-head.txt")),
        ("/dump.sql", 200, fixture!("mysqldump.sql")),
        (MISSING_PATH, 404, fixture!("django-debug-404.html")),
        (
            "/assets/index-DiwrgTda.js.map",
            200,
            fixture!("sourcemap.json"),
        ),
        // A redirect is never followed.
        ("/admin/", 302, b""),
    ])
    .await;
    let report = check(&server.uri()).await;
    let kinds: Vec<ExposureKind> = report.findings.iter().map(|f| f.kind).collect();
    assert_eq!(
        kinds,
        [
            ExposureKind::EnvFile,
            ExposureKind::GitRepository,
            ExposureKind::DatabaseDump,
            ExposureKind::DjangoDebug,
            ExposureKind::SourceMap,
        ],
        "{report:?}"
    );
    assert!(!report.incomplete);
    assert_eq!(report.findings[0].path, "/.env");
    assert!(report.elapsed_ms < 2000, "{}", report.elapsed_ms);
    // Only GETs, a bounded number of them.
    let requests = server.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .all(|r| r.method == wiremock::http::Method::GET)
    );
    // The probes, the source maps and the raw request for a database.
    assert!(
        requests.len() <= PROBES.len() + SOURCE_MAPS + 1,
        "{}",
        requests.len()
    );
}

#[tokio::test]
async fn a_single_page_app_that_answers_everything_is_clean() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(fixture!("vite-index.html")))
        .mount(&server)
        .await;
    let report = check(&server.uri()).await;
    assert!(report.is_clean(), "{:?}", report.findings);
}

#[tokio::test]
async fn stays_within_its_time_budget() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_bytes(fixture!("env.txt"))
                .set_delay(Duration::from_secs(10)),
        )
        .mount(&server)
        .await;
    let started = std::time::Instant::now();
    let report = check(&server.uri()).await;
    // Well short of the server's 10 s, with room for a machine busy compiling.
    assert!(
        started.elapsed() < Duration::from_secs(6),
        "{:?}",
        started.elapsed()
    );
    assert!(report.incomplete);
    assert!(report.is_clean());
}

#[tokio::test]
async fn recognises_a_database_on_the_port() {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // A Redis-like server: answers every request line with an error and closes.
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buffer = [0; 256];
                let _ = stream.read(&mut buffer).await;
                let _ = stream.write_all(fixture!("redis.txt")).await;
            });
        }
    });
    let report = check(&port.to_string()).await;
    let found = report
        .findings
        .iter()
        .find(|f| f.kind == ExposureKind::DatabasePort);
    assert_eq!(
        found.and_then(|f| f.detail.as_deref()),
        Some("Redis"),
        "{report:?}"
    );
}

#[tokio::test]
async fn only_web_origins_are_checked() {
    for origin in ["ssh://localhost:22", "tcp://localhost:5432", "hello_world"] {
        let report = check(origin).await;
        assert!(report.is_clean() && report.requests == 0, "{origin}");
    }
}
