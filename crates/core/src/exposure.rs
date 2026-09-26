//! Exposure check: before a service goes public, look at it (directly, not through
//! Cloudflare) for the common leaks: a `.env` file, the git repository, a directory
//! listing, backups and database dumps, framework debug pages and tools, admin panels
//! and database tools open without a login, a database on the shared port, Jupyter
//! without a token, source maps with the original sources.
//!
//! It's bounded and quick: about twenty GET requests at once to the service itself,
//! 1.5 s each and 2 s in all, at most 64 KiB read per answer, redirects never followed.
//! It never blocks: findings are shown as a warning with "Share anyway".

mod detect;

use std::time::{Duration, Instant};

use serde::Serialize;

use crate::{
    domain::RouteOrigin,
    text::{Text, msg::exposure as m},
};
use detect::{Answer, Detected};

/// The whole check's budget.
const TOTAL: Duration = Duration::from_secs(2);
/// One request's.
const PER_REQUEST: Duration = Duration::from_millis(1500);
/// Bytes read from an answer.
const BODY_LIMIT: usize = 64 * 1024;
/// A path no app has, for the framework's "not found" page.
pub const MISSING_PATH: &str = "/teitunnel-exposure-check-404";

/// What was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ExposureKind {
    /// A `.env` file.
    EnvFile,
    /// The `.git` folder.
    GitRepository,
    /// A macOS `.DS_Store` file.
    DsStore,
    /// A directory listing.
    DirectoryListing,
    /// A zip archive (a backup).
    BackupArchive,
    /// A SQL dump.
    DatabaseDump,
    /// A SQLite database file.
    DatabaseFile,
    /// Django's debug pages (`DEBUG = True`).
    DjangoDebug,
    /// Laravel's Ignition or Whoops.
    LaravelDebug,
    /// Rails in development (error pages, web-console, better_errors, `/rails/info`).
    RailsDevelopment,
    /// Symfony's profiler.
    SymfonyProfiler,
    /// Spring Boot's actuator (`/actuator/env`).
    SpringActuator,
    /// `phpinfo()`.
    PhpInfo,
    /// A stack trace in an error page.
    StackTrace,
    /// An admin panel without a login.
    OpenAdmin,
    /// Adminer or phpMyAdmin.
    DatabaseTool,
    /// A database or search engine on the port itself.
    DatabasePort,
    /// Jupyter without a token.
    Jupyter,
    /// Source maps with the original code.
    SourceMap,
}

/// How bad it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum ExposureSeverity {
    /// Secrets or data are readable.
    High,
    /// Internals or tools are reachable.
    Medium,
    /// Worth knowing.
    Low,
}

impl ExposureKind {
    /// How bad it is.
    pub fn severity(self) -> ExposureSeverity {
        match self {
            Self::EnvFile
            | Self::GitRepository
            | Self::BackupArchive
            | Self::DatabaseDump
            | Self::DatabaseFile
            | Self::LaravelDebug
            | Self::SpringActuator
            | Self::OpenAdmin
            | Self::DatabaseTool
            | Self::DatabasePort
            | Self::Jupyter => ExposureSeverity::High,
            Self::DjangoDebug
            | Self::RailsDevelopment
            | Self::SymfonyProfiler
            | Self::PhpInfo
            | Self::StackTrace => ExposureSeverity::Medium,
            Self::DsStore | Self::DirectoryListing | Self::SourceMap => ExposureSeverity::Low,
        }
    }

    /// One line for the user.
    pub fn title(self) -> Text {
        match self {
            Self::EnvFile => m::env_file::title(),
            Self::GitRepository => m::git_repository::title(),
            Self::DsStore => m::ds_store::title(),
            Self::DirectoryListing => m::directory_listing::title(),
            Self::BackupArchive => m::backup_archive::title(),
            Self::DatabaseDump => m::database_dump::title(),
            Self::DatabaseFile => m::database_file::title(),
            Self::DjangoDebug => m::django_debug::title(),
            Self::LaravelDebug => m::laravel_debug::title(),
            Self::RailsDevelopment => m::rails_development::title(),
            Self::SymfonyProfiler => m::symfony_profiler::title(),
            Self::SpringActuator => m::spring_actuator::title(),
            Self::PhpInfo => m::php_info::title(),
            Self::StackTrace => m::stack_trace::title(),
            Self::OpenAdmin => m::open_admin::title(),
            Self::DatabaseTool => m::database_tool::title(),
            Self::DatabasePort => m::database_port::title(),
            Self::Jupyter => m::jupyter::title(),
            Self::SourceMap => m::source_map::title(),
        }
    }

    /// What to do about it.
    pub fn advice(self) -> Text {
        match self {
            Self::EnvFile => m::env_file::advice(),
            Self::GitRepository => m::git_repository::advice(),
            Self::DsStore => m::ds_store::advice(),
            Self::DirectoryListing => m::directory_listing::advice(),
            Self::BackupArchive => m::backup_archive::advice(),
            Self::DatabaseDump => m::database_dump::advice(),
            Self::DatabaseFile => m::database_file::advice(),
            Self::DjangoDebug => m::django_debug::advice(),
            Self::LaravelDebug => m::laravel_debug::advice(),
            Self::RailsDevelopment => m::rails_development::advice(),
            Self::SymfonyProfiler => m::symfony_profiler::advice(),
            Self::SpringActuator => m::spring_actuator::advice(),
            Self::PhpInfo => m::php_info::advice(),
            Self::StackTrace => m::stack_trace::advice(),
            Self::OpenAdmin => m::open_admin::advice(),
            Self::DatabaseTool => m::database_tool::advice(),
            Self::DatabasePort => m::database_port::advice(),
            Self::Jupyter => m::jupyter::advice(),
            Self::SourceMap => m::source_map::advice(),
        }
    }
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ExposureFinding {
    /// What.
    pub kind: ExposureKind,
    /// How bad.
    pub severity: ExposureSeverity,
    /// Where (a path on the service).
    pub path: String,
    /// One line for the user.
    pub title: Text,
    /// What to do.
    pub advice: Text,
    /// What it's based on, when that helps (variable names, never values; a product).
    pub detail: Option<String>,
}

/// The outcome of a check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ExposureReport {
    /// The service checked.
    pub origin: String,
    /// What was found, worst first.
    pub findings: Vec<ExposureFinding>,
    /// Requests made.
    pub requests: u32,
    /// Some requests didn't finish in time (their answers weren't checked).
    pub incomplete: bool,
    /// How long it took.
    pub elapsed_ms: u32,
}

impl ExposureReport {
    /// Nothing found.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

type Detector = fn(Answer<'_>) -> Option<Detected>;

/// The requests made and what each answer is checked for.
const PROBES: &[(&str, &[Detector])] = &[
    (
        "/",
        &[
            detect::directory_listing,
            detect::open_admin,
            detect::database_on_port,
            detect::debug_page,
        ],
    ),
    ("/.env", &[detect::env_file]),
    ("/.git/config", &[detect::git]),
    ("/.git/HEAD", &[detect::git]),
    ("/.DS_Store", &[detect::ds_store]),
    ("/backup.zip", &[detect::backup]),
    ("/dump.sql", &[detect::backup]),
    ("/db.sqlite", &[detect::backup]),
    (MISSING_PATH, &[detect::debug_page]),
    ("/_profiler", &[detect::debug_page]),
    ("/actuator/env", &[detect::debug_page]),
    ("/phpinfo.php", &[detect::debug_page]),
    ("/_ignition/health-check", &[detect::debug_page]),
    ("/__better_errors", &[detect::debug_page]),
    ("/rails/info/properties", &[detect::debug_page]),
    ("/admin/", &[detect::open_admin]),
    ("/adminer.php", &[detect::open_admin]),
    ("/phpmyadmin/", &[detect::open_admin]),
    ("/api/status", &[detect::jupyter]),
];

/// Source maps checked for (from the scripts the home page loads).
const SOURCE_MAPS: usize = 2;

/// The base URL for a web origin, or `None` for one that isn't HTTP.
fn base_url(origin: &str) -> Option<String> {
    let origin = RouteOrigin::parse(origin).ok()?;
    if !origin.is_web() {
        return None;
    }
    let url = reqwest::Url::parse(origin.as_str()).ok()?;
    Some(format!(
        "{}://{}",
        url.scheme(),
        url.host_str().map(|h| match url.port() {
            Some(p) => format!("{h}:{p}"),
            None => h.to_owned(),
        })?
    ))
}

fn is_private(host: &str) -> bool {
    let bare = host.trim_start_matches('[').trim_end_matches(']');
    match bare.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(ip)) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Ok(std::net::IpAddr::V6(ip)) => ip.is_loopback() || ip.is_unique_local(),
        Err(_) => host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local"),
    }
}

async fn read_limited(mut response: reqwest::Response) -> Vec<u8> {
    let mut body = Vec::new();
    while body.len() < BODY_LIMIT {
        match response.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            _ => break,
        }
    }
    body.truncate(BODY_LIMIT);
    body
}

async fn fetch(client: &reqwest::Client, base: &str, path: &str) -> Option<(u16, Vec<u8>)> {
    let response = client
        .get(format!("{base}{path}"))
        .header(reqwest::header::ACCEPT, "*/*")
        .send()
        .await
        .ok()?;
    let status = response.status().as_u16();
    // Never follow a redirect; its body says nothing about the path asked for.
    if (300..400).contains(&status) {
        return Some((status, Vec::new()));
    }
    Some((status, read_limited(response).await))
}

/// The bytes a service answers a plain request with on the TCP level (databases don't
/// speak HTTP, but most say what they are).
async fn raw_answer(host: &str, port: u16) -> Option<Vec<u8>> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let host = host.trim_start_matches('[').trim_end_matches(']');
    // Every address at once: Windows takes seconds to report a refused one, so trying
    // them in turn (`localhost` is `::1` first) misses a service on 127.0.0.1.
    let connects = tokio::net::lookup_host((host, port))
        .await
        .ok()?
        .map(|addr| Box::pin(tokio::net::TcpStream::connect(addr)));
    let (mut stream, _) = futures_util::future::select_ok(connects).await.ok()?;
    // MySQL greets first; the others answer the request line.
    let _ = stream.write_all(b"GET / HTTP/1.0\r\n\r\n").await;
    let mut buffer = vec![0; 1024];
    let read = stream.read(&mut buffer).await.ok()?;
    buffer.truncate(read);
    Some(buffer)
}

/// Checks the service at `origin` (a port, `host:port` or URL) for leaks. Never fails:
/// what can't be reached is simply not reported.
pub async fn check(origin: &str) -> ExposureReport {
    let started = Instant::now();
    let mut report = ExposureReport {
        origin: origin.to_owned(),
        findings: Vec::new(),
        requests: 0,
        incomplete: false,
        elapsed_ms: 0,
    };
    let Some(base) = base_url(origin) else {
        return report;
    };
    let Ok(url) = reqwest::Url::parse(&base) else {
        return report;
    };
    let host = url.host_str().unwrap_or("localhost").to_owned();
    let port = url.port_or_known_default().unwrap_or(80);
    let Ok(client) = reqwest::Client::builder()
        .user_agent(concat!("Teitunnel/", env!("CARGO_PKG_VERSION"), " exposure-check"))
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_millis(700))
        .timeout(PER_REQUEST)
        // The user's own service on this computer or network, often self-signed.
        .tls_danger_accept_invalid_certs(is_private(&host))
        .build()
    else {
        return report;
    };

    // Every request ends by the deadline; what hasn't answered by then isn't checked.
    let deadline = tokio::time::Instant::now() + TOTAL;
    let within = |path: String| {
        let (client, base) = (&client, &base);
        async move {
            let answer = tokio::time::timeout_at(deadline, fetch(client, base, &path))
                .await
                .ok()
                .flatten();
            (path, answer)
        }
    };
    let raw = tokio::time::timeout(PER_REQUEST / 2, raw_answer(&host, port));
    let (answers, raw) = tokio::join!(
        futures_util::future::join_all(PROBES.iter().map(|(path, _)| within((*path).to_owned()))),
        raw
    );
    // Source maps of the scripts the home page loads.
    let home = answers
        .first()
        .and_then(|(_, a)| a.as_ref())
        .map(|(_, body)| String::from_utf8_lossy(body).into_owned())
        .unwrap_or_default();
    let maps = futures_util::future::join_all(
        detect::scripts(&home, SOURCE_MAPS)
            .into_iter()
            .map(|script| within(format!("{script}.map"))),
    )
    .await;
    let raw = raw.ok().flatten();

    let mut findings: Vec<(String, Detected)> = Vec::new();
    for ((path, answer), (_, detectors)) in answers.iter().zip(PROBES) {
        match answer {
            Some((status, body)) => {
                let answer = Answer {
                    status: *status,
                    body,
                };
                for detector in *detectors {
                    if let Some(found) = detector(answer) {
                        findings.push((path.clone(), found));
                    }
                }
            }
            None => report.incomplete = true,
        }
    }
    if let Some(bytes) = &raw
        && let Some(found) = detect::database_on_port(Answer {
            status: 0,
            body: bytes,
        })
    {
        findings.push(("/".to_owned(), found));
    }
    for (path, answer) in &maps {
        if let Some((status, body)) = answer
            && let Some(found) = detect::source_map(Answer {
                status: *status,
                body,
            })
        {
            findings.push((path.clone(), found));
        }
    }
    report.requests = u32::try_from(answers.len() + maps.len() + 1).unwrap_or(u32::MAX);
    report.findings = collect(findings);
    report.elapsed_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    report
}

/// One finding per kind (the first path it was seen at), worst first.
fn collect(found: Vec<(String, Detected)>) -> Vec<ExposureFinding> {
    let mut out: Vec<ExposureFinding> = Vec::new();
    for (path, detected) in found {
        if out.iter().any(|f| f.kind == detected.kind) {
            continue;
        }
        out.push(ExposureFinding {
            kind: detected.kind,
            severity: detected.kind.severity(),
            path,
            title: detected.kind.title(),
            advice: detected.kind.advice(),
            detail: detected.detail,
        });
    }
    out.sort_by_key(|f| f.severity);
    out
}

#[cfg(test)]
mod tests;
