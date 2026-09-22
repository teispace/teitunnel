//! Client for cloudflared's local metrics server (`127.0.0.1:<port>`).
//!
//! Calls are short (500 ms) and never retried here; the supervisor owns the polling
//! policy. The client ignores system proxies, which must never see loopback traffic.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::{Error, Result, metrics::MetricsSnapshot};

const TIMEOUT: Duration = Duration::from_millis(500);

/// The `/ready` response. cloudflared answers 200 when ready and 503 otherwise, both
/// with this body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ready {
    /// HTTP-style status: 200 ready, 503 not yet.
    pub status: u16,
    /// Edge connections currently registered.
    pub ready_connections: u32,
}

#[derive(Deserialize)]
struct QuickTunnel {
    hostname: String,
}

/// Client bound to one connector's metrics server.
#[derive(Debug, Clone)]
pub struct Endpoints {
    client: reqwest::Client,
    base: String,
}

impl Endpoints {
    /// A client for `127.0.0.1:<port>`.
    ///
    /// # Errors
    /// Fails only if the HTTP client can't be constructed.
    pub fn new(port: u16) -> Result<Self> {
        Self::with_base(format!("http://127.0.0.1:{port}"))
    }

    /// A client for an arbitrary base URL (tests).
    ///
    /// # Errors
    /// Fails only if the HTTP client can't be constructed.
    pub fn with_base(base: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(TIMEOUT)
            .connect_timeout(TIMEOUT)
            .build()?;
        Ok(Self { client, base })
    }

    async fn get(&self, path: &str) -> Result<reqwest::Response> {
        Ok(self
            .client
            .get(format!("{}{path}", self.base))
            .send()
            .await?)
    }

    /// Readiness: how many edge connections are up.
    ///
    /// # Errors
    /// Fails if the server is unreachable or answers with an unexpected body.
    pub async fn ready(&self) -> Result<Ready> {
        Ok(self.get("/ready").await?.json().await?)
    }

    /// The Quick Share hostname, once cloudflared has one (`None` before that).
    ///
    /// # Errors
    /// Fails if the server is unreachable or answers with an unexpected body.
    pub async fn quick_tunnel_host(&self) -> Result<Option<String>> {
        let body: QuickTunnel = self
            .get("/quicktunnel")
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(Some(body.hostname).filter(|host| !host.is_empty()))
    }

    /// One scrape of `/metrics`.
    ///
    /// # Errors
    /// Fails if the server is unreachable or returns an error status.
    pub async fn metrics(&self) -> Result<MetricsSnapshot> {
        let text = self
            .get("/metrics")
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(MetricsSnapshot::parse(&text))
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            Self::Timeout("cloudflared metrics server")
        } else {
            Self::Http(err.without_url().to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{method, path},
    };

    use super::*;

    async fn server() -> (MockServer, Endpoints) {
        let server = MockServer::start().await;
        let endpoints = Endpoints::with_base(server.uri()).unwrap();
        (server, endpoints)
    }

    #[tokio::test]
    async fn reads_ready_in_both_states() {
        let (server, endpoints) = server().await;
        Mock::given(method("GET"))
            .and(path("/ready"))
            .respond_with(
                ResponseTemplate::new(503)
                    .set_body_string(include_str!("../fixtures/2026.9.1/ready-connecting.json")),
            )
            .mount(&server)
            .await;
        assert_eq!(
            endpoints.ready().await.unwrap(),
            Ready {
                status: 503,
                ready_connections: 0
            }
        );
    }

    #[tokio::test]
    async fn quick_tunnel_host_is_none_until_assigned() {
        let (server, endpoints) = server().await;
        Mock::given(path("/quicktunnel"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"hostname":""}"#))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(path("/quicktunnel"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(include_str!("../fixtures/2026.9.1/quicktunnel.json")),
            )
            .mount(&server)
            .await;
        assert_eq!(endpoints.quick_tunnel_host().await.unwrap(), None);
        assert_eq!(
            endpoints.quick_tunnel_host().await.unwrap().as_deref(),
            Some("quiet-river-lamp-orbit.trycloudflare.com")
        );
    }

    #[tokio::test]
    async fn scrapes_metrics() {
        let (server, endpoints) = server().await;
        Mock::given(path("/metrics"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(include_str!("../fixtures/2026.9.1/metrics.prom")),
            )
            .mount(&server)
            .await;
        assert_eq!(endpoints.metrics().await.unwrap().ha_connections(), 1);
    }

    #[tokio::test]
    async fn times_out_quickly() {
        let (server, endpoints) = server().await;
        Mock::given(path("/ready"))
            .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
            .mount(&server)
            .await;
        let started = std::time::Instant::now();
        assert!(matches!(endpoints.ready().await, Err(Error::Timeout(_))));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn unreachable_is_an_error_not_a_hang() {
        let endpoints = Endpoints::new(1).unwrap();
        assert!(endpoints.ready().await.is_err());
    }
}
