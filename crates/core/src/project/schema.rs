//! The project file's schema (version 1), read from the YAML tree with a message and a
//! position for every problem. Keys this version doesn't know are kept out of the model
//! but reported as warnings, so newer files still load; secrets are refused everywhere.

use serde::Serialize;

use super::{
    secret_scan::{credential_kind, is_secret_key},
    template,
    yaml::{Node, Pos, Value},
};
use crate::{
    domain::{OriginOptions, PathRule, RouteOrigin},
    engine::AccessRule,
    text::{Text, UserText, msg::project as m},
};

/// The schema version this Teitunnel reads and writes.
pub const VERSION: u32 = 1;

/// How serious a problem is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum DiagnosticSeverity {
    /// The file can't be applied.
    Error,
    /// Applied anyway (e.g. a key this version doesn't know).
    Warning,
}

/// A problem in the file, where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Diagnostic {
    /// 1-based line (0: the whole file).
    pub line: u32,
    /// 1-based column.
    pub column: u32,
    /// Error or warning.
    pub severity: DiagnosticSeverity,
    /// What's wrong.
    pub message: Text,
}

/// A secret the file refers to without containing it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "from", content = "name", rename_all = "camelCase")]
pub enum SecretRef {
    /// An environment variable of the process applying the file.
    Env(String),
    /// An entry in the OS keychain (Teitunnel's service), by name.
    Keychain(String),
}

/// A route the project needs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct RouteDecl {
    /// Hostname, possibly with placeholders.
    pub hostname: String,
    /// Where traffic goes.
    pub origin: String,
    /// Path regex.
    pub path: Option<String>,
    /// One of this machine's tunnels, by name (default: the default tunnel).
    pub tunnel: Option<String>,
    /// Require a login for these people.
    pub login: Option<AccessRule>,
    /// Origin settings (`originRequest`).
    pub origin_request: Option<OriginOptions>,
    /// Where it's declared.
    pub line: u32,
}

/// The Host header a share sends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "mode", content = "value", rename_all = "camelCase")]
pub enum HostHeaderDecl {
    /// Teitunnel decides (dev servers that need their own address get it).
    Auto,
    /// Pass the visitor's through.
    Off,
    /// Send this.
    Set(String),
}

/// A share the project starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ShareDecl {
    /// The service: `http://localhost:<port>` or the given URL.
    pub origin: String,
    /// A hostname on one of the account's domains (with placeholders); none: a Quick
    /// Share at a random trycloudflare.com address.
    pub hostname: Option<String>,
    /// Ends by itself after this many seconds.
    pub expires_after: Option<u32>,
    /// Record its traffic in the inspector.
    pub inspect: bool,
    /// Host header.
    pub host_header: HostHeaderDecl,
    /// Require a login (on a hostname only).
    pub login: Option<AccessRule>,
    /// Where it's declared.
    pub line: u32,
}

/// Where a Snapshot's files come from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", content = "path", rename_all = "camelCase")]
pub enum SnapshotSourceDecl {
    /// A folder, relative to the project file.
    Folder(String),
    /// A project to build first, relative to the project file.
    Build(String),
}

/// A Snapshot the project publishes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct SnapshotDecl {
    /// Its name.
    pub name: String,
    /// Its files.
    pub source: SnapshotSourceDecl,
    /// A hostname on one of the account's domains; none: workers.dev.
    pub hostname: Option<String>,
    /// Serve `index.html` for unknown paths.
    pub spa: bool,
    /// A password, by reference.
    pub password: Option<SecretRef>,
    /// Require a login.
    pub login: Option<AccessRule>,
    /// Delete it after this many days.
    pub expires_in_days: Option<u32>,
    /// Where it's declared.
    pub line: u32,
}

/// A local HTTPS name for a port on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LocalDomainDecl {
    /// E.g. `shop.localhost`.
    pub name: String,
    /// The port it serves.
    pub port: u16,
    /// Subdomains go to the same port.
    pub wildcard: bool,
    /// Where it's declared.
    pub line: u32,
}

/// A project file, validated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    /// Schema version.
    pub version: u32,
    /// The project's name (default: detected from the folder).
    pub project: Option<String>,
    /// The Cloudflare account, by name or id (needed with several).
    pub account: Option<String>,
    /// Routes.
    pub routes: Vec<RouteDecl>,
    /// Shares.
    pub shares: Vec<ShareDecl>,
    /// Snapshots.
    pub snapshots: Vec<SnapshotDecl>,
    /// Local domains.
    pub local_domains: Vec<LocalDomainDecl>,
    /// Top-level keys this version doesn't know (kept for newer versions, not applied).
    pub unknown_keys: Vec<String>,
}

/// Reading one file: the model (unless it had errors) and every problem found.
#[derive(Debug, Default)]
struct Reader {
    diagnostics: Vec<Diagnostic>,
}

impl Reader {
    fn error(&mut self, at: Pos, message: Text) {
        self.diagnostics.push(Diagnostic {
            line: at.line,
            column: at.column,
            severity: DiagnosticSeverity::Error,
            message,
        });
    }

    fn warn(&mut self, at: Pos, message: Text) {
        self.diagnostics.push(Diagnostic {
            line: at.line,
            column: at.column,
            severity: DiagnosticSeverity::Warning,
            message,
        });
    }

    fn text(&mut self, key: &str, node: &Node) -> Option<String> {
        match &node.value {
            Value::Str(s) if !s.trim().is_empty() => Some(s.trim().to_owned()),
            Value::Int(_) | Value::Float(_) => node.scalar(),
            _ => {
                self.error(node.at, m::expected_text(key));
                None
            }
        }
    }

    /// A list of text, or one entry as text.
    fn texts(&mut self, key: &str, node: &Node) -> Option<Vec<String>> {
        match &node.value {
            Value::Str(_) => self.text(key, node).map(|s| vec![s]),
            Value::Seq(items) => items.iter().map(|item| self.text(key, item)).collect(),
            _ => {
                self.error(node.at, m::expected_list(key));
                None
            }
        }
    }

    fn bool(&mut self, key: &str, node: &Node) -> Option<bool> {
        if let Value::Bool(b) = node.value {
            Some(b)
        } else {
            self.error(node.at, m::expected_bool(key));
            None
        }
    }

    fn list<'n>(&mut self, key: &str, node: &'n Node) -> &'n [Node] {
        match &node.value {
            Value::Seq(items) => items,
            Value::Null => &[],
            _ => {
                self.error(node.at, m::expected_list(key));
                &[]
            }
        }
    }

    fn mapping<'n>(&mut self, key: &str, node: &'n Node) -> Option<Vec<(&'n str, Pos, &'n Node)>> {
        let entries = node.entries();
        if entries.is_none() {
            self.error(node.at, m::expected_mapping(key));
        }
        entries
    }

    fn port(&mut self, node: &Node) -> Option<u16> {
        let value = node.scalar().unwrap_or_default();
        match value.trim().parse::<u16>() {
            Ok(port) if port > 0 => Some(port),
            _ => {
                self.error(node.at, m::bad_port(value));
                None
            }
        }
    }

    fn unknown(&mut self, key: &str, at: Pos) {
        self.warn(at, m::unknown_key(key));
    }

    /// `login: [me@example.com, "@example.com"]` (or one entry as text).
    fn login(&mut self, node: &Node) -> Option<AccessRule> {
        let items: Vec<(String, Pos)> = match &node.value {
            Value::Str(s) => vec![(s.clone(), node.at)],
            Value::Seq(items) => items
                .iter()
                .filter_map(|item| match item.scalar() {
                    Some(s) => Some((s, item.at)),
                    None => {
                        self.error(item.at, m::expected_text("login"));
                        None
                    }
                })
                .collect(),
            _ => {
                self.error(node.at, m::expected_list("login"));
                return None;
            }
        };
        let (emails, domains): (Vec<_>, Vec<_>) = items
            .iter()
            .partition(|(a, _)| a.trim().find('@').is_some_and(|at| at > 0));
        let rule = AccessRule {
            emails: emails.into_iter().map(|(s, _)| s.clone()).collect(),
            email_domains: domains.into_iter().map(|(s, _)| s.clone()).collect(),
            bypass: Vec::new(),
        };
        match rule.normalized() {
            Ok(rule) => Some(rule),
            Err(err) => {
                self.error(node.at, err.text());
                None
            }
        }
    }

    /// A hostname (placeholders allowed): checked with them filled in by example values.
    fn hostname(&mut self, node: &Node) -> Option<String> {
        let raw = self.text("hostname", node)?.to_ascii_lowercase();
        let sample = template::Vars {
            branch: Some("main".into()),
            user: Some("me".into()),
            project: Some("app".into()),
        };
        match template::expand(&raw, &sample) {
            Ok(expanded) => {
                if let Err(err) = crate::domain::Hostname::parse(&expanded) {
                    self.error(node.at, err.text());
                    return None;
                }
            }
            Err(err) => {
                self.error(node.at, template_error(&err));
                return None;
            }
        }
        Some(raw)
    }

    fn secret_ref(&mut self, key: &str, node: &Node) -> Option<SecretRef> {
        let Some(entries) = node.entries() else {
            self.error(node.at, m::literal_secret(key));
            return None;
        };
        let [(from, _, value)] = entries.as_slice() else {
            self.error(node.at, m::secret_ref(key));
            return None;
        };
        let name = value.scalar().filter(|n| valid_ref_name(n));
        match (*from, name) {
            ("env", Some(name)) => Some(SecretRef::Env(name)),
            ("keychain", Some(name)) => Some(SecretRef::Keychain(name)),
            _ => {
                self.error(node.at, m::secret_ref(key));
                None
            }
        }
    }

    fn duration(&mut self, node: &Node, days_only: bool) -> Option<u32> {
        let value = node.scalar().unwrap_or_default();
        match parse_duration(&value) {
            Some(seconds) if !days_only || seconds % 86_400 == 0 => Some(seconds),
            _ => {
                self.error(node.at, m::bad_duration(value));
                None
            }
        }
    }
}

fn valid_ref_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

pub(crate) fn template_error(err: &template::TemplateError) -> Text {
    match err {
        template::TemplateError::Unknown(name) => m::unknown_placeholder(name),
        template::TemplateError::Unclosed => m::unclosed_placeholder(),
        template::TemplateError::NoValue(name) => m::no_value(name),
    }
}

/// `90s`, `30m`, `2h`, `7d` or a bare number of minutes, in seconds (1 s to 90 days).
pub fn parse_duration(input: &str) -> Option<u32> {
    let input = input.trim();
    let split = input
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(input.len());
    let (number, unit) = input.split_at(split);
    let value: u32 = number.parse().ok()?;
    let seconds = match unit.trim() {
        "s" => value,
        "" | "m" | "min" => value.checked_mul(60)?,
        "h" => value.checked_mul(3600)?,
        "d" => value.checked_mul(86_400)?,
        _ => return None,
    };
    (1..=90 * 86_400).contains(&seconds).then_some(seconds)
}

/// Whether `name` is a local name Teitunnel can serve (`*.localhost`, `*.test`, `*.local`).
pub fn valid_local_name(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    let Some(rest) = [".localhost", ".test", ".local"]
        .iter()
        .find_map(|suffix| name.strip_suffix(suffix))
    else {
        return false;
    };
    !rest.is_empty()
        && rest.split('.').all(|l| {
            !l.is_empty()
                && l.len() <= 63
                && l.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && !l.starts_with('-')
                && !l.ends_with('-')
        })
}

/// The outcome of reading a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// The model, unless there were errors.
    pub file: Option<ProjectFile>,
    /// Errors and warnings, in file order.
    pub diagnostics: Vec<Diagnostic>,
}

impl Parsed {
    /// Whether any problem is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|d| d.severity == DiagnosticSeverity::Error)
    }
}

/// Reads and validates a project file's text.
pub fn parse(text: &str) -> Parsed {
    let mut reader = Reader::default();
    let root = match super::yaml::parse(text) {
        Ok(root) => root,
        Err(err) => {
            reader.error(err.at, m::syntax(err.message));
            return Parsed {
                file: None,
                diagnostics: reader.diagnostics,
            };
        }
    };
    let file = read_file(&mut reader, &root);
    scan_secrets(&mut reader, &root);
    reader
        .diagnostics
        .sort_by_key(|d| (d.line, d.column, d.severity == DiagnosticSeverity::Warning));
    reader.diagnostics.dedup();
    let errors = reader
        .diagnostics
        .iter()
        .any(|d| d.severity == DiagnosticSeverity::Error);
    Parsed {
        file: file.filter(|_| !errors),
        diagnostics: reader.diagnostics,
    }
}

/// Refuses every literal secret and credential-looking value, known keys or not.
fn scan_secrets(reader: &mut Reader, root: &Node) {
    let mut strings = Vec::new();
    root.strings(None, &mut strings);
    for (key, value, at) in strings {
        if let Some(kind) = credential_kind(value) {
            reader.error(at, m::secret_value(kind));
        } else if is_secret_key(key) {
            reader.error(at, m::literal_secret(key));
        }
    }
}

fn read_file(r: &mut Reader, root: &Node) -> Option<ProjectFile> {
    let Some(entries) = root.entries() else {
        r.error(root.at, m::not_a_mapping());
        return None;
    };
    let mut file = ProjectFile {
        version: 0,
        project: None,
        account: None,
        routes: Vec::new(),
        shares: Vec::new(),
        snapshots: Vec::new(),
        local_domains: Vec::new(),
        unknown_keys: Vec::new(),
    };
    for (key, at, value) in &entries {
        match *key {
            "version" => match &value.value {
                Value::Int(1) => file.version = VERSION,
                Value::Int(n) => r.error(value.at, m::version_unsupported(n.to_string())),
                _ => r.error(
                    value.at,
                    m::version_unsupported(value.scalar().unwrap_or_default()),
                ),
            },
            "project" => file.project = r.text(key, value).map(|p| template::label(&p)),
            "account" => file.account = r.text(key, value),
            "routes" => {
                for item in r.list(key, value) {
                    if let Some(route) = read_route(r, item) {
                        file.routes.push(route);
                    }
                }
            }
            "shares" => {
                for item in r.list(key, value) {
                    if let Some(share) = read_share(r, item) {
                        file.shares.push(share);
                    }
                }
            }
            "snapshots" => {
                for item in r.list(key, value) {
                    if let Some(snapshot) = read_snapshot(r, item) {
                        file.snapshots.push(snapshot);
                    }
                }
            }
            "localDomains" => {
                for item in r.list(key, value) {
                    if let Some(domain) = read_local_domain(r, item) {
                        file.local_domains.push(domain);
                    }
                }
            }
            // `$schema` is for editors.
            "$schema" => {}
            other => {
                r.unknown(other, *at);
                file.unknown_keys.push(other.to_owned());
            }
        }
    }
    if file.version == 0 && !entries.iter().any(|(k, _, _)| *k == "version") {
        r.error(root.at, m::version_missing());
    }
    check_duplicates(r, &file);
    Some(file)
}

fn check_duplicates(r: &mut Reader, file: &ProjectFile) {
    let mut seen = std::collections::HashSet::new();
    for route in &file.routes {
        let key = (
            route.hostname.clone(),
            route.path.clone().unwrap_or_default(),
        );
        if !seen.insert(key) {
            r.error(
                Pos {
                    line: route.line,
                    column: 1,
                },
                m::duplicate(&route.hostname),
            );
        }
    }
    for share in file.shares.iter().filter(|s| s.hostname.is_some()) {
        let hostname = share.hostname.clone().unwrap_or_default();
        if !seen.insert((hostname.clone(), String::new())) {
            r.error(
                Pos {
                    line: share.line,
                    column: 1,
                },
                m::duplicate(&hostname),
            );
        }
    }
    let mut names = std::collections::HashSet::new();
    for snapshot in &file.snapshots {
        if !names.insert(snapshot.name.to_ascii_lowercase()) {
            r.error(
                Pos {
                    line: snapshot.line,
                    column: 1,
                },
                m::duplicate(&snapshot.name),
            );
        }
    }
    let mut local = std::collections::HashSet::new();
    for domain in &file.local_domains {
        if !local.insert(domain.name.clone()) {
            r.error(
                Pos {
                    line: domain.line,
                    column: 1,
                },
                m::duplicate(&domain.name),
            );
        }
    }
}

fn read_route(r: &mut Reader, node: &Node) -> Option<RouteDecl> {
    let entries = r.mapping("routes", node)?;
    let (mut hostname, mut origin, mut path, mut tunnel, mut login, mut options) =
        (None, None, None, None, None, None);
    let mut skip_login: Option<(Vec<String>, Pos)> = None;
    let mut ok = true;
    for (key, at, value) in entries {
        match key {
            "hostname" => {
                hostname = r.hostname(value);
                ok &= hostname.is_some();
            }
            "origin" => {
                origin = r.text(key, value);
                if let Some(o) = &origin
                    && let Err(err) = RouteOrigin::parse(o)
                {
                    r.error(value.at, err.text());
                    ok = false;
                }
            }
            "path" => {
                path = r.text(key, value);
                if let Some(p) = &path
                    && let Err(err) = PathRule::parse(p)
                {
                    r.error(value.at, err.text());
                    ok = false;
                }
            }
            "tunnel" => tunnel = r.text(key, value),
            "login" => {
                login = r.login(value);
                ok &= login.is_some();
            }
            "skipLogin" => match r.texts(key, value) {
                Some(paths) => skip_login = Some((paths, value.at)),
                None => ok = false,
            },
            "originRequest" => {
                options = read_origin_request(r, value);
                ok &= options.is_some();
            }
            other => r.unknown(other, at),
        }
    }
    if let Some((paths, at)) = skip_login {
        match login.as_mut() {
            Some(rule) => {
                let with = AccessRule {
                    bypass: paths,
                    ..rule.clone()
                };
                match with.normalized() {
                    Ok(with) => *rule = with,
                    Err(err) => {
                        r.error(at, err.text());
                        ok = false;
                    }
                }
            }
            None => {
                r.error(at, m::skip_login_needs_login());
                ok = false;
            }
        }
    }
    let Some(hostname) = hostname else {
        if ok {
            r.error(node.at, m::missing("hostname"));
        }
        return None;
    };
    let Some(origin) = origin else {
        r.error(node.at, m::missing("origin"));
        return None;
    };
    ok.then_some(RouteDecl {
        hostname,
        origin,
        path,
        tunnel,
        login,
        origin_request: options,
        line: node.at.line,
    })
}

fn read_origin_request(r: &mut Reader, node: &Node) -> Option<OriginOptions> {
    let entries = r.mapping("originRequest", node)?;
    let mut map = serde_json::Map::new();
    for (key, at, value) in entries {
        if !crate::domain::ORIGIN_OPTION_KEYS.contains(&key) {
            r.unknown(key, at);
            continue;
        }
        let json = match &value.value {
            Value::Bool(b) => serde_json::Value::Bool(*b),
            Value::Int(n) => serde_json::Value::from(*n),
            Value::Str(s) => serde_json::Value::String(s.clone()),
            _ => {
                r.error(value.at, m::expected_text(key));
                return None;
            }
        };
        map.insert(key.to_owned(), json);
    }
    let options: OriginOptions = match serde_json::from_value(serde_json::Value::Object(map)) {
        Ok(options) => options,
        Err(_) => {
            r.error(node.at, m::bad_origin_request());
            return None;
        }
    };
    match options.validated() {
        Ok(options) => Some(options),
        Err(err) => {
            r.error(node.at, err.text());
            None
        }
    }
}

fn read_share(r: &mut Reader, node: &Node) -> Option<ShareDecl> {
    let entries = r.mapping("shares", node)?;
    let mut share = ShareDecl {
        origin: String::new(),
        hostname: None,
        expires_after: None,
        inspect: true,
        host_header: HostHeaderDecl::Auto,
        login: None,
        line: node.at.line,
    };
    let (mut port, mut url) = (None, None);
    let mut ok = true;
    for (key, at, value) in entries {
        match key {
            "port" => {
                port = r.port(value);
                ok &= port.is_some();
            }
            "url" => {
                url = r.text(key, value);
                if let Some(u) = &url
                    && let Err(err) = crate::domain::OriginUrl::parse(u)
                {
                    r.error(value.at, err.text());
                    ok = false;
                }
            }
            "hostname" => {
                share.hostname = r.hostname(value);
                ok &= share.hostname.is_some();
            }
            "expires" => {
                share.expires_after = r.duration(value, false);
                ok &= share.expires_after.is_some();
            }
            "inspect" => share.inspect = r.bool(key, value).unwrap_or(true),
            "hostHeader" => {
                share.host_header = match &value.value {
                    Value::Bool(false) => HostHeaderDecl::Off,
                    Value::Str(s) if s.trim() == "auto" => HostHeaderDecl::Auto,
                    Value::Str(s) => {
                        if crate::dev_server::parse_host_header(s).is_none() {
                            r.error(value.at, m::bad_host_header(s));
                            ok = false;
                        }
                        HostHeaderDecl::Set(s.trim().to_owned())
                    }
                    _ => {
                        r.error(
                            value.at,
                            m::bad_host_header(value.scalar().unwrap_or_default()),
                        );
                        ok = false;
                        HostHeaderDecl::Auto
                    }
                };
            }
            "login" => {
                share.login = r.login(value);
                ok &= share.login.is_some();
            }
            other => r.unknown(other, at),
        }
    }
    share.origin = match (port, url) {
        (Some(port), None) => format!("http://localhost:{port}"),
        (None, Some(url)) => url,
        (Some(_), Some(_)) => {
            r.error(node.at, m::share_both());
            return None;
        }
        (None, None) => {
            if ok {
                r.error(node.at, m::share_origin());
            }
            return None;
        }
    };
    if share.login.is_some() && share.hostname.is_none() {
        r.error(node.at, m::login_needs_hostname());
        return None;
    }
    ok.then_some(share)
}

fn read_snapshot(r: &mut Reader, node: &Node) -> Option<SnapshotDecl> {
    let entries = r.mapping("snapshots", node)?;
    let mut ok = true;
    let (mut name, mut source) = (None, None);
    let mut snapshot = SnapshotDecl {
        name: String::new(),
        source: SnapshotSourceDecl::Folder(String::new()),
        hostname: None,
        spa: false,
        password: None,
        login: None,
        expires_in_days: None,
        line: node.at.line,
    };
    for (key, at, value) in entries {
        match key {
            "name" => {
                name = r.text(key, value);
                if let Some(n) = &name
                    && let Err(err) = crate::snapshot::valid_name(n)
                {
                    r.error(value.at, err.text());
                    ok = false;
                }
            }
            "source" => {
                source = read_source(r, value);
                ok &= source.is_some();
            }
            "hostname" => {
                snapshot.hostname = r.hostname(value);
                ok &= snapshot.hostname.is_some();
            }
            "spa" => snapshot.spa = r.bool(key, value).unwrap_or(false),
            "password" => {
                snapshot.password = r.secret_ref(key, value);
                ok &= snapshot.password.is_some();
            }
            "login" => {
                snapshot.login = r.login(value);
                ok &= snapshot.login.is_some();
            }
            "expires" => {
                snapshot.expires_in_days = r.duration(value, true).map(|s| s / 86_400);
                ok &= snapshot.expires_in_days.is_some();
            }
            other => r.unknown(other, at),
        }
    }
    let (Some(name), Some(source)) = (name, source) else {
        if ok {
            r.error(node.at, m::missing("name, source"));
        }
        return None;
    };
    if snapshot.login.is_some() && snapshot.hostname.is_none() {
        r.error(node.at, m::login_needs_hostname());
        return None;
    }
    snapshot.name = name;
    snapshot.source = source;
    ok.then_some(snapshot)
}

fn read_source(r: &mut Reader, node: &Node) -> Option<SnapshotSourceDecl> {
    let entries = r.mapping("source", node)?;
    let [(kind, at, value)] = entries.as_slice() else {
        r.error(node.at, m::snapshot_source());
        return None;
    };
    let path = r.text(kind, value)?;
    if std::path::Path::new(&path).is_absolute() || path.split(['/', '\\']).any(|p| p == "..") {
        r.error(value.at, m::relative_path(&path));
        return None;
    }
    match *kind {
        "folder" => Some(SnapshotSourceDecl::Folder(path)),
        "build" => Some(SnapshotSourceDecl::Build(path)),
        _ => {
            r.error(*at, m::snapshot_source());
            None
        }
    }
}

fn read_local_domain(r: &mut Reader, node: &Node) -> Option<LocalDomainDecl> {
    let entries = r.mapping("localDomains", node)?;
    let (mut name, mut port, mut wildcard) = (None, None, false);
    let mut ok = true;
    for (key, at, value) in entries {
        match key {
            "name" => {
                name = r.text(key, value).map(|n| n.to_ascii_lowercase());
                if let Some(n) = &name
                    && !valid_local_name(n)
                {
                    r.error(value.at, m::local_name(n));
                    ok = false;
                }
            }
            "port" => {
                port = r.port(value);
                ok &= port.is_some();
            }
            "wildcard" => wildcard = r.bool(key, value).unwrap_or(false),
            other => r.unknown(other, at),
        }
    }
    let (Some(name), Some(port)) = (name, port) else {
        if ok {
            r.error(node.at, m::missing("name, port"));
        }
        return None;
    };
    ok.then_some(LocalDomainDecl {
        name,
        port,
        wildcard,
        line: node.at.line,
    })
}
