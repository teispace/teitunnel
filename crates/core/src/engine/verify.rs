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
use crate::domain::{Hostname, RouteOrigin};

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
    },
    /// The origin didn't answer in time (504).
    OriginTimeout,
}

impl Failure {
    /// The stage the failure is at.
    pub fn stage(&self) -> Stage {
        match self {
            Self::NoRecord | Self::RecordElsewhere { .. } => Stage::Dns,
            Self::EdgeUnreachable { .. }
            | Self::CertificateNotCovered
            | Self::NotOnCloudflareYet => Stage::Edge,
            Self::NoConnector | Self::TunnelMismatch => Stage::Tunnel,
            Self::OriginUnreachable { .. } | Self::OriginTimeout => Stage::Origin,
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
    pub fn message(&self) -> String {
        match self {
            Self::NoRecord => "There's no DNS record for this hostname.".to_owned(),
            Self::RecordElsewhere { content } => {
                format!("The DNS record points at {content}, not at this Mac's tunnel.")
            }
            Self::EdgeUnreachable { message } => format!("Couldn't reach Cloudflare: {message}"),
            Self::CertificateNotCovered => "Cloudflare's free certificate covers one level of subdomain (app.example.com), not deeper names like a.b.example.com. Use a single-level name or add an Advanced Certificate.".to_owned(),
            Self::NotOnCloudflareYet => "Cloudflare doesn't serve this hostname yet. New records usually take a few seconds.".to_owned(),
            Self::NoConnector => "Cloudflare can't reach this Mac: the connector isn't connected.".to_owned(),
            Self::TunnelMismatch => "The hostname points at a tunnel that doesn't serve it.".to_owned(),
            Self::OriginUnreachable { listening: Some(false) } => {
                "Nothing is listening on the origin's port. Start your app and test again.".to_owned()
            }
            Self::OriginUnreachable { .. } => {
                "The tunnel works, but the connector couldn't connect to the origin.".to_owned()
            }
            Self::OriginTimeout => "The origin didn't answer in time.".to_owned(),
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
    pub message: Option<String>,
}

impl Verification {
    pub(crate) fn new(hostname: String, status: Option<u16>, failure: Option<Failure>) -> Self {
        Self {
            hostname,
            status,
            message: failure.as_ref().map(Failure::message),
            failure,
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
/// carry `error code: NNNN` in the body.
pub fn classify(status: u16, body: &str) -> Option<Failure> {
    let code = body
        .find("error code: ")
        .and_then(|i| body[i + 12..].split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|c| c.parse::<u32>().ok());
    match (status, code) {
        (_, Some(1033)) => Some(Failure::NoConnector),
        (_, Some(1016)) | (530, _) => Some(Failure::TunnelMismatch),
        (_, Some(1001)) => Some(Failure::NotOnCloudflareYet),
        (502, _) => Some(Failure::OriginUnreachable { listening: None }),
        (504, _) => Some(Failure::OriginTimeout),
        _ => None,
    }
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
    // Error pages are small; don't download a large origin response to classify it.
    let body = if status >= 500 || status == 404 {
        response.text().await.unwrap_or_default()
    } else {
        String::new()
    };
    match classify(status, &body) {
        Some(Failure::OriginUnreachable { .. }) => {
            let listening = match origin {
                Some(origin) => listening(origin).await,
                None => None,
            };
            result(Some(status), Some(Failure::OriginUnreachable { listening }))
        }
        Some(failure) => result(None, Some(failure)),
        None => result(Some(status), None),
    }
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
            Some(Failure::OriginUnreachable { listening: None })
        );
        assert_eq!(classify(504, ""), Some(Failure::OriginTimeout));
        for ok in [200, 301, 401, 404, 500] {
            assert_eq!(classify(ok, "hello"), None, "{ok} is the origin answering");
        }
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
                listening: Some(false)
            })
        );
    }
}
