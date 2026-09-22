//! The HTTP client: authentication, retries, rate limiting and pagination.

use std::{collections::VecDeque, sync::Arc, time::Duration};

use serde::de::DeserializeOwned;
use tokio::{sync::Mutex, time::Instant};

use crate::{
    envelope::{Envelope, ResultInfo},
    error::Result,
    token::ApiToken,
};

/// Production API base.
pub const API_BASE: &str = "https://api.cloudflare.com/client/v4";

const MAX_RETRIES: u32 = 3;
const PER_PAGE: u32 = 50;
/// Safety cap for list endpoints (50 × 200 = 10 000 items).
const MAX_PAGES: u32 = 200;
/// Cloudflare's global limit is 1200 requests per 5 minutes per user; stay under it.
const RATE_LIMIT: usize = 1100;
const RATE_WINDOW: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Retry {
    /// Safe to repeat: retry 429, 5xx and network errors.
    Idempotent,
    /// A create: retry only 429 (the request wasn't processed).
    Once,
}

/// A Cloudflare API client bound to one credential. Cheap to clone.
#[derive(Debug, Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: ApiToken,
    limiter: Arc<Mutex<VecDeque<Instant>>>,
    rate: (usize, Duration),
    backoff: Duration,
}

impl Client {
    /// A client for the production API.
    ///
    /// # Errors
    /// Fails only if the HTTP client can't be constructed.
    pub fn new(token: ApiToken) -> Result<Self> {
        Self::with_base(API_BASE, token)
    }

    /// A client for another base URL (tests).
    ///
    /// # Errors
    /// Fails only if the HTTP client can't be constructed.
    pub fn with_base(base: &str, token: ApiToken) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(concat!("Teitunnel/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_owned(),
            token,
            limiter: Arc::default(),
            rate: (RATE_LIMIT, RATE_WINDOW),
            backoff: Duration::from_millis(500),
        })
    }

    /// Waits if this credential has used its request budget for the window.
    async fn throttle(&self) {
        let (limit, window) = self.rate;
        let mut sent = self.limiter.lock().await;
        let now = Instant::now();
        while sent
            .front()
            .is_some_and(|t| now.duration_since(*t) >= window)
        {
            sent.pop_front();
        }
        if sent.len() >= limit
            && let Some(oldest) = sent.front().copied()
        {
            let wait = window.saturating_sub(now.duration_since(oldest));
            tracing::warn!(?wait, "Cloudflare API rate budget used; waiting");
            tokio::time::sleep(wait).await;
            sent.pop_front();
        }
        sent.push_back(Instant::now());
    }

    /// Sends an idempotent request (GET/PUT/PATCH/DELETE) with retries.
    async fn send(&self, build: impl Fn() -> reqwest::RequestBuilder) -> Result<(u16, Vec<u8>)> {
        self.send_with(Retry::Idempotent, build).await
    }

    /// Sends a request with retries: 429 honours `Retry-After`, other 4xx fail at once.
    /// Idempotent requests also retry 5xx and network errors with exponential backoff;
    /// creates don't, because the first attempt may have succeeded (no duplicates).
    async fn send_with(
        &self,
        retry: Retry,
        build: impl Fn() -> reqwest::RequestBuilder,
    ) -> Result<(u16, Vec<u8>)> {
        let mut attempt = 0;
        loop {
            self.throttle().await;
            let result = build()
                .header("Authorization", self.token.bearer())
                .send()
                .await;
            let retry_after = match result {
                Ok(response) => {
                    let status = response.status().as_u16();
                    let retry_after = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(Duration::from_secs);
                    let retryable = status == 429
                        || (retry == Retry::Idempotent && (500..600).contains(&status));
                    if !retryable || attempt >= MAX_RETRIES {
                        let body = response.bytes().await?.to_vec();
                        return Ok((status, body));
                    }
                    retry_after
                }
                Err(err) if attempt >= MAX_RETRIES || retry == Retry::Once => {
                    return Err(err.into());
                }
                Err(_) => None,
            };
            attempt += 1;
            let delay = retry_after
                .unwrap_or_else(|| self.backoff * 2u32.saturating_pow(attempt - 1))
                .min(Duration::from_secs(60));
            tracing::debug!(attempt, ?delay, "retrying Cloudflare API request");
            tokio::time::sleep(delay).await;
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// `GET path` → `result`.
    ///
    /// # Errors
    /// API errors, network failures or unexpected bodies.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let url = self.url(path);
        let (status, body) = self.send(|| self.http.get(&url)).await?;
        Envelope::decode(status, &body)
    }

    /// `POST path` with a JSON body → `result`. Not retried on 5xx or network errors.
    ///
    /// # Errors
    /// API errors, network failures or unexpected bodies.
    pub async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let url = self.url(path);
        let (status, bytes) = self
            .send_with(Retry::Once, || self.http.post(&url).json(body))
            .await?;
        Envelope::decode(status, &bytes)
    }

    /// `PUT path` with a JSON body → `result`.
    ///
    /// # Errors
    /// API errors, network failures or unexpected bodies.
    pub async fn put<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let url = self.url(path);
        let (status, bytes) = self.send(|| self.http.put(&url).json(body)).await?;
        Envelope::decode(status, &bytes)
    }

    /// `DELETE path`. A 404 counts as success (already gone), so retries are safe.
    ///
    /// # Errors
    /// API errors or network failures.
    pub async fn delete(&self, path: &str) -> Result<()> {
        let url = self.url(path);
        let (status, bytes) = self.send(|| self.http.delete(&url)).await?;
        if status == 404 {
            return Ok(());
        }
        Envelope::<serde_json::Value>::check(status, &bytes)
    }

    /// `PATCH path` with a JSON body → `result`.
    ///
    /// # Errors
    /// API errors, network failures or unexpected bodies.
    pub async fn patch<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T> {
        let url = self.url(path);
        let (status, bytes) = self.send(|| self.http.patch(&url).json(body)).await?;
        Envelope::decode(status, &bytes)
    }

    async fn get_page<T: DeserializeOwned>(
        &self,
        path: &str,
        page: u32,
    ) -> Result<(Vec<T>, Option<ResultInfo>)> {
        let separator = if path.contains('?') { '&' } else { '?' };
        let url = self.url(&format!("{path}{separator}page={page}&per_page={PER_PAGE}"));
        let (status, body) = self.send(|| self.http.get(&url)).await?;
        Envelope::decode_page(status, &body)
    }

    /// `GET path` over every page.
    ///
    /// # Errors
    /// API errors, network failures or unexpected bodies.
    pub async fn get_all<T: DeserializeOwned>(&self, path: &str) -> Result<Vec<T>> {
        let mut items = Vec::new();
        for page in 1..=MAX_PAGES {
            let (batch, info) = self.get_page::<T>(path, page).await?;
            let count = batch.len();
            items.extend(batch);
            let more = match info.and_then(|i| i.total_pages) {
                Some(total) => page < total,
                None => u32::try_from(count).unwrap_or(u32::MAX) >= PER_PAGE,
            };
            if !more {
                break;
            }
        }
        Ok(items)
    }

    #[cfg(test)]
    pub(crate) fn with_backoff(mut self, backoff: Duration) -> Self {
        self.backoff = backoff;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_rate(mut self, limit: usize, window: Duration) -> Self {
        self.rate = (limit, window);
        self
    }
}

#[cfg(test)]
mod tests {
    use wiremock::{
        Mock, MockServer, ResponseTemplate,
        matchers::{header, method, path, query_param},
    };

    use super::*;

    #[allow(clippy::needless_pass_by_value)]
    fn ok(result: serde_json::Value) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "success": true, "errors": [], "messages": [], "result": result
        }))
    }

    async fn client() -> (MockServer, Client) {
        let server = MockServer::start().await;
        let client = Client::with_base(&server.uri(), ApiToken::new("secret-token"))
            .unwrap()
            .with_backoff(Duration::from_millis(10));
        (server, client)
    }

    #[tokio::test]
    async fn sends_bearer_auth_and_user_agent() {
        let (server, client) = client().await;
        Mock::given(method("GET"))
            .and(path("/user"))
            .and(header("authorization", "Bearer secret-token"))
            .respond_with(ok(serde_json::json!({"id": "u1"})))
            .expect(1)
            .mount(&server)
            .await;
        let user: serde_json::Value = client.get("/user").await.unwrap();
        assert_eq!(user["id"], "u1");
    }

    #[tokio::test]
    async fn retries_429_with_retry_after_then_succeeds() {
        let (server, client) = client().await;
        Mock::given(path("/thing"))
            .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
            .up_to_n_times(2)
            .mount(&server)
            .await;
        Mock::given(path("/thing"))
            .respond_with(ok(serde_json::json!(1)))
            .mount(&server)
            .await;
        let value: u8 = client.get("/thing").await.unwrap();
        assert_eq!(value, 1);
    }

    #[tokio::test]
    async fn retries_5xx_but_gives_up_after_three_retries() {
        let (server, client) = client().await;
        Mock::given(path("/down"))
            .respond_with(ResponseTemplate::new(503))
            .expect(4)
            .mount(&server)
            .await;
        let err = client.get::<u8>("/down").await.unwrap_err();
        assert_eq!(err.status(), Some(503));
    }

    #[tokio::test]
    async fn never_retries_creates_on_server_errors() {
        let (server, client) = client().await;
        Mock::given(method("POST"))
            .and(path("/things"))
            .respond_with(ResponseTemplate::new(502))
            .expect(1)
            .mount(&server)
            .await;
        let err = client
            .post::<u8>("/things", &serde_json::json!({}))
            .await
            .unwrap_err();
        assert_eq!(err.status(), Some(502));
    }

    #[tokio::test]
    async fn deleting_something_already_gone_succeeds() {
        let (server, client) = client().await;
        Mock::given(method("DELETE"))
            .and(path("/gone"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        client.delete("/gone").await.unwrap();
    }

    #[tokio::test]
    async fn never_retries_client_errors() {
        let (server, client) = client().await;
        Mock::given(path("/forbidden"))
            .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
                "success": false, "errors": [{"code": 10000, "message": "Authentication error"}], "messages": [], "result": null
            })))
            .expect(1)
            .mount(&server)
            .await;
        let err = client.get::<u8>("/forbidden").await.unwrap_err();
        assert!(err.is_auth());
        assert_eq!(err.codes(), [10000]);
    }

    #[tokio::test]
    async fn collects_every_page() {
        let (server, client) = client().await;
        for page in 1..=3u32 {
            let items: Vec<u32> = if page < 3 {
                (0..PER_PAGE).collect()
            } else {
                vec![7]
            };
            Mock::given(path("/zones"))
                .and(query_param("page", page.to_string()))
                .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                    "success": true, "errors": [], "messages": [], "result": items,
                    "result_info": {"page": page, "per_page": PER_PAGE, "total_pages": 3}
                })))
                .expect(1)
                .mount(&server)
                .await;
        }
        let all: Vec<u32> = client.get_all("/zones").await.unwrap();
        assert_eq!(all.len(), usize::try_from(PER_PAGE * 2 + 1).unwrap());
    }

    #[tokio::test]
    async fn throttles_to_the_request_budget() {
        let (server, client) = client().await;
        let client = client.with_rate(2, Duration::from_millis(300));
        Mock::given(path("/x"))
            .respond_with(ok(serde_json::json!(0)))
            .mount(&server)
            .await;
        let started = std::time::Instant::now();
        for _ in 0..3 {
            let _: u8 = client.get("/x").await.unwrap();
        }
        assert!(
            started.elapsed() >= Duration::from_millis(250),
            "{:?}",
            started.elapsed()
        );
    }

    #[tokio::test]
    async fn debug_never_prints_the_token() {
        let (_server, client) = client().await;
        assert!(!format!("{client:?}").contains("secret-token"));
    }
}
