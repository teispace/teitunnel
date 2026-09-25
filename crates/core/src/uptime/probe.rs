//! One uptime check: a GET through Cloudflare's edge (never through DNS, like the
//! verifier, D-037), timed, and classified the same way as the verifier's probe.

use std::{
    net::SocketAddr,
    time::{Duration, Instant},
};

use tokio::net::lookup_host;

use super::{Cause, CheckOutcome};
use crate::engine::{Edge, Failure, classify, is_access_login};

/// How long one check may take before it counts as a timeout.
pub(crate) const CHECK_TIMEOUT: Duration = Duration::from_secs(8);
/// How long the baseline (is this computer online?) may take.
const BASELINE_TIMEOUT: Duration = Duration::from_secs(5);

/// An edge address to connect to (any edge address serves any proxied hostname).
pub(crate) async fn edge_address(edge: Edge) -> Option<SocketAddr> {
    match edge {
        Edge::Cloudflare => lookup_host(("api.cloudflare.com", 443)).await.ok()?.next(),
        Edge::Test(addr) => Some(addr),
    }
}

/// Whether this computer can reach Cloudflare at all: a TCP connection to the edge. When
/// it can't, failing checks say nothing about the routes.
pub(crate) async fn baseline(edge: Edge) -> bool {
    let attempt = async {
        let addr = edge_address(edge).await?;
        tokio::net::TcpStream::connect(addr).await.ok()
    };
    matches!(
        tokio::time::timeout(BASELINE_TIMEOUT, attempt).await,
        Ok(Some(_))
    )
}

fn cause_of(failure: &Failure) -> Cause {
    match failure {
        Failure::NoRecord | Failure::RecordElsewhere { .. } => Cause::NoRecord,
        Failure::EdgeUnreachable { .. } => Cause::EdgeUnreachable,
        Failure::CertificateNotCovered => Cause::Certificate,
        Failure::NotOnCloudflareYet => Cause::NotOnCloudflare,
        Failure::NoConnector => Cause::NoConnector,
        Failure::TunnelMismatch => Cause::TunnelMismatch,
        Failure::OriginUnreachable { .. } => Cause::OriginUnreachable,
        Failure::OriginTimeout => Cause::OriginTimeout,
        Failure::HostRejected { .. } => Cause::HostRejected,
        Failure::BodyTooLarge | Failure::TooManyRequests => Cause::Limited,
    }
}

fn certificate_error(err: &reqwest::Error) -> bool {
    let text = format!("{err:?}").to_ascii_lowercase();
    ["certificate", "notvalidforname", "unknownissuer"]
        .iter()
        .any(|needle| text.contains(needle))
}

/// Checks `https://{hostname}{path}` through `addr`. Up means the edge passed the request
/// on and something answered below 500 (a redirect to Cloudflare Access's login counts:
/// the route is up behind its login).
pub(crate) async fn check(
    edge: Edge,
    addr: Option<SocketAddr>,
    hostname: &str,
    path: &str,
) -> CheckOutcome {
    let down = |cause, status| CheckOutcome {
        ok: false,
        status,
        latency_ms: None,
        cause: Some(cause),
    };
    let url = match edge {
        Edge::Cloudflare => format!("https://{hostname}{path}"),
        Edge::Test(test) => format!("http://{hostname}:{}{path}", test.port()),
    };
    let mut builder = reqwest::Client::builder()
        .user_agent(concat!(
            "Teitunnel/",
            env!("CARGO_PKG_VERSION"),
            " (uptime check)"
        ))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(CHECK_TIMEOUT);
    if let Some(addr) = addr {
        builder = builder.resolve(hostname, addr);
    }
    let Ok(client) = builder.build() else {
        return down(Cause::EdgeUnreachable, None);
    };
    let started = Instant::now();
    let response = match client
        .get(&url)
        .header("cache-control", "no-cache")
        .send()
        .await
    {
        Ok(response) => response,
        Err(err) if certificate_error(&err) => return down(Cause::Certificate, None),
        Err(err) if err.is_timeout() => return down(Cause::Timeout, None),
        Err(_) => return down(Cause::EdgeUnreachable, None),
    };
    let latency_ms = u32::try_from(started.elapsed().as_millis()).unwrap_or(u32::MAX);
    let status = response.status().as_u16();
    let login = response.status().is_redirection()
        && response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|l| l.to_str().ok())
            .is_some_and(is_access_login);
    if login {
        return CheckOutcome {
            ok: true,
            status: Some(status),
            latency_ms: Some(latency_ms),
            cause: None,
        };
    }
    // Only error pages are read (they're small), to classify them.
    let body = if status >= 500 || status == 404 || status == 409 {
        response.text().await.unwrap_or_default()
    } else {
        String::new()
    };
    match classify(status, &body) {
        Some(failure) => CheckOutcome {
            latency_ms: Some(latency_ms),
            ..down(cause_of(&failure), Some(status))
        },
        None if status >= 500 => CheckOutcome {
            latency_ms: Some(latency_ms),
            ..down(Cause::ServerError, Some(status))
        },
        None => CheckOutcome {
            ok: true,
            status: Some(status),
            latency_ms: Some(latency_ms),
            cause: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

    use super::*;

    async fn serve(template: ResponseTemplate) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(any())
            .respond_with(template)
            .mount(&server)
            .await;
        server
    }

    async fn run(template: ResponseTemplate) -> CheckOutcome {
        let server = serve(template).await;
        let edge = Edge::Test(*server.address());
        let addr = edge_address(edge).await;
        check(edge, addr, "app.teitunnel-test.invalid", "/api/").await
    }

    #[tokio::test]
    async fn classifies_answers() {
        let up = run(ResponseTemplate::new(404)).await;
        assert!(up.ok && up.status == Some(404) && up.latency_ms.is_some());
        let down = run(ResponseTemplate::new(530).set_body_string("error code: 1033")).await;
        assert_eq!((down.ok, down.cause), (false, Some(Cause::NoConnector)));
        let bad = run(ResponseTemplate::new(502)).await;
        assert_eq!(bad.cause, Some(Cause::OriginUnreachable));
        let error = run(ResponseTemplate::new(500)).await;
        assert_eq!(
            (error.cause, error.status),
            (Some(Cause::ServerError), Some(500))
        );
        let login = run(ResponseTemplate::new(302).insert_header(
            "location",
            "https://team.cloudflareaccess.com/cdn-cgi/access/login/app",
        ))
        .await;
        assert!(login.ok);
    }

    #[tokio::test]
    async fn baseline_needs_the_edge() {
        let server = serve(ResponseTemplate::new(200)).await;
        assert!(baseline(Edge::Test(*server.address())).await);
        // Port 9 (discard) is closed on test machines.
        assert!(!baseline(Edge::Test(([127, 0, 0, 1], 9).into())).await);
    }
}
