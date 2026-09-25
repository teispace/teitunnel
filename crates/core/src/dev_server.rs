//! Dev servers that refuse requests for addresses they don't know (M12-01).
//!
//! Since early 2025 most dev servers check the `Host` header against an allow list to
//! stop DNS-rebinding attacks, so a request for `quiet-river.trycloudflare.com` gets a
//! short error page instead of the app. The verifier recognises those answers
//! ([`detect`]); this module knows the two ways out: send the origin's own Host header
//! (`--http-host-header` for a Quick Share, `httpHostHeader` for a route), or add the
//! public hostname to the server's allow list ([`HostRejection::config_line`]).
//!
//! Sending another Host header is only safe where the server reads Host for that check
//! alone. Next.js, Rails, Django, SvelteKit and Astro also compare the browser's
//! `Origin` with it (server actions, CSRF protection) or build redirects and OAuth
//! callbacks from it, so for them the config line is the fix.

use std::time::Duration;

use serde::Serialize;

use crate::{discovery::ServiceKind, domain::RouteOrigin};

/// The most of an answer read to recognise it: dev-server and Cloudflare error pages are
/// a few kilobytes at most, and a large page is never one of them.
pub const BODY_LIMIT: usize = 64 * 1024;

/// A dev server that checks the Host header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum DevServer {
    /// Vite 5.4.12+ / 6.0.9+ (`server.allowedHosts`), also Laravel's and Remix's.
    Vite,
    /// SvelteKit (Vite).
    SvelteKit,
    /// Astro (Vite).
    Astro,
    /// Nuxt (Vite).
    Nuxt,
    /// Angular CLI (`ng serve`, Vite or webpack).
    Angular,
    /// webpack-dev-server (webpack, Create React App, Vue CLI).
    Webpack,
    /// Next.js 15.2+ (`allowedDevOrigins`).
    Next,
    /// Rails 6+ (`ActionDispatch::HostAuthorization`).
    Rails,
    /// Django (`ALLOWED_HOSTS`).
    Django,
}

impl DevServer {
    /// Its name, as people write it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Vite => "Vite",
            Self::SvelteKit => "SvelteKit",
            Self::Astro => "Astro",
            Self::Nuxt => "Nuxt",
            Self::Angular => "Angular",
            Self::Webpack => "webpack-dev-server",
            Self::Next => "Next.js",
            Self::Rails => "Rails",
            Self::Django => "Django",
        }
    }

    /// The dev server a discovered process is, if it's one that checks hosts.
    pub fn of_kind(kind: ServiceKind) -> Option<Self> {
        Some(match kind {
            ServiceKind::Vite | ServiceKind::Remix => Self::Vite,
            ServiceKind::SvelteKit => Self::SvelteKit,
            ServiceKind::Astro => Self::Astro,
            ServiceKind::Nuxt => Self::Nuxt,
            ServiceKind::Angular => Self::Angular,
            ServiceKind::Webpack => Self::Webpack,
            ServiceKind::Next => Self::Next,
            ServiceKind::Rails => Self::Rails,
            ServiceKind::Django => Self::Django,
            _ => return None,
        })
    }

    /// Whether sending the origin's own Host header fixes it and breaks nothing: the
    /// server reads Host only for its rebinding check. New shares of these send it from
    /// the start.
    pub fn host_header_safe(self) -> bool {
        matches!(
            self,
            Self::Vite | Self::Nuxt | Self::Angular | Self::Webpack
        )
    }

    /// Whether the Host header matters at all (Next.js checks `Origin` instead).
    pub fn host_header_helps(self) -> bool {
        self != Self::Next
    }

    /// An answer shows the engine (Vite's message is the same in every Vite-based
    /// framework); discovery knows the framework.
    #[must_use]
    pub fn refine(self, kind: Option<ServiceKind>) -> Self {
        match (self, kind.and_then(Self::of_kind)) {
            (
                Self::Vite,
                Some(framework @ (Self::SvelteKit | Self::Astro | Self::Nuxt | Self::Angular)),
            )
            | (Self::Webpack, Some(framework @ Self::Angular)) => framework,
            (server, _) => server,
        }
    }

    /// The file to edit and the line that allows `host` there. A `trycloudflare.com`
    /// address changes with every Quick Share, so for those the line allows them all.
    pub fn config(self, host: &str) -> (&'static str, String) {
        let quick = host.ends_with(".trycloudflare.com");
        // `.example.com` also allows subdomains in Vite, webpack, Angular, Rails and
        // Django; Next.js takes `*.example.com`.
        let dotted = if quick { ".trycloudflare.com" } else { host };
        let wildcard = if quick { "*.trycloudflare.com" } else { host };
        match self {
            Self::Vite | Self::SvelteKit => (
                "vite.config.js",
                format!("server: {{ allowedHosts: ['{dotted}'] }}"),
            ),
            Self::Astro => (
                "astro.config.mjs",
                format!("vite: {{ server: {{ allowedHosts: ['{dotted}'] }} }}"),
            ),
            Self::Nuxt => (
                "nuxt.config.ts",
                format!("vite: {{ server: {{ allowedHosts: ['{dotted}'] }} }}"),
            ),
            Self::Angular => ("angular.json", format!("\"allowedHosts\": [\"{dotted}\"]")),
            Self::Webpack => (
                "webpack.config.js",
                format!("devServer: {{ allowedHosts: ['{dotted}'] }}"),
            ),
            Self::Next => (
                "next.config.js",
                format!("allowedDevOrigins: ['{wildcard}']"),
            ),
            Self::Rails => (
                "config/environments/development.rb",
                format!("config.hosts << \"{dotted}\""),
            ),
            // With DEBUG on and the list empty, Django allows localhost; a list replaces
            // that, so it keeps localhost. Forms also check Origin.
            Self::Django => (
                "settings.py",
                format!(
                    "ALLOWED_HOSTS = [\"{dotted}\", \"localhost\", \"127.0.0.1\"]\nCSRF_TRUSTED_ORIGINS = [\"https://{wildcard}\"]"
                ),
            ),
        }
    }
}

/// A dev server refused a request for the public address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct HostRejection {
    /// Which server.
    pub server: DevServer,
    /// The public hostname it refused.
    pub host: String,
    /// The Host header that makes it answer (the origin's own address), when sending it
    /// helps and isn't sent already.
    pub host_header: Option<String>,
    /// Sending that header breaks nothing (see [`DevServer::host_header_safe`]). When
    /// false, the config line is the recommended fix.
    pub host_header_safe: bool,
    /// The file the config line goes in.
    pub config_file: String,
    /// The line that allows the address.
    pub config_line: String,
}

impl HostRejection {
    /// What to do about `server` refusing `host`, for an origin at `origin`.
    pub fn new(server: DevServer, host: &str, origin: Option<&RouteOrigin>) -> Self {
        let (file, line) = server.config(host);
        Self {
            server,
            host: host.to_owned(),
            host_header: origin
                .filter(|_| server.host_header_helps())
                .and_then(host_header_for),
            host_header_safe: server.host_header_safe(),
            config_file: file.to_owned(),
            config_line: line,
        }
    }
}

/// The Host header a local server expects: its own address, `localhost:<port>` for one
/// on this machine. `None` for origins that aren't HTTP.
pub fn host_header_for(origin: &RouteOrigin) -> Option<String> {
    let (scheme, rest) = origin.as_str().split_once("://")?;
    if scheme != "http" && scheme != "https" {
        return None;
    }
    let authority = rest.split('/').next().filter(|a| !a.is_empty())?;
    Some(match origin.port() {
        Some(port) if origin.is_local() => format!("localhost:{port}"),
        _ => authority.to_owned(),
    })
}

/// The Host header a new share of `origin` sends unless told otherwise: the origin's
/// own address, when discovery finds a dev server there that refuses unknown hosts and
/// for which sending it is safe ([`DevServer::host_header_safe`]).
pub async fn default_host_header(origin: &RouteOrigin) -> Option<(String, DevServer)> {
    if !origin.is_local() {
        return None;
    }
    let kind = crate::discovery::kind_on_port(origin.port()?).await?;
    default_for(kind, origin)
}

/// [`default_host_header`] for a service discovery already classified.
pub fn default_for(kind: ServiceKind, origin: &RouteOrigin) -> Option<(String, DevServer)> {
    let server = DevServer::of_kind(kind).filter(|s| s.host_header_safe())?;
    Some((host_header_for(origin)?, server))
}

/// A Host header as typed (`localhost:5173`, `app.local`, `[::1]:3000`), trimmed, or
/// `None` if it isn't one.
pub fn parse_host_header(value: &str) -> Option<String> {
    let value = value.trim();
    let valid = !value.is_empty()
        && value.len() <= 255
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._:[]".contains(c));
    valid.then(|| value.to_owned())
}

/// Recognises a dev server's refusal from its status and (bounded) body:
///
/// - Vite 5.4.12+ / 6.0.9+ (and every framework on it): 403 `text/plain`,
///   ``Blocked request. This host ("…") is not allowed. To allow this host, add "…" to
///   `server.allowedHosts` in vite.config.js.`` (`host-validation-middleware`).
/// - webpack-dev-server 4/5 (also Angular's webpack builder, Create React App, Vue CLI):
///   403 `Invalid Host header`; version 3 answers the same text with 200.
/// - Rails 6.0: 403 `Blocked host: …`; 6.1+: `Blocked hosts: …`, both with the
///   `config.hosts << "…"` line (the `<<` HTML-escaped).
/// - Django with `DEBUG` on: 400 `DisallowedHost … Invalid HTTP_HOST header: '…'`.
///
/// Next.js is recognised separately ([`detect_next`]): it doesn't check Host, it blocks
/// cross-origin requests for its dev resources.
pub fn detect(status: u16, body: &str) -> Option<DevServer> {
    let short = body.trim();
    match status {
        403 if body.contains("Blocked request. This host (") && body.contains("allowedHosts") => {
            Some(DevServer::Vite)
        }
        200 | 403 if short.starts_with("Invalid Host header") && short.len() < 64 => {
            Some(DevServer::Webpack)
        }
        403 if (body.contains("Blocked host: ") || body.contains("Blocked hosts: "))
            && body.contains("config.hosts") =>
        {
            Some(DevServer::Rails)
        }
        400 if body.contains("Invalid HTTP_HOST header") || body.contains("DisallowedHost") => {
            Some(DevServer::Django)
        }
        _ => None,
    }
}

/// Django with `DEBUG` off answers a disallowed host with its bare 400 page, which only
/// says something when discovery knows the origin is Django.
pub fn is_plain_django_400(status: u16, body: &str) -> bool {
    status == 400 && body.contains("<h1>Bad Request (400)</h1>")
}

/// Next.js 16 (15.2+ once `allowedDevOrigins` is set) answers a cross-origin request
/// for a dev resource under `/_next/` with 403 `Unauthorized`
/// (`router-utils/block-cross-site-dev.ts`). The page itself loads, but its scripts and
/// hot reload don't.
pub fn detect_next(status: u16, body: &str) -> bool {
    status == 403 && body.trim() == "Unauthorized"
}

/// The path [`detect_next`] probes: a dev resource under `/_next/` that isn't exempt.
pub const NEXT_PROBE_PATH: &str = "/_next/webpack-hmr";

/// Whether an answer is a Server-Sent Events stream.
pub fn is_event_stream(content_type: Option<&str>) -> bool {
    content_type.is_some_and(|ct| {
        ct.trim_start()
            .to_ascii_lowercase()
            .starts_with("text/event-stream")
    })
}

/// Paths a local server usually streams events on: the page itself, and the MCP
/// transports' (`/sse` for HTTP+SSE, `/mcp` for Streamable HTTP).
const EVENT_STREAM_PATHS: [&str; 3] = ["/", "/sse", "/mcp"];

/// Whether the service at `origin` (a local URL like `http://localhost:3000`) answers
/// with Server-Sent Events on one of the usual paths. Only headers are read.
pub async fn serves_event_stream(origin: &str) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .build()
    else {
        return false;
    };
    let base = origin.trim_end_matches('/');
    let checks = EVENT_STREAM_PATHS.map(|path| {
        let request = client
            .get(format!("{base}{path}"))
            .header(reqwest::header::ACCEPT, "text/event-stream");
        async move {
            let Ok(response) = request.send().await else {
                return false;
            };
            is_event_stream(
                response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok()),
            )
        }
    });
    let [a, b, c] = checks;
    let (a, b, c) = tokio::join!(a, b, c);
    a || b || c
}

/// Reads at most [`BODY_LIMIT`] bytes of a response body, as text.
pub(crate) async fn read_limited(mut response: reqwest::Response) -> String {
    let mut body = Vec::new();
    while body.len() < BODY_LIMIT {
        match response.chunk().await {
            Ok(Some(chunk)) => body.extend_from_slice(&chunk),
            _ => break,
        }
    }
    body.truncate(BODY_LIMIT);
    String::from_utf8_lossy(&body).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const VITE: &str = include_str!("dev_server/fixtures/vite-6.txt");
    const VITE_PREVIEW: &str = include_str!("dev_server/fixtures/vite-preview.txt");
    const RAILS_7: &str = include_str!("dev_server/fixtures/rails-7.html");
    const RAILS_6: &str = include_str!("dev_server/fixtures/rails-6.html");
    const DJANGO_DEBUG: &str = include_str!("dev_server/fixtures/django-debug.html");
    const DJANGO: &str = include_str!("dev_server/fixtures/django.html");

    #[test]
    fn recognises_each_servers_refusal() {
        assert_eq!(detect(403, VITE), Some(DevServer::Vite));
        assert_eq!(detect(403, VITE_PREVIEW), Some(DevServer::Vite));
        // webpack-dev-server 4/5 answer 403, version 3 answers 200.
        assert_eq!(detect(403, "Invalid Host header"), Some(DevServer::Webpack));
        assert_eq!(detect(200, "Invalid Host header"), Some(DevServer::Webpack));
        assert_eq!(detect(403, RAILS_7), Some(DevServer::Rails));
        assert_eq!(detect(403, RAILS_6), Some(DevServer::Rails));
        assert_eq!(detect(400, DJANGO_DEBUG), Some(DevServer::Django));
        assert!(detect_next(403, "Unauthorized"));
        assert!(is_plain_django_400(400, DJANGO));
    }

    #[test]
    fn ignores_ordinary_answers() {
        // The status has to match too: a page quoting the message isn't a refusal.
        assert_eq!(detect(200, VITE), None);
        assert_eq!(detect(404, RAILS_7), None);
        assert_eq!(detect(200, DJANGO_DEBUG), None);
        // A page that merely starts with the words isn't webpack's answer.
        let long = format!("Invalid Host header{}", " lorem".repeat(40));
        assert_eq!(detect(403, &long), None);
        assert_eq!(detect(403, "Forbidden"), None);
        assert_eq!(detect(400, DJANGO), None, "needs discovery to say Django");
        assert!(!detect_next(403, "Forbidden"));
        assert!(!detect_next(200, "Unauthorized"));
        assert!(!is_plain_django_400(200, DJANGO));
    }

    #[test]
    fn config_lines_allow_every_quick_share_or_the_exact_host() {
        let quick = "quiet-river-lamp.trycloudflare.com";
        assert_eq!(
            DevServer::Vite.config(quick),
            (
                "vite.config.js",
                "server: { allowedHosts: ['.trycloudflare.com'] }".to_owned()
            )
        );
        assert_eq!(
            DevServer::Vite.config("app.example.com").1,
            "server: { allowedHosts: ['app.example.com'] }"
        );
        assert_eq!(
            DevServer::Next.config(quick).1,
            "allowedDevOrigins: ['*.trycloudflare.com']"
        );
        assert_eq!(
            DevServer::Rails.config("app.example.com"),
            (
                "config/environments/development.rb",
                "config.hosts << \"app.example.com\"".to_owned()
            )
        );
        let (file, django) = DevServer::Django.config(quick);
        assert_eq!(file, "settings.py");
        assert!(
            django.contains("\".trycloudflare.com\", \"localhost\""),
            "{django}"
        );
        assert!(django.contains("https://*.trycloudflare.com"), "{django}");
        assert!(DevServer::Nuxt.config(quick).1.starts_with("vite: {"));
        assert!(DevServer::Astro.config(quick).1.starts_with("vite: {"));
        assert_eq!(
            DevServer::Angular.config(quick),
            (
                "angular.json",
                "\"allowedHosts\": [\".trycloudflare.com\"]".to_owned()
            )
        );
        assert!(
            DevServer::Webpack
                .config(quick)
                .1
                .starts_with("devServer: {")
        );
    }

    #[test]
    fn the_host_header_is_offered_where_it_helps() {
        let local = RouteOrigin::parse("http://localhost:5173").unwrap();
        let vite = HostRejection::new(DevServer::Vite, "a.trycloudflare.com", Some(&local));
        assert_eq!(vite.host_header.as_deref(), Some("localhost:5173"));
        assert!(vite.host_header_safe);

        let next = HostRejection::new(DevServer::Next, "a.trycloudflare.com", Some(&local));
        assert_eq!(next.host_header, None, "Next.js checks Origin, not Host");

        let rails = HostRejection::new(DevServer::Rails, "app.example.com", Some(&local));
        assert_eq!(rails.host_header.as_deref(), Some("localhost:5173"));
        assert!(!rails.host_header_safe, "forms and redirects use Host");

        let lan = RouteOrigin::parse("http://192.168.1.20:8080").unwrap();
        assert_eq!(host_header_for(&lan).as_deref(), Some("192.168.1.20:8080"));
        let ssh = RouteOrigin::parse("ssh://localhost:22").unwrap();
        assert_eq!(host_header_for(&ssh), None);
        assert_eq!(
            host_header_for(&RouteOrigin::parse("127.0.0.1:3000").unwrap()).as_deref(),
            Some("localhost:3000")
        );
    }

    #[test]
    fn discovery_names_the_framework_behind_vite() {
        assert_eq!(
            DevServer::Vite.refine(Some(ServiceKind::SvelteKit)),
            DevServer::SvelteKit
        );
        assert_eq!(
            DevServer::Webpack.refine(Some(ServiceKind::Angular)),
            DevServer::Angular
        );
        assert_eq!(
            DevServer::Vite.refine(Some(ServiceKind::Remix)),
            DevServer::Vite
        );
        assert_eq!(
            DevServer::Rails.refine(Some(ServiceKind::Vite)),
            DevServer::Rails
        );
        assert_eq!(DevServer::Vite.refine(None), DevServer::Vite);
        assert!(DevServer::of_kind(ServiceKind::Flask).is_none());
        for safe in [
            DevServer::Vite,
            DevServer::Nuxt,
            DevServer::Angular,
            DevServer::Webpack,
        ] {
            assert!(safe.host_header_safe());
        }
        for unsafe_ in [
            DevServer::Next,
            DevServer::Rails,
            DevServer::Django,
            DevServer::SvelteKit,
            DevServer::Astro,
        ] {
            assert!(!unsafe_.host_header_safe(), "{unsafe_:?}");
        }
    }

    #[test]
    fn new_shares_of_safe_dev_servers_send_their_own_host() {
        let vite = RouteOrigin::parse("http://localhost:5173").unwrap();
        assert_eq!(
            default_for(ServiceKind::Vite, &vite),
            Some(("localhost:5173".to_owned(), DevServer::Vite))
        );
        assert_eq!(
            default_for(ServiceKind::Angular, &vite).map(|(_, s)| s),
            Some(DevServer::Angular)
        );
        // Not where it breaks origin checks, nor for servers that don't check.
        assert_eq!(default_for(ServiceKind::Next, &vite), None);
        assert_eq!(default_for(ServiceKind::Rails, &vite), None);
        assert_eq!(default_for(ServiceKind::SvelteKit, &vite), None);
        assert_eq!(default_for(ServiceKind::Flask, &vite), None);
    }

    #[test]
    fn parses_host_headers() {
        assert_eq!(
            parse_host_header(" localhost:5173 ").as_deref(),
            Some("localhost:5173")
        );
        assert_eq!(
            parse_host_header("[::1]:3000").as_deref(),
            Some("[::1]:3000")
        );
        let long = "a".repeat(256);
        for bad in [
            "",
            "   ",
            "local host",
            "a\r\nX-Evil: 1",
            "a/b",
            long.as_str(),
        ] {
            assert_eq!(parse_host_header(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn event_streams_by_content_type() {
        assert!(is_event_stream(Some("text/event-stream")));
        assert!(is_event_stream(Some("Text/Event-Stream; charset=utf-8")));
        assert!(!is_event_stream(Some("text/html")));
        assert!(!is_event_stream(None));
    }

    #[tokio::test]
    async fn finds_an_mcp_server_on_sse() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};
        let server = MockServer::start().await;
        Mock::given(path("/sse"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw("event: endpoint\ndata: /messages\n\n", "text/event-stream"),
            )
            .mount(&server)
            .await;
        assert!(serves_event_stream(&server.uri()).await);

        let plain = MockServer::start().await;
        Mock::given(wiremock::matchers::any())
            .respond_with(ResponseTemplate::new(200).set_body_string("<html></html>"))
            .mount(&plain)
            .await;
        assert!(!serves_event_stream(&plain.uri()).await);
    }
}
