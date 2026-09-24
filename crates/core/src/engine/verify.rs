//! The verifier: an end-to-end check of a route, reported by stage so a failure says
//! what to fix (ARCHITECTURE §4.5).
//!
//! It never queries DNS for the route's hostname. A lookup made before a new record has
//! propagated caches NXDOMAIN (for up to 30 minutes) in the Mac's and the ISP's
//! resolvers, which would break the URL for the user (D-037). Instead, the DNS stage
//! reads the record through the API, and the HTTPS probe connects straight to a
//! Cloudflare edge address (any edge address serves any proxied hostname) with the
//! hostname as SNI and Host.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};

use serde::Serialize;
use tokio::net::{TcpStream, lookup_host};

use super::types::{Snapshot, tunnel_target};
use crate::dev_server::{self, DevServer, HostRejection};
use crate::discovery::{self, ServiceKind};
use crate::domain::{Hostname, RouteOrigin};
use crate::text::Text;

/// How long one HTTPS probe may take.
const PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Which part of the path from the internet to the origin a result is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// The DNS record points at the tunnel.
    Dns,
    /// Cloudflare's edge answers for the hostname.
    Edge,
    /// The edge reaches this Mac's connector.
    Tunnel,
    /// The connector reaches the origin.
    Origin,
}

/// Why a route doesn't work.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Failure {
    /// There's no DNS record for the hostname.
    NoRecord,
    /// The record exists but doesn't point at this Mac's tunnel (or isn't proxied).
    RecordElsewhere {
        /// What it points at.
        content: String,
    },
    /// The edge couldn't be reached (network, firewall).
    EdgeUnreachable {
        /// The error.
        message: String,
    },
    /// The certificate doesn't cover the hostname (Universal SSL covers one level).
    CertificateNotCovered,
    /// Cloudflare doesn't know the hostname yet (error 1001), usually propagation.
    NotOnCloudflareYet,
    /// No connector is connected to the tunnel (error 1033).
    NoConnector,
    /// The record points at a tunnel that doesn't serve it (error 1016 / 530).
    TunnelMismatch,
    /// The connector couldn't reach the origin (502).
    OriginUnreachable {
        /// Whether something listens on the origin's local port (None: not local).
        listening: Option<bool>,
        /// The origin's port, when it has one.
        port: Option<u16>,
    },
    /// The origin didn't answer in time (504).
    OriginTimeout,
    /// A dev server refused the public address (its Host or Origin check).
    HostRejected {
        /// Which server, and the ways to fix it.
        rejection: HostRejection,
    },
    /// Cloudflare refused a request body over its limit (413): 100 MB on the Free and
    /// Pro plans.
    BodyTooLarge,
    /// A Quick Share had 200 requests in flight, its limit (429).
    TooManyRequests,
}

impl Failure {
    /// The stage the failure is at.
    pub fn stage(&self) -> Stage {
        match self {
            Self::NoRecord | Self::RecordElsewhere { .. } => Stage::Dns,
            Self::EdgeUnreachable { .. }
            | Self::CertificateNotCovered
            | Self::NotOnCloudflareYet
            | Self::BodyTooLarge
            | Self::TooManyRequests => Stage::Edge,
            Self::NoConnector | Self::TunnelMismatch => Stage::Tunnel,
            Self::OriginUnreachable { .. } | Self::OriginTimeout | Self::HostRejected { .. } => {
                Stage::Origin
            }
        }
    }

    /// Whether waiting a little may fix it (propagation, connector still connecting).
    pub fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::NotOnCloudflareYet | Self::NoConnector | Self::TunnelMismatch
        )
    }

    /// One sentence for the UI.
    pub fn message(&self) -> Text {
        use crate::text::msg::verify as m;
        match self {
            Self::NoRecord => m::no_record(),
            Self::RecordElsewhere { content } => m::record_elsewhere(content),
            Self::EdgeUnreachable { message } => m::edge_unreachable(message),
            Self::CertificateNotCovered => m::certificate_not_covered(),
            Self::NotOnCloudflareYet => m::not_on_cloudflare_yet(),
            Self::NoConnector => m::no_connector(),
            Self::TunnelMismatch => m::tunnel_mismatch(),
            Self::OriginUnreachable {
                listening: Some(false),
                port: Some(port),
            } => m::origin_not_listening_on(port),
            Self::OriginUnreachable {
                listening: Some(false),
                port: None,
            } => m::origin_not_listening(),
            Self::OriginUnreachable {
                port: Some(port), ..
            } => m::origin_unreachable_on(port),
            Self::OriginUnreachable { .. } => m::origin_unreachable(),
            Self::OriginTimeout => m::origin_timeout(),
            Self::HostRejected { rejection } => m::host_rejected(rejection.server.name()),
            Self::BodyTooLarge => m::body_too_large(),
            Self::TooManyRequests => m::too_many_requests(),
        }
    }
}

/// The result of checking one hostname.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct Verification {
    /// Hostname.
    pub hostname: String,
    /// The origin's HTTP status, when it answered.
    pub status: Option<u16>,
    /// What's wrong, if anything.
    pub failure: Option<Failure>,
    /// The failure, in a sentence for the UI.
    pub message: Option<Text>,
    /// Cloudflare asked for a login (Access) instead of passing the request on, so the
    /// check reached the edge but not the origin behind the login.
    pub protected: bool,
    /// The origin answered with a Server-Sent Events stream (Quick Shares don't carry
    /// them).
    pub event_stream: bool,
}

impl Verification {
    pub(crate) fn new(hostname: String, status: Option<u16>, failure: Option<Failure>) -> Self {
        Self {
            hostname,
            status,
            message: failure.as_ref().map(Failure::message),
            failure,
            protected: false,
            event_stream: false,
        }
    }

    /// Whether the route works end to end.
    pub fn ok(&self) -> bool {
        self.failure.is_none()
    }
}

/// Where the HTTPS probe connects.
#[derive(Debug, Clone, Copy)]
pub enum Edge {
    /// Cloudflare's edge, found by resolving `api.cloudflare.com`.
    Cloudflare,
    /// A plain-HTTP test server standing in for the edge.
    Test(SocketAddr),
}

/// Maps a response from the edge to a failure, if it is one. Cloudflare error pages
/// carry `error code: NNNN` in the body; its plain nginx-style pages end with
/// `<center>cloudflare</center>`.
pub fn classify(status: u16, body: &str) -> Option<Failure> {
    let code = body
        .find("error code: ")
        .and_then(|i| body[i + 12..].split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|c| c.parse::<u32>().ok());
    match (status, code) {
        (_, Some(1033)) => Some(Failure::NoConnector),
        (_, Some(1016)) | (530, _) => Some(Failure::TunnelMismatch),
        (_, Some(1001)) => Some(Failure::NotOnCloudflareYet),
        (502, _) => Some(Failure::OriginUnreachable {
            listening: None,
            port: None,
        }),
        (504, _) => Some(Failure::OriginTimeout),
        // The origin may answer 413 itself; only Cloudflare's page is the edge limit.
        (413, _) if body.contains("<center>cloudflare</center>") => Some(Failure::BodyTooLarge),
        _ => None,
    }
}

/// Quick Shares answer 429 once 200 requests are in flight.
fn is_quick_share(hostname: &str) -> bool {
    hostname.ends_with(".trycloudflare.com")
}

/// Whether a redirect goes to Cloudflare Access's login page.
pub(crate) fn is_access_login(location: &str) -> bool {
    let host = location
        .split_once("://")
        .map_or("", |(_, rest)| rest)
        .split(['/', '?', ':'])
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    host.ends_with(".cloudflareaccess.com") || location.contains("/cdn-cgi/access/login")
}

/// Checks the DNS record in `snapshot` for `hostname`.
pub(crate) fn check_dns(snapshot: &Snapshot, hostname: &str) -> Option<Failure> {
    let target = snapshot.tunnel.as_ref().map(|t| tunnel_target(&t.id));
    let records: Vec<_> = snapshot
        .records_named(hostname)
        .filter(|r| matches!(r.record.kind.as_str(), "A" | "AAAA" | "CNAME"))
        .collect();
    let Some(first) = records.first() else {
        return Some(Failure::NoRecord);
    };
    let ours = records.iter().any(|r| {
        r.record.proxied
            && target
                .as_deref()
                .is_some_and(|t| r.record.content.eq_ignore_ascii_case(t))
    });
    (!ours).then(|| Failure::RecordElsewhere {
        content: first.record.content.clone(),
    })
}

/// Whether something accepts connections on a local origin's port (None if the origin
/// isn't on this Mac).
async fn listening(origin: &RouteOrigin) -> Option<bool> {
    if !origin.is_local() {
        return None;
    }
    let port = origin.port()?;
    let addrs = [
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
    ];
    for addr in addrs {
        if let Ok(Ok(_)) =
            tokio::time::timeout(Duration::from_millis(500), TcpStream::connect(addr)).await
        {
            return Some(true);
        }
    }
    Some(false)
}

fn certificate_error(err: &reqwest::Error) -> bool {
    let text = format!("{err:?}").to_ascii_lowercase();
    [
        "certificate",
        "notvalidforname",
        "handshake",
        "unknownissuer",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

/// Probes `hostname` through the edge. `origin` lets a 502 say whether the local port
/// is listening.
pub(crate) async fn probe(
    edge: Edge,
    hostname: &Hostname,
    origin: Option<&RouteOrigin>,
) -> Verification {
    let name = hostname.as_str().to_owned();
    let result = |status, failure| Verification::new(name.clone(), status, failure);
    let (url, addr) = match edge {
        Edge::Cloudflare => {
            let addr = match lookup_host(("api.cloudflare.com", 443)).await {
                Ok(mut addrs) => addrs.next(),
                Err(err) => {
                    return result(
                        None,
                        Some(Failure::EdgeUnreachable {
                            message: err.to_string(),
                        }),
                    );
                }
            };
            (format!("https://{name}/"), addr)
        }
        Edge::Test(addr) => (format!("http://{name}:{}/", addr.port()), Some(addr)),
    };
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!(
            "Teitunnel/",
            env!("CARGO_PKG_VERSION"),
            " (route check)"
        ))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(PROBE_TIMEOUT);
    if let Some(addr) = addr {
        builder = builder.resolve(&name, addr);
    }
    let client = match builder.build() {
        Ok(client) => client,
        Err(err) => {
            return result(
                None,
                Some(Failure::EdgeUnreachable {
                    message: err.to_string(),
                }),
            );
        }
    };
    let response = match client.get(&url).send().await {
        Ok(response) => response,
        Err(err) if certificate_error(&err) => {
            return result(None, Some(Failure::CertificateNotCovered));
        }
        Err(err) if err.is_timeout() => return result(None, Some(Failure::OriginTimeout)),
        Err(err) => {
            return result(
                None,
                Some(Failure::EdgeUnreachable {
                    message: err.without_url().to_string(),
                }),
            );
        }
    };
    let status = response.status().as_u16();
    let headers = response.headers();
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let protected =
        response.status().is_redirection() && header("location").is_some_and(is_access_login);
    if protected {
        return Verification {
            protected,
            ..result(Some(status), None)
        };
    }
    let event_stream = dev_server::is_event_stream(header("content-type"));
    let next = header("x-powered-by").is_some_and(|v| v.contains("Next.js"));
    // Error pages are small; read a bounded prefix, and of a success only a tiny body
    // (webpack-dev-server 3 refuses with 200). A stream never ends, so never read one.
    let small = response.content_length().is_some_and(|n| n <= 256);
    let body = if !event_stream && (status >= 400 || small) {
        dev_server::read_limited(response).await
    } else {
        String::new()
    };
    if status == 429 && is_quick_share(&name) {
        return result(Some(status), Some(Failure::TooManyRequests));
    }
    match classify(status, &body) {
        Some(Failure::OriginUnreachable { .. }) => {
            let listening = match origin {
                Some(origin) => listening(origin).await,
                None => None,
            };
            let port = origin.and_then(RouteOrigin::port);
            result(
                Some(status),
                Some(Failure::OriginUnreachable { listening, port }),
            )
        }
        Some(failure) => result(None, Some(failure)),
        None => {
            let answer = Answer {
                status,
                body: &body,
                next,
            };
            let rejection = host_rejection(&client, &url, &name, origin, answer).await;
            Verification {
                event_stream,
                ..result(
                    Some(status),
                    rejection.map(|rejection| Failure::HostRejected { rejection }),
                )
            }
        }
    }
}

/// What the origin answered, as far as recognising a dev server needs.
struct Answer<'a> {
    status: u16,
    body: &'a str,
    /// It says it's Next.js (`X-Powered-By`).
    next: bool,
}

/// What a local origin is, from discovery (only asked once an answer looks like a
/// refusal).
async fn local_kind(origin: Option<&RouteOrigin>) -> Option<ServiceKind> {
    let origin = origin.filter(|o| o.is_local())?;
    discovery::kind_on_port(origin.port()?).await
}

/// Recognises a dev server refusing `host` (see [`dev_server::detect`]). For Next.js,
/// whose page loads but whose dev resources are refused, it asks for one of those with
/// the public address as `Origin`, as the browser would.
async fn host_rejection(
    client: &reqwest::Client,
    url: &str,
    host: &str,
    origin: Option<&RouteOrigin>,
    answer: Answer<'_>,
) -> Option<HostRejection> {
    let server = if let Some(server) = dev_server::detect(answer.status, answer.body) {
        server.refine(local_kind(origin).await)
    } else if dev_server::is_plain_django_400(answer.status, answer.body)
        && local_kind(origin).await == Some(ServiceKind::Django)
    {
        DevServer::Django
    } else if answer.next {
        let response = client
            .get(format!(
                "{}{}",
                url.trim_end_matches('/'),
                dev_server::NEXT_PROBE_PATH
            ))
            .header(reqwest::header::ORIGIN, format!("https://{host}"))
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .ok()?;
        let status = response.status().as_u16();
        let body = dev_server::read_limited(response).await;
        if !dev_server::detect_next(status, &body) {
            return None;
        }
        DevServer::Next
    } else {
        return None;
    };
    Some(HostRejection::new(server, host, origin))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_cloudflare_error_pages() {
        assert_eq!(
            classify(530, "<html>… error code: 1033 …</html>"),
            Some(Failure::NoConnector)
        );
        assert_eq!(
            classify(530, "error code: 1016"),
            Some(Failure::TunnelMismatch)
        );
        assert_eq!(classify(530, ""), Some(Failure::TunnelMismatch));
        assert_eq!(
            classify(409, "error code: 1001"),
            Some(Failure::NotOnCloudflareYet)
        );
        assert_eq!(
            classify(502, "Bad Gateway"),
            Some(Failure::OriginUnreachable {
                listening: None,
                port: None
            })
        );
        assert_eq!(
            classify(
                413,
                include_str!("../dev_server/fixtures/cloudflare-413.html")
            ),
            Some(Failure::BodyTooLarge)
        );
        assert_eq!(classify(413, "too big"), None, "the origin's own 413");
        assert_eq!(classify(504, ""), Some(Failure::OriginTimeout));
        for ok in [200, 301, 401, 404, 500] {
            assert_eq!(classify(ok, "hello"), None, "{ok} is the origin answering");
        }
    }

    #[test]
    fn recognizes_the_access_login() {
        assert!(is_access_login(
            "https://myteam.cloudflareaccess.com/cdn-cgi/access/login/app.xyz.com?kid=1"
        ));
        assert!(is_access_login(
            "https://app.xyz.com/cdn-cgi/access/login/app.xyz.com"
        ));
        assert!(!is_access_login("https://app.xyz.com/login"));
        assert!(!is_access_login(
            "https://evil.com/?next=x.cloudflareaccess.com"
        ));
    }

    #[test]
    fn failures_know_their_stage() {
        assert_eq!(Failure::NoRecord.stage(), Stage::Dns);
        assert_eq!(Failure::CertificateNotCovered.stage(), Stage::Edge);
        assert_eq!(Failure::NoConnector.stage(), Stage::Tunnel);
        assert_eq!(Failure::OriginTimeout.stage(), Stage::Origin);
        assert!(Failure::NoConnector.is_transient());
        assert!(!Failure::OriginTimeout.is_transient());
    }

    async fn serve(status: u16, body: &'static str) -> wiremock::MockServer {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(ResponseTemplate::new(status).set_body_string(body))
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn probes_through_the_edge_without_dns() {
        // The hostname doesn't exist anywhere: the probe must not resolve it.
        let host = Hostname::parse("app.teitunnel-test.invalid").unwrap();
        let server = serve(200, "hi").await;
        let ok = probe(Edge::Test(*server.address()), &host, None).await;
        assert!(ok.ok(), "{ok:?}");
        assert_eq!(ok.status, Some(200));
        assert!(!ok.protected);

        let server = {
            use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};
            let server = MockServer::start().await;
            Mock::given(any())
                .respond_with(ResponseTemplate::new(302).insert_header(
                    "location",
                    "https://team.cloudflareaccess.com/cdn-cgi/access/login/app",
                ))
                .mount(&server)
                .await;
            server
        };
        let login = probe(Edge::Test(*server.address()), &host, None).await;
        assert!(login.ok() && login.protected, "{login:?}");

        let server = serve(530, "error code: 1033").await;
        let down = probe(Edge::Test(*server.address()), &host, None).await;
        assert_eq!(down.failure, Some(Failure::NoConnector));

        // 502 with a local origin nobody listens on.
        let origin = RouteOrigin::parse("localhost:9").unwrap();
        let server = serve(502, "").await;
        let bad = probe(Edge::Test(*server.address()), &host, Some(&origin)).await;
        assert_eq!(
            bad.failure,
            Some(Failure::OriginUnreachable {
                listening: Some(false),
                port: Some(9)
            })
        );
        assert_eq!(
            bad.message,
            Some(crate::text::msg::verify::origin_not_listening_on(9))
        );
    }

    async fn serve_with(response: wiremock::ResponseTemplate) -> wiremock::MockServer {
        use wiremock::{Mock, MockServer, matchers::any};
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(response)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn recognises_a_dev_server_refusing_the_address() {
        use wiremock::ResponseTemplate;
        let host = Hostname::parse("quiet-river-lamp.trycloudflare.com").unwrap();
        let origin = RouteOrigin::parse("http://localhost:5173").unwrap();
        let server = serve_with(
            ResponseTemplate::new(403)
                .insert_header("content-type", "text/plain")
                .set_body_string(include_str!("../dev_server/fixtures/vite-6.txt")),
        )
        .await;
        let result = probe(Edge::Test(*server.address()), &host, Some(&origin)).await;
        let Some(Failure::HostRejected { rejection }) = &result.failure else {
            panic!("{result:?}");
        };
        assert_eq!(rejection.server, DevServer::Vite);
        assert_eq!(rejection.host, "quiet-river-lamp.trycloudflare.com");
        assert_eq!(rejection.host_header.as_deref(), Some("localhost:5173"));
        assert_eq!(result.status, Some(403));
        assert_eq!(
            result.failure.as_ref().map(Failure::stage),
            Some(Stage::Origin)
        );

        // webpack-dev-server 3 refuses with a 200.
        let server =
            serve_with(ResponseTemplate::new(200).set_body_string("Invalid Host header")).await;
        let result = probe(Edge::Test(*server.address()), &host, None).await;
        assert!(
            matches!(&result.failure, Some(Failure::HostRejected { rejection }) if rejection.server == DevServer::Webpack),
            "{result:?}"
        );

        // A large ordinary page isn't read at all.
        let page = "<p>hello</p>".repeat(1000);
        let server = serve_with(ResponseTemplate::new(200).set_body_string(page)).await;
        assert!(probe(Edge::Test(*server.address()), &host, None).await.ok());
    }

    #[tokio::test]
    async fn recognises_next_blocking_its_dev_resources() {
        use wiremock::{Mock, MockServer, ResponseTemplate, matchers};
        let host = Hostname::parse("app.teitunnel-test.invalid").unwrap();
        let server = MockServer::start().await;
        Mock::given(matchers::path_regex("^/_next/"))
            .and(matchers::header(
                "origin",
                "https://app.teitunnel-test.invalid",
            ))
            .respond_with(ResponseTemplate::new(403).set_body_string("Unauthorized"))
            .mount(&server)
            .await;
        Mock::given(matchers::path("/"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("x-powered-by", "Next.js")
                    .set_body_string("<html></html>"),
            )
            .mount(&server)
            .await;
        let result = probe(Edge::Test(*server.address()), &host, None).await;
        let Some(Failure::HostRejected { rejection }) = &result.failure else {
            panic!("{result:?}");
        };
        assert_eq!(rejection.server, DevServer::Next);
        assert_eq!(rejection.host_header, None);
        assert_eq!(
            rejection.config_line,
            "allowedDevOrigins: ['app.teitunnel-test.invalid']"
        );
    }

    #[tokio::test]
    async fn explains_edge_limits_and_streams() {
        use wiremock::ResponseTemplate;
        let quick = Hostname::parse("quiet-river-lamp.trycloudflare.com").unwrap();
        let server = serve_with(ResponseTemplate::new(429)).await;
        let busy = probe(Edge::Test(*server.address()), &quick, None).await;
        assert_eq!(busy.failure, Some(Failure::TooManyRequests));
        // On a route, a 429 is the origin's own rate limit.
        let own = Hostname::parse("app.teitunnel-test.invalid").unwrap();
        assert!(probe(Edge::Test(*server.address()), &own, None).await.ok());

        let server = serve_with(
            ResponseTemplate::new(200).set_body_raw("data: hi\n\n", "text/event-stream"),
        )
        .await;
        let stream = probe(Edge::Test(*server.address()), &own, None).await;
        assert!(stream.ok() && stream.event_stream, "{stream:?}");
    }
}
