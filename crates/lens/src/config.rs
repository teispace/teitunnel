//! Tap configuration: the upstream, capture limits, forwarding, and the per-tap features
//! (gates, stubs, header rules, injection, paused page).

use std::{fmt, path::PathBuf, sync::Arc, time::Duration};

use http::uri::Scheme;
use serde::{Deserialize, Serialize};

use crate::{
    FaultRule, Gates, HeaderRules, Injection, LensError, NetworkConfig, ReservedHandler, StubRule,
    TapId,
};

/// Default cap on captured bytes per body.
pub const DEFAULT_MAX_BODY_BYTES: usize = 1024 * 1024;

/// Where a tap sends requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Upstream {
    /// An HTTP(S) origin server.
    Origin(OriginConfig),
    /// A folder served as static files.
    Folder(FolderConfig),
}

impl Upstream {
    /// An origin at `url` (e.g. `http://localhost:3000`) with default settings.
    ///
    /// # Errors
    /// [`LensError::InvalidUpstream`] for malformed or non-HTTP URLs.
    pub fn origin(url: &str) -> Result<Self, LensError> {
        Ok(Self::Origin(OriginConfig::new(OriginUrl::parse(url)?)))
    }

    /// A folder served with default settings (index files, no listing, no SPA fallback).
    pub fn folder(root: impl Into<PathBuf>) -> Self {
        Self::Folder(FolderConfig::new(root))
    }

    /// Human-readable description, e.g. `http://localhost:3000` or `folder /srv/site`.
    pub fn describe(&self) -> String {
        match self {
            Self::Origin(origin) => origin.url.to_string(),
            Self::Folder(folder) => format!("folder {}", folder.root.display()),
        }
    }
}

/// A parsed origin URL: scheme, host and port, no path.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OriginUrl {
    https: bool,
    host: String,
    port: u16,
}

impl OriginUrl {
    /// Parses `http://host[:port]` or `https://host[:port]` (a trailing `/` is fine).
    ///
    /// # Errors
    /// [`LensError::InvalidUpstream`] for other schemes, missing hosts, paths or queries.
    pub fn parse(url: &str) -> Result<Self, LensError> {
        let invalid = |why: &str| LensError::InvalidUpstream(format!("{url:?}: {why}"));
        let uri: http::Uri = url.parse().map_err(|_| invalid("not a valid URL"))?;
        let https = match uri.scheme() {
            Some(scheme) if *scheme == Scheme::HTTP => false,
            Some(scheme) if *scheme == Scheme::HTTPS => true,
            _ => return Err(invalid("only http:// and https:// origins are supported")),
        };
        let authority = uri.authority().ok_or_else(|| invalid("missing host"))?;
        if authority.as_str().contains('@') {
            return Err(invalid("credentials in the URL aren't supported"));
        }
        let host = authority
            .host()
            .trim_start_matches('[')
            .trim_end_matches(']');
        if host.is_empty() {
            return Err(invalid("missing host"));
        }
        if uri.path() != "/" && !uri.path().is_empty() {
            return Err(invalid("origins can't have a path"));
        }
        if uri.query().is_some() {
            return Err(invalid("origins can't have a query"));
        }
        let port = authority.port_u16().unwrap_or(if https { 443 } else { 80 });
        Ok(Self {
            https,
            host: host.to_ascii_lowercase(),
            port,
        })
    }

    /// Whether the origin speaks TLS.
    pub fn is_https(&self) -> bool {
        self.https
    }

    /// Host name or IP (IPv6 without brackets).
    pub fn host(&self) -> &str {
        &self.host
    }

    /// Port.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `http` or `https`.
    pub fn scheme(&self) -> &'static str {
        if self.https { "https" } else { "http" }
    }

    /// `host[:port]` as a `Host` header (the port is omitted when it's the default).
    pub fn authority(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let default = if self.https { 443 } else { 80 };
        if self.port == default {
            host
        } else {
            format!("{host}:{}", self.port)
        }
    }
}

impl fmt::Display for OriginUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}://{}", self.scheme(), self.authority())
    }
}

/// Settings for an origin upstream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginConfig {
    /// Where the origin listens.
    pub url: OriginUrl,
    /// Verify the origin's TLS certificate (https only). Turn off for self-signed
    /// development certificates.
    pub verify_tls: bool,
    /// Server name for SNI and certificate verification, when it differs from the URL's
    /// host (cloudflared's `originServerName`).
    pub server_name: Option<String>,
    /// Speak HTTP/2 to the origin (h2c for http, ALPN h2 for https); needed for gRPC.
    pub http2: bool,
    /// How long to wait for a TCP (and TLS) connection.
    pub connect_timeout: Duration,
    /// How long to wait for the response head; `None` waits forever.
    pub response_timeout: Option<Duration>,
    /// Idle keep-alive connections kept per origin.
    pub max_idle_connections: usize,
    /// How long an idle connection is kept.
    pub idle_timeout: Duration,
}

impl OriginConfig {
    /// Default settings for `url`.
    pub fn new(url: OriginUrl) -> Self {
        Self {
            url,
            verify_tls: true,
            server_name: None,
            http2: false,
            connect_timeout: Duration::from_secs(5),
            response_timeout: Some(Duration::from_secs(120)),
            max_idle_connections: 32,
            idle_timeout: Duration::from_secs(60),
        }
    }
}

/// Settings for a folder upstream (a static file server).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FolderConfig {
    /// The folder. Nothing outside it is ever served (symlinks that leave it are refused).
    pub root: PathBuf,
    /// Serve `index.html` for directory requests.
    pub index: bool,
    /// Show a listing for directories without an index file.
    pub listing: bool,
    /// Single-page app: serve `/index.html` for unknown paths that accept HTML.
    pub spa_fallback: bool,
    /// Serve dotfiles (`.env`, `.git/…`). Off by default.
    pub hidden: bool,
    /// Names that are never served or listed, whatever `hidden` says (the embedder's
    /// rules for secrets and tooling, e.g. `.env*`, keys, `node_modules`).
    #[serde(skip)]
    pub allow: NameFilter,
}

impl FolderConfig {
    /// Default settings for `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            index: true,
            listing: false,
            spa_fallback: false,
            hidden: false,
            allow: NameFilter::default(),
        }
    }
}

/// Which file and folder names a folder upstream may serve: called with each path
/// segment's name and whether it names a folder; `false` refuses it (404) and hides it
/// from listings. The default allows everything.
#[derive(Clone, Default)]
pub struct NameFilter(Option<Arc<NameRule>>);

/// A name rule: `(name, is_dir) -> allowed`.
type NameRule = dyn Fn(&str, bool) -> bool + Send + Sync;

impl NameFilter {
    /// A filter allowing the names `allow` accepts.
    pub fn new(allow: impl Fn(&str, bool) -> bool + Send + Sync + 'static) -> Self {
        Self(Some(Arc::new(allow)))
    }

    /// Whether `name` (a folder when `is_dir`) may be served.
    pub fn allows(&self, name: &str, is_dir: bool) -> bool {
        self.0.as_ref().is_none_or(|allow| allow(name, is_dir))
    }
}

impl fmt::Debug for NameFilter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(if self.0.is_some() {
            "NameFilter(custom)"
        } else {
            "NameFilter(all)"
        })
    }
}

impl PartialEq for NameFilter {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for NameFilter {}

/// Which `Host` header the upstream receives.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HostHeader {
    /// The visitor's host, as cloudflared sent it (default; what the origin would see
    /// without Lens).
    #[default]
    Preserve,
    /// The upstream's own `host:port` (fixes dev servers that reject unknown hosts).
    Upstream,
    /// A fixed value.
    Custom(String),
}

/// How `X-Forwarded-*` headers are handled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ForwardedHeaders {
    /// Keep what cloudflared sent; add `X-Forwarded-For`/`-Proto`/`-Host` only when
    /// missing (default).
    #[default]
    Preserve,
    /// Replace them with Lens's view (client IP, listener scheme, original host).
    Replace,
    /// Leave headers exactly as received.
    Off,
}

/// Capture limits for a tap.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfig {
    /// Record exchanges at all (metrics are kept either way).
    pub enabled: bool,
    /// Bytes captured per body (request and response each); the rest streams through.
    pub max_body_bytes: usize,
    /// Exchanges kept in the ring (`None`: the store's default, 1,000).
    pub capacity: Option<usize>,
    /// WebSocket/SSE messages previewed per exchange.
    pub stream_previews: usize,
    /// Bytes kept per message preview.
    pub preview_bytes: usize,
    /// WebSocket frames kept per exchange (the most recent; older ones are counted).
    pub ws_frames: usize,
    /// Bytes kept per WebSocket frame preview.
    pub frame_preview_bytes: usize,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_body_bytes: DEFAULT_MAX_BODY_BYTES,
            capacity: None,
            stream_previews: 20,
            preview_bytes: 1024,
            ws_frames: 500,
            frame_preview_bytes: 4 * 1024,
        }
    }
}

/// The page served while a tap is paused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct PausedPage {
    /// Heading.
    pub title: String,
    /// Explanation below the heading.
    pub message: String,
    /// `Retry-After`, in seconds.
    pub retry_after_secs: u32,
}

impl Default for PausedPage {
    fn default() -> Self {
        Self {
            title: "Paused".into(),
            message: "This site is paused for a moment. Please try again soon.".into(),
            retry_after_secs: 60,
        }
    }
}

/// Everything about one tap. Build with [`TapConfig::new`] and adjust fields.
#[derive(Debug, Clone)]
pub struct TapConfig {
    /// Stable id chosen by the embedder, or `None` for a generated one.
    pub id: Option<TapId>,
    /// Display name (e.g. the share's hostname).
    pub name: String,
    /// Where requests go.
    pub upstream: Upstream,
    /// Capture limits.
    pub capture: CaptureConfig,
    /// The `Host` header sent upstream.
    pub host_header: HostHeader,
    /// `X-Forwarded-*` handling.
    pub forwarded: ForwardedHeaders,
    /// Use `CF-Connecting-IP` as the client IP for gates and `X-Forwarded-For` (only
    /// safe when the listener is reachable solely through cloudflared).
    pub trust_cf_connecting_ip: bool,
    /// Protection (password, secret link, basic auth, IP and user-agent rules).
    pub gates: Gates,
    /// Canned responses, checked in order.
    pub stubs: Vec<StubRule>,
    /// Header rewrites and the CORS helper.
    pub headers: HeaderRules,
    /// Snippet injected into HTML pages.
    pub injection: Option<Injection>,
    /// Handler for `/__teitunnel/…` paths (overlay assets and APIs).
    pub reserved: Option<Arc<dyn ReservedHandler>>,
    /// Serve the paused page instead of forwarding.
    pub paused: Option<PausedPage>,
    /// For `text/event-stream` responses: write `: keep-alive` after this much
    /// downstream silence (at an event boundary), so Cloudflare doesn't end the stream
    /// after 100 s. `None` turns it off.
    pub sse_keepalive: Option<Duration>,
    /// Simulated latency and bandwidth.
    pub network: NetworkConfig,
    /// Fault injection, checked in order.
    pub faults: Vec<FaultRule>,
    /// Requests to stop for a look (the first matching rule applies); only while
    /// capturing.
    pub breakpoints: Vec<crate::BreakpointRule>,
}

impl TapConfig {
    /// A tap forwarding to `upstream` with defaults everywhere else.
    pub fn new(upstream: Upstream) -> Self {
        Self {
            id: None,
            name: String::new(),
            upstream,
            capture: CaptureConfig::default(),
            host_header: HostHeader::default(),
            forwarded: ForwardedHeaders::default(),
            trust_cf_connecting_ip: false,
            gates: Gates::default(),
            stubs: Vec::new(),
            headers: HeaderRules::default(),
            injection: None,
            reserved: None,
            paused: None,
            sse_keepalive: Some(crate::keepalive::DEFAULT_SSE_KEEPALIVE),
            network: NetworkConfig::default(),
            faults: Vec::new(),
            breakpoints: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_origin_urls() {
        let url = OriginUrl::parse("http://localhost:3000").unwrap();
        assert_eq!(
            (url.host(), url.port(), url.is_https()),
            ("localhost", 3000, false)
        );
        assert_eq!(url.authority(), "localhost:3000");
        assert_eq!(url.to_string(), "http://localhost:3000");

        let url = OriginUrl::parse("https://Example.test/").unwrap();
        assert_eq!(
            (url.port(), url.authority()),
            (443, "example.test".to_owned())
        );

        let url = OriginUrl::parse("http://[::1]:8080").unwrap();
        assert_eq!(url.host(), "::1");
        assert_eq!(url.authority(), "[::1]:8080");
    }

    #[test]
    fn rejects_bad_origins() {
        for bad in [
            "localhost:3000",
            "ftp://x",
            "http://",
            "http://a/path",
            "http://a/?q=1",
            "http://user:pw@a",
            "unix:/tmp/sock",
        ] {
            assert!(OriginUrl::parse(bad).is_err(), "{bad}");
        }
    }
}
