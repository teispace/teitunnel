//! Recognising leaks in a response: pure functions over the status and the first bytes
//! of the body, written against real servers' answers (fixtures next to this file).
//! Every detector looks at the content, never the status alone: dev servers answer any
//! path with their app (a single-page app's `index.html`), which must not count.

use super::ExposureKind;

/// What a probe got back.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Answer<'a> {
    pub(crate) status: u16,
    pub(crate) body: &'a [u8],
}

impl Answer<'_> {
    fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    fn text(&self) -> std::borrow::Cow<'_, str> {
        String::from_utf8_lossy(self.body)
    }

    fn html(&self) -> bool {
        let head = self.text();
        let start = head
            .trim_start()
            .get(..200)
            .unwrap_or(head.trim_start())
            .to_ascii_lowercase();
        start.starts_with('<') || start.contains("<html") || start.contains("<!doctype")
    }
}

/// A finding with what it's based on (names only, never values).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Detected {
    pub(crate) kind: ExposureKind,
    pub(crate) detail: Option<String>,
}

fn found(kind: ExposureKind) -> Option<Detected> {
    Some(Detected { kind, detail: None })
}

/// `/.env`: `NAME=value` lines (the names are kept, the values never).
pub(crate) fn env_file(a: Answer<'_>) -> Option<Detected> {
    if !a.ok() || a.html() {
        return None;
    }
    let text = a.text();
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect();
    let names: Vec<String> = lines
        .iter()
        .filter_map(|line| {
            let line = line.strip_prefix("export ").unwrap_or(line);
            let (name, _) = line.split_once('=')?;
            let name = name.trim();
            let valid = !name.is_empty()
                && name.len() <= 64
                && name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            valid.then(|| name.to_owned())
        })
        .collect();
    if names.is_empty() || names.len() * 2 < lines.len() {
        return None;
    }
    let mut shown: Vec<&str> = names.iter().take(5).map(String::as_str).collect();
    if names.len() > 5 {
        shown.push("…");
    }
    Some(Detected {
        kind: ExposureKind::EnvFile,
        detail: Some(shown.join(", ")),
    })
}

/// `/.git/config` or `/.git/HEAD`: the repository (history, remotes, maybe credentials).
pub(crate) fn git(a: Answer<'_>) -> Option<Detected> {
    if !a.ok() || a.html() {
        return None;
    }
    let text = a.text();
    let t = text.trim();
    let config = t.contains("[core]") && t.contains("repositoryformatversion");
    let head =
        t.starts_with("ref: refs/") || (t.len() == 40 && t.chars().all(|c| c.is_ascii_hexdigit()));
    (config || head).then_some(Detected {
        kind: ExposureKind::GitRepository,
        detail: None,
    })
}

/// `/.DS_Store`: a Finder index listing the folder's files.
pub(crate) fn ds_store(a: Answer<'_>) -> Option<Detected> {
    (a.ok() && a.body.starts_with(&[0, 0, 0, 1, b'B', b'u', b'd', b'1'])).then_some(Detected {
        kind: ExposureKind::DsStore,
        detail: None,
    })
}

/// A directory listing at `/` (nginx/Apache autoindex, Python's http.server,
/// http-server, serve-index, IIS).
pub(crate) fn directory_listing(a: Answer<'_>) -> Option<Detected> {
    if !a.ok() {
        return None;
    }
    let text = a.text();
    let markers = [
        "<title>Index of /",
        "<h1>Index of /",
        "Directory listing for /",
        "<title>listing directory /",
        "[To Parent Directory]",
    ];
    markers
        .iter()
        .any(|m| text.contains(m))
        .then_some(Detected {
            kind: ExposureKind::DirectoryListing,
            detail: None,
        })
}

/// `/backup.zip`, `/dump.sql`, `/db.sqlite`: an archive, a database dump or file.
pub(crate) fn backup(a: Answer<'_>) -> Option<Detected> {
    if !a.ok() {
        return None;
    }
    if a.body.starts_with(b"PK\x03\x04") {
        return found(ExposureKind::BackupArchive);
    }
    if a.body.starts_with(b"SQLite format 3\0") {
        return found(ExposureKind::DatabaseFile);
    }
    if a.html() {
        return None;
    }
    let text = a.text();
    let dump = text.contains("-- MySQL dump")
        || text.contains("-- MariaDB dump")
        || text.contains("PostgreSQL database dump")
        || ((text.contains("CREATE TABLE") || text.contains("INSERT INTO")) && text.contains(';'));
    dump.then_some(Detected {
        kind: ExposureKind::DatabaseDump,
        detail: None,
    })
}

/// Framework debug and error pages (a request for a page that doesn't exist, or the
/// framework's own tools).
pub(crate) fn debug_page(a: Answer<'_>) -> Option<Detected> {
    let text = a.text();
    if text.contains("DEBUG = True") && (text.contains("Django") || text.contains("URLconf")) {
        return found(ExposureKind::DjangoDebug);
    }
    if text.contains("can_execute_commands")
        || (text.contains("Ignition") && text.contains("laravel"))
    {
        return found(ExposureKind::LaravelDebug);
    }
    if text.contains("Whoops! There was an error") || text.contains("Whoops container") {
        return found(ExposureKind::LaravelDebug);
    }
    if (text.contains("Routing Error") && text.contains("Rails.root"))
        || (a.ok() && text.contains("Rails version") && text.contains("Ruby version"))
    {
        return found(ExposureKind::RailsDevelopment);
    }
    if text.contains("better_errors") || text.contains("BetterErrors") {
        return found(ExposureKind::RailsDevelopment);
    }
    if a.ok() && (text.contains("Symfony Profiler") || text.contains("sf-toolbar")) {
        return found(ExposureKind::SymfonyProfiler);
    }
    if a.ok() && text.contains("\"activeProfiles\"") && text.contains("\"propertySources\"") {
        return found(ExposureKind::SpringActuator);
    }
    if a.ok() && text.contains("phpinfo()") && text.contains("PHP Version") {
        return found(ExposureKind::PhpInfo);
    }
    stack_trace(&text).then_some(Detected {
        kind: ExposureKind::StackTrace,
        detail: None,
    })
}

/// Node/Express, Python or Java stack traces in a page.
fn stack_trace(text: &str) -> bool {
    let js = text
        .lines()
        .filter(|l| {
            let l = l.replace("&nbsp;", " ");
            let l = l.trim();
            l.starts_with("at ")
                && (l.contains(".js:")
                    || l.contains(".ts:")
                    || l.contains(".mjs:")
                    || l.contains(".cjs:"))
        })
        .count();
    let python = text.contains("Traceback (most recent call last)");
    let java = text.contains("\tat java.") || text.contains("at org.springframework.");
    js >= 2 || python || java
}

/// Admin panels and database tools answering without a login.
pub(crate) fn open_admin(a: Answer<'_>) -> Option<Detected> {
    if !a.ok() {
        return None;
    }
    let text = a.text();
    if text.contains("Site administration")
        && text.contains("logout")
        && !text.contains("login-form")
    {
        return found(ExposureKind::OpenAdmin);
    }
    if text.contains("Adminer")
        && (text.contains("adminer.org") || text.contains("Login - Adminer"))
    {
        return found(ExposureKind::DatabaseTool);
    }
    if text.contains("phpMyAdmin") && (text.contains("pma_") || text.contains("phpmyadmin")) {
        return found(ExposureKind::DatabaseTool);
    }
    None
}

/// Jupyter's API answering without a token.
pub(crate) fn jupyter(a: Answer<'_>) -> Option<Detected> {
    let text = a.text();
    (a.ok() && text.contains("\"started\"") && text.contains("\"kernels\"")).then_some(Detected {
        kind: ExposureKind::Jupyter,
        detail: None,
    })
}

/// A database or search engine on the shared port itself (seen through HTTP or the raw
/// bytes it answers a request with).
pub(crate) fn database_on_port(a: Answer<'_>) -> Option<Detected> {
    let text = a.text();
    let name = if text.contains("It looks like you are trying to access MongoDB over HTTP") {
        "MongoDB"
    } else if text.contains("You Know, for Search") {
        "Elasticsearch"
    } else if text.contains("\"couchdb\"") && text.contains("Welcome") {
        "CouchDB"
    } else if text.starts_with("-ERR") || text.starts_with("-DENIED") {
        "Redis"
    } else if text.contains("unsupported frontend protocol") {
        "PostgreSQL"
    } else if a.body.len() > 5
        && a.body[4] == 10
        && ["mysql_native_password", "caching_sha2_password", "MariaDB"]
            .iter()
            .any(|m| text.contains(m))
    {
        "MySQL"
    } else {
        return None;
    };
    Some(Detected {
        kind: ExposureKind::DatabasePort,
        detail: Some(name.to_owned()),
    })
}

/// A source map with the original sources in it.
pub(crate) fn source_map(a: Answer<'_>) -> Option<Detected> {
    if !a.ok() || a.html() {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(a.body).ok()?;
    let sources = value.get("sourcesContent")?.as_array()?;
    let with_content = sources
        .iter()
        .filter(|s| s.as_str().is_some_and(|s| !s.is_empty()))
        .count();
    (with_content > 0).then(|| Detected {
        kind: ExposureKind::SourceMap,
        detail: Some(with_content.to_string()),
    })
}

/// Same-origin script URLs in a page (at most `limit`), for their source maps.
pub(crate) fn scripts(html: &str, limit: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(at) = rest.find("<script") {
        rest = &rest[at + 7..];
        let tag_end = rest.find('>').unwrap_or(rest.len());
        let tag = &rest[..tag_end];
        if let Some(src) = attribute(tag, "src") {
            let same_origin =
                !src.contains("://") && !src.starts_with("//") && !src.starts_with("data:");
            let path = src.split(['?', '#']).next().unwrap_or_default();
            if same_origin && path.ends_with(".js") && !path.contains("..") {
                let path = if path.starts_with('/') {
                    path.to_owned()
                } else {
                    format!("/{path}")
                };
                if !out.contains(&path) {
                    out.push(path);
                }
            }
        }
        if out.len() >= limit {
            break;
        }
    }
    out
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let at = tag.find(&format!("{name}="))? + name.len() + 1;
    let value = &tag[at..];
    let quote = value.chars().next()?;
    if quote == '"' || quote == '\'' {
        value[1..].split(quote).next()
    } else {
        value.split([' ', '>']).next()
    }
}
