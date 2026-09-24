//! Capturing a running site (usually a local dev server) as files, for apps without a
//! static export: a bounded crawl from `/` that follows same-origin links in HTML, CSS
//! and (best effort) JavaScript, plus the sitemap. It never leaves the origin and never
//! writes outside the folder it's given.

use std::{
    collections::{BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::LazyLock,
    time::Duration,
};

use regex::Regex;
use reqwest::Url;
use serde::Serialize;

use super::SnapshotError;
use crate::engine::MAX_FILE_SIZE;

/// How far a crawl may go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// HTML pages.
    pub pages: usize,
    /// Files of any kind.
    pub files: usize,
    /// Bytes in total.
    pub bytes: u64,
    /// Per request.
    pub timeout: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            pages: 500,
            files: 5_000,
            bytes: 500 * 1000 * 1000,
            timeout: Duration::from_secs(30),
        }
    }
}

/// What a crawl captured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct CrawlReport {
    /// HTML pages saved.
    pub pages: u32,
    /// Files saved (pages included).
    pub files: u32,
    /// Bytes saved.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub bytes: u64,
    /// Paths that answered with an error (first 50).
    pub failed: Vec<String>,
    /// A limit was reached: the capture is incomplete.
    pub truncated: bool,
    /// Only one page was found and it loads scripts: probably a single-page app (serve
    /// `index.html` for every path).
    pub single_page: bool,
}

static HTML_LINK: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(?i)\b(?:href|src|poster|data-src|content)\s*=\s*["']([^"'<>]+)["']"#).ok()
});
static SRCSET: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"(?i)\bsrcset\s*=\s*["']([^"'<>]+)["']"#).ok());
static CSS_URL: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"url\(\s*["']?([^"')\s]+)["']?\s*\)|@import\s+["']([^"']+)["']"#).ok()
});
/// Module imports and asset paths in bundles: `import("./About-x1.js")`, `"/assets/logo.svg"`.
static JS_PATH: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"["'`]((?:\.{1,2}/|/)[A-Za-z0-9_@~./-]+\.(?:m?js|css|json|svg|png|jpe?g|gif|webp|avif|ico|woff2?|ttf|otf|wasm|mp4|webm))["'`]"#)
        .ok()
});
static SITEMAP_LOC: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"<loc>\s*([^<\s]+)\s*</loc>").ok());

/// Matches of a pattern (the patterns are constants covered by tests; one that failed to
/// compile matches nothing rather than panicking).
fn captures<'a>(
    pattern: &'a LazyLock<Option<Regex>>,
    text: &'a str,
) -> impl Iterator<Item = regex::Captures<'a>> + 'a {
    pattern.iter().flat_map(move |p| p.captures_iter(text))
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// Links in a document of `kind` at `base`, resolved; others' origins are dropped later.
fn links(kind: Kind, body: &str, base: &Url) -> Vec<(Url, bool)> {
    let mut found: Vec<(String, bool)> = Vec::new();
    match kind {
        Kind::Html => {
            found.extend(captures(&HTML_LINK, body).map(|c| (c[1].to_owned(), true)));
            for set in captures(&SRCSET, body) {
                found.extend(set[1].split(',').filter_map(|candidate| {
                    candidate
                        .split_whitespace()
                        .next()
                        .map(|u| (u.to_owned(), true))
                }));
            }
            found.extend(captures(&CSS_URL, body).filter_map(|c| {
                c.get(1)
                    .or_else(|| c.get(2))
                    .map(|m| (m.as_str().to_owned(), true))
            }));
        }
        Kind::Css => found.extend(captures(&CSS_URL, body).filter_map(|c| {
            c.get(1)
                .or_else(|| c.get(2))
                .map(|m| (m.as_str().to_owned(), true))
        })),
        // Guesses: a miss is ignored rather than reported.
        Kind::Script => found.extend(captures(&JS_PATH, body).map(|c| (c[1].to_owned(), false))),
        Kind::Other => {}
    }
    found
        .into_iter()
        .filter(|(link, _)| {
            !link.starts_with("data:")
                && !link.starts_with("mailto:")
                && !link.starts_with("javascript:")
                && !link.starts_with('#')
        })
        .filter_map(|(link, sure)| {
            let decoded = link.replace("&amp;", "&");
            base.join(&decoded).ok().map(|url| (url, sure))
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Html,
    Css,
    Script,
    Other,
}

fn kind_of(content_type: &str, path: &str) -> Kind {
    let content_type = content_type.to_ascii_lowercase();
    if content_type.starts_with("text/html") || content_type.starts_with("application/xhtml") {
        Kind::Html
    } else if content_type.starts_with("text/css") || path.ends_with(".css") {
        Kind::Css
    } else if content_type.contains("javascript") || path.ends_with(".js") || path.ends_with(".mjs")
    {
        Kind::Script
    } else {
        Kind::Other
    }
}

fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = segment.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Where a URL path is saved under the capture folder, or `None` if it can't be saved
/// safely (a traversal, a reserved or unrepresentable name).
///
/// `/` → `index.html`; an HTML page `/about` → `about.html` (served at `/about`);
/// `/docs/` → `docs/index.html`; other files keep their path.
pub(crate) fn file_for(path: &str, html: bool) -> Option<PathBuf> {
    let mut segments = Vec::new();
    for raw in path.trim_start_matches('/').split('/') {
        if raw.is_empty() {
            continue;
        }
        let segment = percent_decode(raw)?;
        let unsafe_name = segment == "."
            || segment == ".."
            || segment.starts_with('.') && segment != ".well-known"
            || segment.chars().any(|c| {
                c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
            });
        if unsafe_name {
            return None;
        }
        segments.push(segment);
    }
    let directory = path.ends_with('/') || segments.is_empty();
    let mut file = PathBuf::new();
    if html {
        if directory {
            file.extend(&segments);
            file.push("index.html");
        } else {
            let last = segments.pop()?;
            file.extend(&segments);
            if Path::new(&last)
                .extension()
                .is_some_and(|e| e == "html" || e == "htm")
            {
                file.push(last);
            } else {
                file.push(format!("{last}.html"));
            }
        }
    } else {
        if directory {
            return None;
        }
        file.extend(&segments);
    }
    Some(file)
}

/// Crawls `start`'s origin into `dest` (created if needed, and expected to be empty).
///
/// # Errors
/// The site can't be reached at all, answers the first page with an error, or `dest`
/// can't be written.
pub async fn crawl(start: &str, dest: &Path, limits: Limits) -> Result<CrawlReport, SnapshotError> {
    let origin = Url::parse(start).map_err(|_| SnapshotError::InvalidUrl(start.to_owned()))?;
    if !matches!(origin.scheme(), "http" | "https") || origin.host_str().is_none() {
        return Err(SnapshotError::InvalidUrl(start.to_owned()));
    }
    let root = origin
        .join("/")
        .map_err(|_| SnapshotError::InvalidUrl(start.to_owned()))?;
    let policy_origin = root.clone();
    let http = reqwest::Client::builder()
        .user_agent(concat!(
            "Teitunnel/",
            env!("CARGO_PKG_VERSION"),
            " snapshot"
        ))
        .timeout(limits.timeout)
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            if attempt.previous().len() < 5 && same_origin(attempt.url(), &policy_origin) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(|e| SnapshotError::Crawl(e.to_string()))?;
    tokio::fs::create_dir_all(dest)
        .await
        .map_err(|e| SnapshotError::io(dest, &e))?;
    let dest = tokio::fs::canonicalize(dest)
        .await
        .map_err(|e| SnapshotError::io(dest, &e))?;

    let mut report = CrawlReport {
        pages: 0,
        files: 0,
        bytes: 0,
        failed: Vec::new(),
        truncated: false,
        single_page: false,
    };
    let mut queue: VecDeque<(Url, bool)> = VecDeque::from([(origin.clone(), true)]);
    if origin.path() != "/" {
        queue.push_back((root.clone(), true));
    }
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut saved: BTreeSet<PathBuf> = BTreeSet::new();
    let mut scripts = 0u32;
    let mut first = true;

    // The sitemap lists pages nothing links to.
    if let Ok(sitemap) = root.join("/sitemap.xml")
        && let Ok(response) = http.get(sitemap).send().await
        && response.status().is_success()
        && let Ok(text) = response.text().await
    {
        for loc in captures(&SITEMAP_LOC, &text) {
            if let Ok(url) = Url::parse(&loc[1]).or_else(|_| root.join(&loc[1])) {
                queue.push_back((url, true));
            }
        }
    }

    while let Some((mut url, sure)) = queue.pop_front() {
        if !same_origin(&url, &root) {
            continue;
        }
        url.set_fragment(None);
        url.set_query(None);
        if !seen.insert(url.path().to_owned()) {
            continue;
        }
        if report.files as usize >= limits.files || report.bytes >= limits.bytes {
            report.truncated = true;
            break;
        }
        let response = match http.get(url.clone()).send().await {
            Ok(response) => response,
            Err(err) if first => return Err(SnapshotError::Unreachable(format!("{url}: {err}"))),
            Err(_) => {
                if sure && report.failed.len() < 50 {
                    report.failed.push(url.path().to_owned());
                }
                continue;
            }
        };
        if !response.status().is_success() || !same_origin(response.url(), &root) {
            if first {
                return Err(SnapshotError::Unreachable(format!(
                    "{url}: {}",
                    response.status()
                )));
            }
            if sure && report.failed.len() < 50 {
                report.failed.push(url.path().to_owned());
            }
            continue;
        }
        first = false;
        let final_url = response.url().clone();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let kind = kind_of(&content_type, url.path());
        if kind == Kind::Html && report.pages as usize >= limits.pages {
            report.truncated = true;
            continue;
        }
        if response.content_length().is_some_and(|n| n > MAX_FILE_SIZE) {
            continue;
        }
        let Ok(body) = response.bytes().await else {
            continue;
        };
        if body.len() as u64 > MAX_FILE_SIZE {
            continue;
        }
        let Some(relative) = file_for(url.path(), kind == Kind::Html) else {
            continue;
        };
        let target = dest.join(&relative);
        if !target.starts_with(&dest) || !saved.insert(relative.clone()) {
            continue;
        }
        if let Some(parent) = target.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| SnapshotError::io(parent, &e))?;
        }
        tokio::fs::write(&target, &body)
            .await
            .map_err(|e| SnapshotError::io(&target, &e))?;
        report.files += 1;
        report.bytes += body.len() as u64;
        match kind {
            Kind::Html => report.pages += 1,
            Kind::Script => scripts += 1,
            _ => {}
        }
        if kind != Kind::Other {
            let text = String::from_utf8_lossy(&body);
            for (link, sure) in links(kind, &text, &final_url) {
                if same_origin(&link, &root) {
                    queue.push_back((link, sure));
                }
            }
        }
    }
    // Favicons are requested by browsers without a link.
    if !saved.contains(Path::new("favicon.ico"))
        && let Ok(url) = root.join("/favicon.ico")
        && let Ok(response) = http.get(url).send().await
        && response.status().is_success()
        && let Ok(body) = response.bytes().await
        && (body.len() as u64) <= MAX_FILE_SIZE
    {
        let target = dest.join("favicon.ico");
        tokio::fs::write(&target, &body)
            .await
            .map_err(|e| SnapshotError::io(&target, &e))?;
        report.files += 1;
        report.bytes += body.len() as u64;
    }
    report.single_page = report.pages == 1 && scripts > 0;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    fn html(body: &str) -> ResponseTemplate {
        ResponseTemplate::new(200)
            .set_body_raw(body.as_bytes().to_vec(), "text/html; charset=utf-8")
    }

    fn typed(body: &str, content_type: &str) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_raw(body.as_bytes().to_vec(), content_type)
    }

    async fn mount(server: &MockServer, at: &str, response: ResponseTemplate) {
        Mock::given(method("GET"))
            .and(path(at))
            .respond_with(response)
            .mount(server)
            .await;
    }

    fn files(dir: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![dir.to_path_buf()];
        while let Some(current) = stack.pop() {
            for entry in std::fs::read_dir(current).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    out.push(
                        path.strip_prefix(dir)
                            .unwrap()
                            .display()
                            .to_string()
                            .replace('\\', "/"),
                    );
                }
            }
        }
        out.sort();
        out
    }

    #[tokio::test]
    async fn captures_pages_and_their_assets_on_the_same_origin() {
        let server = MockServer::start().await;
        let other = MockServer::start().await;
        mount(
            &server,
            "/",
            html(&format!(
                r##"<html><head><link rel="stylesheet" href="/style.css"><script type="module" src="/assets/app.js"></script></head>
                <body><a href="/about">About</a> <a href="docs/">Docs</a> <a href="{}/elsewhere">Out</a>
                <img srcset="/img/a.png 1x, /img/b.png 2x"> <a href="#top">top</a> <a href="mailto:x@y.z">m</a></body></html>"##,
                other.uri()
            )),
        )
        .await;
        mount(
            &server,
            "/style.css",
            typed("body{background:url('/img/bg.png')}", "text/css"),
        )
        .await;
        mount(
            &server,
            "/assets/app.js",
            typed(r#"import("./chunk-1.js")"#, "text/javascript"),
        )
        .await;
        mount(
            &server,
            "/assets/chunk-1.js",
            typed("export default 1", "text/javascript"),
        )
        .await;
        mount(&server, "/about", html("<a href='/'>home</a>")).await;
        mount(&server, "/docs/", html("<p>docs</p>")).await;
        for image in ["/img/a.png", "/img/b.png", "/img/bg.png"] {
            mount(&server, image, typed("png", "image/png")).await;
        }
        mount(
            &server,
            "/sitemap.xml",
            typed(
                &format!(
                    "<urlset><url><loc>{}/hidden</loc></url></urlset>",
                    server.uri()
                ),
                "application/xml",
            ),
        )
        .await;
        mount(&server, "/hidden", html("secret-ish page")).await;
        Mock::given(path("/elsewhere"))
            .respond_with(html("x"))
            .expect(0)
            .mount(&other)
            .await;

        let dest = tempfile::tempdir().unwrap();
        let report = crawl(&server.uri(), dest.path(), Limits::default())
            .await
            .unwrap();
        assert_eq!(
            files(dest.path()),
            [
                "about.html",
                "assets/app.js",
                "assets/chunk-1.js",
                "docs/index.html",
                "hidden.html",
                "img/a.png",
                "img/b.png",
                "img/bg.png",
                "index.html",
                "style.css",
            ]
        );
        assert_eq!(report.pages, 4);
        assert!(!report.truncated);
        assert!(!report.single_page);
    }

    #[tokio::test]
    async fn stays_within_its_limits() {
        let server = MockServer::start().await;
        let links: String = (0..20)
            .map(|i| format!("<a href='/p{i}'>{i}</a>"))
            .collect();
        mount(&server, "/", html(&links)).await;
        for i in 0..20 {
            mount(&server, &format!("/p{i}"), html("page")).await;
        }
        let dest = tempfile::tempdir().unwrap();
        let limits = Limits {
            pages: 5,
            ..Limits::default()
        };
        let report = crawl(&server.uri(), dest.path(), limits).await.unwrap();
        assert_eq!(report.pages, 5);
        assert!(report.truncated);
    }

    #[tokio::test]
    async fn never_writes_outside_the_folder_or_follows_other_origins() {
        let server = MockServer::start().await;
        let other = MockServer::start().await;
        mount(
            &server,
            "/",
            html(r#"<a href="/%2e%2e/%2e%2e/escape.txt">1</a><a href="/a/..%2f..%2fescape2.txt">2</a><a href="/.env">3</a><a href="/redirect">4</a>"#),
        )
        .await;
        mount(
            &server,
            "/redirect",
            ResponseTemplate::new(302)
                .insert_header("location", format!("{}/landing", other.uri())),
        )
        .await;
        Mock::given(path("/landing"))
            .respond_with(html("x"))
            .expect(0)
            .mount(&other)
            .await;
        let parent = tempfile::tempdir().unwrap();
        let dest = parent.path().join("capture");
        let report = crawl(&server.uri(), &dest, Limits::default())
            .await
            .unwrap();
        assert_eq!(files(parent.path()), ["capture/index.html"]);
        assert_eq!(report.files, 1);
    }

    #[tokio::test]
    async fn recognises_a_single_page_app_and_an_unreachable_site() {
        let server = MockServer::start().await;
        mount(
            &server,
            "/",
            html(r#"<div id="root"></div><script src="/app.js"></script>"#),
        )
        .await;
        mount(
            &server,
            "/app.js",
            typed("render()", "application/javascript"),
        )
        .await;
        let dest = tempfile::tempdir().unwrap();
        let report = crawl(&server.uri(), dest.path(), Limits::default())
            .await
            .unwrap();
        assert!(report.single_page);

        let down = MockServer::start().await;
        let dest = tempfile::tempdir().unwrap();
        assert!(matches!(
            crawl(&down.uri(), dest.path(), Limits::default()).await,
            Err(SnapshotError::Unreachable(_))
        ));
        assert!(matches!(
            crawl("file:///etc/passwd", dest.path(), Limits::default()).await,
            Err(SnapshotError::InvalidUrl(_))
        ));
    }

    #[test]
    fn maps_url_paths_to_safe_files() {
        let f = |p: &str, html: bool| {
            file_for(p, html).map(|p| p.display().to_string().replace('\\', "/"))
        };
        assert_eq!(f("/", true).as_deref(), Some("index.html"));
        assert_eq!(f("/about", true).as_deref(), Some("about.html"));
        assert_eq!(f("/about.html", true).as_deref(), Some("about.html"));
        assert_eq!(f("/docs/", true).as_deref(), Some("docs/index.html"));
        assert_eq!(f("/a/b%20c.png", false).as_deref(), Some("a/b c.png"));
        assert_eq!(
            f("/.well-known/x.txt", false).as_deref(),
            Some(".well-known/x.txt")
        );
        assert_eq!(f("/%2e%2e/x", false), None);
        assert_eq!(f("/a%2f..%2fb", false), None);
        assert_eq!(f("/.env", false), None);
        assert_eq!(f("/dir/", false), None);
        assert_eq!(f("/bad%ZZ", false), None);
    }
}
