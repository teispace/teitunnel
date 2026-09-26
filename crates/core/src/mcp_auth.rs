//! OAuth 2.1 for shared MCP servers: Teitunnel is the authorization
//! server in front of an MCP server shared on the person's domain, so claude.ai,
//! ChatGPT and other remote clients can connect without a static key, each connection
//! approved by the person in the app.
//!
//! Follows the MCP authorization spec (2026-07-28): Protected Resource Metadata
//! (RFC 9728) and Authorization Server Metadata (RFC 8414, also as OpenID discovery);
//! clients identified by Client ID Metadata Documents (preferred) or Dynamic Client
//! Registration (RFC 7591, kept for older clients); authorization code with PKCE S256
//! only; tokens bound to the server (RFC 8707 `resource`); `iss` in authorization
//! responses (RFC 9207); short-lived access tokens and rotating refresh tokens with
//! replay detection; revocation (RFC 7009).
//!
//! Every authorization waits for the person to approve it in the app, which shows the
//! client, where it sends the code, and a short code that the browser page shows too:
//! a client's identity says nothing about who's using it (claude.ai's client id is the
//! same for all its users). Only hashes of tokens, codes and client secrets are stored.
//! [`McpAuth::provider`] gives Lens what it needs for one shared server.

mod ask_app;
mod clients;
mod pages;
pub(crate) mod protocol;
mod store;
#[cfg(test)]
mod tests;

use std::{
    collections::HashMap,
    future::Future,
    net::IpAddr,
    pin::Pin,
    sync::{Arc, Mutex, MutexGuard, PoisonError, Weak},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use http::{HeaderMap, HeaderValue, Method, Response, StatusCode, header};
use lens::{HandlerFuture, LensBody, OAuthProvider, ReservedRequest};
use serde::Serialize;
use serde_json::{Value, json};

pub use self::ask_app::AskApp;
use self::{
    clients::{Documents, Fetch},
    protocol::{Client, OAuthError},
    store::{Grant, Refresh},
};
use crate::store::{Store, StoreError};

/// How long an access token lasts.
const ACCESS_TTL: Duration = Duration::from_secs(3600);
/// How long a refresh token lasts unused (each use makes a new one).
const REFRESH_TTL: Duration = Duration::from_secs(30 * 24 * 3600);
/// How long an authorization code can be exchanged.
const CODE_TTL: Duration = Duration::from_secs(60);
/// How long the person has to answer.
const APPROVAL_TIMEOUT: Duration = Duration::from_secs(180);
/// How long an answered request is kept for the browser to collect.
const PENDING_TTL: Duration = Duration::from_secs(300);
/// Requests waiting for an answer per hostname, at most.
const MAX_WAITING: usize = 5;
/// Authorizations and registrations per client address and minute, at most.
const PER_MINUTE: usize = 20;
/// How often the in-memory token index is checked against the database (connections
/// ended in another process stop working within this).
const SYNC_EVERY: Duration = Duration::from_secs(30);

/// What the person is asked when a client wants to connect.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsentRequest {
    /// The shared server's hostname.
    pub host: String,
    /// The client's name, as it says (not verified).
    pub client_name: String,
    /// The host that published the client's metadata, e.g. `claude.ai` (verified by
    /// fetching it), for clients identified that way.
    pub published_by: Option<String>,
    /// Where the authorization goes, e.g. `claude.ai` or `127.0.0.1`.
    pub redirect_host: String,
    /// It goes to an app on the visitor's own computer, which any app could claim.
    pub redirect_loopback: bool,
    /// The code the browser page shows, to compare.
    pub code: String,
    /// The visitor's address.
    pub client_ip: IpAddr,
}

/// Asks the person to approve a connection.
pub trait Approver: Send + Sync + 'static {
    /// `true` to allow it. Unanswered requests are refused after three minutes.
    fn approve(&self, request: ConsentRequest) -> Pin<Box<dyn Future<Output = bool> + Send>>;
}

/// A client connected to a shared MCP server (for the app's list).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct McpConnection {
    /// Id (to disconnect it).
    pub id: String,
    /// The shared server's hostname.
    pub host: String,
    /// The client's name.
    pub client_name: String,
    /// Where its authorization went.
    pub redirect_host: String,
    /// When it was approved (milliseconds since the epoch).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub created_at: u64,
    /// When it last got a token.
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub last_used_at: u64,
}

impl From<Grant> for McpConnection {
    fn from(grant: Grant) -> Self {
        Self {
            id: grant.id,
            host: grant.host,
            client_name: grant.client_name,
            redirect_host: grant.redirect_host,
            created_at: grant.created_at,
            last_used_at: grant.last_used_at,
        }
    }
}

/// The connections to shared MCP servers (one hostname's, or all), newest first.
///
/// # Errors
/// Database errors.
pub async fn connections(
    store: &Store,
    host: Option<&str>,
) -> Result<Vec<McpConnection>, StoreError> {
    Ok(store::grants(store, host)
        .await?
        .into_iter()
        .map(McpConnection::from)
        .collect())
}

/// Ends a connection: its tokens stop working (within 30 seconds in another process).
///
/// # Errors
/// Database errors.
pub async fn disconnect(store: &Store, id: &str) -> Result<bool, StoreError> {
    store::delete_grant(store, id).await
}

fn now_ms() -> u64 {
    crate::domain_shares::now_ms()
}

fn ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

#[derive(Debug, Clone)]
enum Answer {
    Waiting,
    Approved(String),
    Denied,
}

#[derive(Debug, Clone)]
struct Pending {
    host: String,
    redirect_uri: String,
    state: Option<String>,
    answer: Answer,
    shown: String,
    client_name: String,
    created: Instant,
}

#[derive(Debug, Clone)]
struct Code {
    host: String,
    client_id: String,
    client_name: String,
    redirect_uri: String,
    challenge: String,
    expires: Instant,
}

#[derive(Debug, Clone)]
struct Access {
    host: String,
    grant: String,
    expires_at: u64,
}

#[derive(Debug, Default)]
struct State {
    pending: HashMap<String, Pending>,
    /// By hash.
    codes: HashMap<String, Code>,
    /// Exchanged codes (by hash) and the connection they made, to catch replays.
    used_codes: HashMap<String, (Instant, String)>,
    /// Valid access tokens by hash.
    access: HashMap<String, Access>,
    /// Recent authorizations and registrations per client address.
    hits: HashMap<IpAddr, Vec<Instant>>,
}

struct Inner {
    store: Store,
    approver: Arc<dyn Approver>,
    documents: Documents,
    state: Mutex<State>,
}

/// The authorization server, shared by every MCP server this process shares.
#[derive(Clone)]
pub struct McpAuth {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for McpAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("McpAuth").finish_non_exhaustive()
    }
}

impl McpAuth {
    /// Starts the server over `store`, asking `approver` about each connection. Must
    /// run inside Tokio (a task keeps it in sync with the database).
    ///
    /// # Errors
    /// Database errors.
    pub async fn open(store: Store, approver: Arc<dyn Approver>) -> Result<Self, StoreError> {
        Self::with_fetch(store, approver, Arc::new(clients::Web)).await
    }

    pub(crate) async fn with_fetch(
        store: Store,
        approver: Arc<dyn Approver>,
        fetch: Arc<dyn Fetch>,
    ) -> Result<Self, StoreError> {
        let auth = Self {
            inner: Arc::new(Inner {
                store,
                approver,
                documents: Documents::new(fetch),
                state: Mutex::new(State::default()),
            }),
        };
        auth.inner.sync().await?;
        let weak: Weak<Inner> = Arc::downgrade(&auth.inner);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(SYNC_EVERY).await;
                let Some(inner) = weak.upgrade() else { return };
                if let Err(err) = inner.sync().await {
                    tracing::warn!("couldn't check MCP connections: {err}");
                }
            }
        });
        Ok(auth)
    }

    /// What Lens needs to protect the MCP server shared on `host` whose endpoint is
    /// `path` (e.g. `/mcp`), named `name` in its metadata.
    pub fn provider(&self, host: &str, path: &str, name: &str) -> Arc<dyn OAuthProvider> {
        let host = host.trim().to_ascii_lowercase();
        let path = path.trim_end_matches('/');
        Arc::new(Resource {
            inner: Arc::clone(&self.inner),
            resource: format!("{}{path}", protocol::issuer(&host)),
            challenge: HeaderValue::from_str(&format!(
                "Bearer resource_metadata=\"{}{}\"",
                protocol::issuer(&host),
                protocol::RESOURCE_METADATA
            ))
            .unwrap_or_else(|_| HeaderValue::from_static("Bearer")),
            host,
            name: name.to_owned(),
        })
    }

    /// Ends a connection now (see [`disconnect`]).
    ///
    /// # Errors
    /// Database errors.
    pub async fn disconnect(&self, id: &str) -> Result<bool, StoreError> {
        let ended = store::delete_grant(&self.inner.store, id).await?;
        self.inner.lock().access.retain(|_, a| a.grant != id);
        Ok(ended)
    }
}

impl Inner {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Matches the token index to the database (connections ended elsewhere, tokens
    /// issued before a restart) and forgets what expired.
    async fn sync(&self) -> Result<(), StoreError> {
        let now = now_ms();
        store::sweep(&self.store, now).await?;
        let grants = store::grants(&self.store, None).await?;
        let mut state = self.lock();
        state.access = grants
            .into_iter()
            .filter_map(|g| {
                let hash = g.access_hash?;
                let expires_at = g.access_expires_at.filter(|at| *at > now)?;
                Some((
                    hash,
                    Access {
                        host: g.host,
                        grant: g.id,
                        expires_at,
                    },
                ))
            })
            .collect();
        let instant = Instant::now();
        state
            .pending
            .retain(|_, p| instant.duration_since(p.created) < PENDING_TTL);
        state.codes.retain(|_, c| c.expires > instant);
        state
            .used_codes
            .retain(|_, (at, _)| instant.duration_since(*at) < PENDING_TTL * 2);
        state.hits.retain(|_, hits| {
            hits.retain(|at| instant.duration_since(*at) < Duration::from_secs(60));
            !hits.is_empty()
        });
        Ok(())
    }

    /// Counts a request from `ip`; `false` when it made too many this minute.
    fn allow(&self, ip: IpAddr) -> bool {
        let now = Instant::now();
        let mut state = self.lock();
        let hits = state.hits.entry(ip).or_default();
        hits.retain(|at| now.duration_since(*at) < Duration::from_secs(60));
        if hits.len() >= PER_MINUTE {
            return false;
        }
        hits.push(now);
        true
    }

    async fn client(&self, host: &str, client_id: &str) -> Result<Client, OAuthError> {
        if protocol::is_document_url(client_id) {
            return self
                .documents
                .client(client_id)
                .await
                .map_err(|e| OAuthError::new("invalid_client", e.0));
        }
        store::client(&self.store, host, client_id)
            .await
            .map_err(|e| OAuthError::new("server_error", e.to_string()))?
            .ok_or_else(|| {
                OAuthError::new(
                    "invalid_client",
                    "Unknown client: register again (the server may have been reset).",
                )
            })
    }

    /// Checks a client's secret, when it was registered with one.
    fn authenticate(
        client: &Client,
        form: &HashMap<String, String>,
        headers: &HeaderMap,
    ) -> Result<(), OAuthError> {
        let Some(expected) = &client.secret_hash else {
            return Ok(());
        };
        let from_basic = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| {
                v.strip_prefix("Basic ")
                    .or_else(|| v.strip_prefix("basic "))
            })
            .and_then(|b| STANDARD.decode(b.trim()).ok())
            .and_then(|raw| String::from_utf8(raw).ok())
            .and_then(|pair| {
                let (id, secret) = pair.split_once(':')?;
                let decode = |s: &str| protocol::form(&format!("x={s}")).remove("x");
                (decode(id)? == client.id).then(|| decode(secret))?
            });
        let secret = from_basic.or_else(|| form.get("client_secret").cloned());
        if secret.is_some_and(|s| protocol::hash(&s) == *expected) {
            Ok(())
        } else {
            Err(OAuthError::new(
                "invalid_client",
                "The client secret is wrong.",
            ))
        }
    }

    fn issue(&self, grant: &str, host: &str) -> Result<(String, String, u64, u64), OAuthError> {
        let random = |prefix| {
            protocol::random(prefix).map_err(|e| OAuthError::new("server_error", e.to_string()))
        };
        let access = random("ttat_")?;
        let refresh = random("ttrt_")?;
        let now = now_ms();
        let access_expires = now + ms(ACCESS_TTL);
        self.lock().access.insert(
            protocol::hash(&access),
            Access {
                host: host.to_owned(),
                grant: grant.to_owned(),
                expires_at: access_expires,
            },
        );
        Ok((access, refresh, access_expires, now + ms(REFRESH_TTL)))
    }

    async fn end(&self, grant: &str) {
        self.lock().access.retain(|_, a| a.grant != grant);
        if let Err(err) = store::delete_grant(&self.store, grant).await {
            tracing::warn!("couldn't end an MCP connection: {err}");
        }
    }
}

/// One shared MCP server's side of the authorization server (Lens's provider).
struct Resource {
    inner: Arc<Inner>,
    host: String,
    /// The canonical server URI, e.g. `https://mcp.example.com/mcp`.
    resource: String,
    name: String,
    challenge: HeaderValue,
}

impl std::fmt::Debug for Resource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resource")
            .field("resource", &self.resource)
            .finish_non_exhaustive()
    }
}

fn json_response(status: StatusCode, value: &Value) -> Response<LensBody> {
    let mut response = Response::new(lens::full(serde_json::to_vec(value).unwrap_or_default()));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response
}

fn oauth_error(error: &OAuthError) -> Response<LensBody> {
    let status = if error.error == "invalid_client" {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::BAD_REQUEST
    };
    json_response(status, &json!(error))
}

fn redirect(location: &str) -> Response<LensBody> {
    let mut response = Response::new(lens::empty());
    *response.status_mut() = StatusCode::SEE_OTHER;
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(location) {
        headers.insert(header::LOCATION, value);
    }
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response
}

fn preflight() -> Response<LensBody> {
    let mut response = Response::new(lens::empty());
    *response.status_mut() = StatusCode::NO_CONTENT;
    let headers = response.headers_mut();
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    headers.insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("authorization, content-type, mcp-protocol-version"),
    );
    headers.insert(
        header::ACCESS_CONTROL_MAX_AGE,
        HeaderValue::from_static("600"),
    );
    response
}

impl OAuthProvider for Resource {
    fn handles(&self, path: &str) -> bool {
        path == protocol::RESOURCE_METADATA
            || path.starts_with(&format!("{}/", protocol::RESOURCE_METADATA))
            || path == protocol::SERVER_METADATA
            || path == protocol::OPENID_METADATA
            || path.starts_with(&format!("{}/", protocol::PREFIX))
    }

    fn handle(&self, request: ReservedRequest) -> HandlerFuture {
        let this = Resource {
            inner: Arc::clone(&self.inner),
            host: self.host.clone(),
            resource: self.resource.clone(),
            name: self.name.clone(),
            challenge: self.challenge.clone(),
        };
        Box::pin(async move { this.answer(request).await })
    }

    fn valid(&self, token: &str) -> bool {
        let hash = protocol::hash(token);
        let now = now_ms();
        self.inner
            .lock()
            .access
            .get(&hash)
            .is_some_and(|a| a.host == self.host && a.expires_at > now)
    }

    fn challenge(&self) -> HeaderValue {
        self.challenge.clone()
    }
}

impl Resource {
    async fn answer(&self, request: ReservedRequest) -> Response<LensBody> {
        let path = request.uri.path().to_owned();
        if request.method == Method::OPTIONS {
            return preflight();
        }
        let get = request.method == Method::GET || request.method == Method::HEAD;
        let post = request.method == Method::POST;
        match path.as_str() {
            p if get
                && (p == protocol::RESOURCE_METADATA
                    || p.starts_with(&format!("{}/", protocol::RESOURCE_METADATA))) =>
            {
                json_response(
                    StatusCode::OK,
                    &protocol::resource_metadata(&self.host, &self.resource, &self.name),
                )
            }
            protocol::SERVER_METADATA | protocol::OPENID_METADATA if get => {
                json_response(StatusCode::OK, &protocol::server_metadata(&self.host))
            }
            protocol::AUTHORIZE if get => self.authorize(&request).await,
            protocol::WAIT if get => self.wait(&request),
            protocol::TOKEN if post => self.token(&request).await,
            protocol::REGISTER if post => self.register(&request).await,
            protocol::REVOKE if post => self.revoke(&request).await,
            _ => pages::message(
                if get || post {
                    StatusCode::NOT_FOUND
                } else {
                    StatusCode::METHOD_NOT_ALLOWED
                },
                "Not found",
                "There's nothing here.",
            ),
        }
    }

    async fn authorize(&self, request: &ReservedRequest) -> Response<LensBody> {
        let query = protocol::form(request.uri.query().unwrap_or_default());
        if !self.inner.allow(request.client_ip) {
            return pages::message(
                StatusCode::TOO_MANY_REQUESTS,
                "Too many attempts",
                "Please wait a minute and try again.",
            );
        }
        let client_id = query
            .get("client_id")
            .map(String::as_str)
            .unwrap_or_default();
        let client = match self.inner.client(&self.host, client_id).await {
            Ok(client) => client,
            Err(err) => {
                return pages::message(
                    StatusCode::BAD_REQUEST,
                    "This app couldn't be identified",
                    &err.error_description,
                );
            }
        };
        // Until the redirect is known to be the client's, errors are shown, never sent.
        let Some(redirect_uri) = query
            .get("redirect_uri")
            .filter(|uri| protocol::redirect_matches(&client.redirect_uris, uri))
            .cloned()
        else {
            return pages::message(
                StatusCode::BAD_REQUEST,
                "This app's address doesn't match",
                "The app asked to be sent somewhere it didn't register. Nothing was shared.",
            );
        };
        let state = query.get("state").cloned();
        let iss = protocol::issuer(&self.host);
        let fail = |error: &str, description: &str| {
            let mut pairs = vec![("error", error), ("error_description", description)];
            if let Some(state) = &state {
                pairs.push(("state", state));
            }
            pairs.push(("iss", &iss));
            redirect(&protocol::with_query(&redirect_uri, &pairs))
        };
        if query.get("response_type").map(String::as_str) != Some("code") {
            return fail(
                "unsupported_response_type",
                "Only the code response type is supported.",
            );
        }
        let challenge = query.get("code_challenge").cloned().unwrap_or_default();
        if query.get("code_challenge_method").map(String::as_str) != Some("S256")
            || !protocol::challenge_ok(&challenge)
        {
            return fail("invalid_request", "PKCE with S256 is required.");
        }
        if let Some(resource) = query.get("resource")
            && !protocol::resource_matches(&self.host, resource)
        {
            return fail("invalid_target", "This server can only authorize itself.");
        }
        let (Ok(id), Ok(shown)) = (protocol::random("ttreq_"), protocol::short_code()) else {
            return fail("server_error", "No randomness.");
        };
        {
            let mut state_lock = self.inner.lock();
            let waiting = state_lock
                .pending
                .values()
                .filter(|p| p.host == self.host && matches!(p.answer, Answer::Waiting))
                .count();
            if waiting >= MAX_WAITING {
                drop(state_lock);
                return pages::message(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Too many requests are waiting",
                    "Answer the ones waiting in Teitunnel, then try again.",
                );
            }
            state_lock.pending.insert(
                id.clone(),
                Pending {
                    host: self.host.clone(),
                    redirect_uri: redirect_uri.clone(),
                    state: state.clone(),
                    answer: Answer::Waiting,
                    shown: shown.clone(),
                    client_name: client.name.clone(),
                    created: Instant::now(),
                },
            );
        }
        let consent = ConsentRequest {
            host: self.host.clone(),
            client_name: client.name.clone(),
            published_by: client.published_by.clone(),
            redirect_host: protocol::redirect_host(&redirect_uri),
            redirect_loopback: protocol::redirect_is_loopback(&redirect_uri),
            code: shown.clone(),
            client_ip: request.client_ip,
        };
        let name = client.name.clone();
        let inner = Arc::clone(&self.inner);
        let (pending_id, host) = (id.clone(), self.host.clone());
        tokio::spawn(async move {
            let approved = tokio::time::timeout(APPROVAL_TIMEOUT, inner.approver.approve(consent))
                .await
                .unwrap_or(false);
            let answer = if approved {
                match protocol::random("ttcode_") {
                    Ok(code) => {
                        inner.lock().codes.insert(
                            protocol::hash(&code),
                            Code {
                                host,
                                client_id: client.id.clone(),
                                client_name: client.name.clone(),
                                redirect_uri: redirect_uri.clone(),
                                challenge,
                                expires: Instant::now() + CODE_TTL,
                            },
                        );
                        Answer::Approved(code)
                    }
                    Err(_) => Answer::Denied,
                }
            } else {
                Answer::Denied
            };
            if let Some(pending) = inner.lock().pending.get_mut(&pending_id) {
                pending.answer = answer;
            }
        });
        pages::waiting(&self.host, &name, &shown, &id)
    }

    fn wait(&self, request: &ReservedRequest) -> Response<LensBody> {
        let query = protocol::form(request.uri.query().unwrap_or_default());
        let id = query.get("request").cloned().unwrap_or_default();
        let mut state = self.inner.lock();
        let Some(pending) = state
            .pending
            .get(&id)
            .filter(|p| p.host == self.host)
            .cloned()
        else {
            return pages::message(
                StatusCode::NOT_FOUND,
                "This request expired",
                "Go back to your AI app and connect again.",
            );
        };
        let iss = protocol::issuer(&self.host);
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        match &pending.answer {
            Answer::Waiting => {
                drop(state);
                return pages::waiting(&self.host, &pending.client_name, &pending.shown, &id);
            }
            Answer::Approved(code) => pairs.push(("code", code)),
            Answer::Denied => {
                pairs.push(("error", "access_denied"));
                pairs.push(("error_description", "The connection wasn't approved."));
            }
        }
        if let Some(value) = &pending.state {
            pairs.push(("state", value));
        }
        pairs.push(("iss", &iss));
        let location = protocol::with_query(&pending.redirect_uri, &pairs);
        state.pending.remove(&id);
        drop(state);
        redirect(&location)
    }

    async fn token(&self, request: &ReservedRequest) -> Response<LensBody> {
        let form = protocol::form(&String::from_utf8_lossy(&request.body));
        let result = match form.get("grant_type").map(String::as_str) {
            Some("authorization_code") => self.exchange_code(&form, &request.headers).await,
            Some("refresh_token") => self.refresh(&form, &request.headers).await,
            _ => Err(OAuthError::new(
                "unsupported_grant_type",
                "Use authorization_code or refresh_token.",
            )),
        };
        match result {
            Ok(tokens) => json_response(StatusCode::OK, &tokens),
            Err(err) => oauth_error(&err),
        }
    }

    fn check_resource(&self, form: &HashMap<String, String>) -> Result<(), OAuthError> {
        match form.get("resource") {
            Some(resource) if !protocol::resource_matches(&self.host, resource) => {
                Err(OAuthError::new(
                    "invalid_target",
                    "This server can only issue tokens for itself.",
                ))
            }
            _ => Ok(()),
        }
    }

    async fn exchange_code(
        &self,
        form: &HashMap<String, String>,
        headers: &HeaderMap,
    ) -> Result<Value, OAuthError> {
        let code_hash = protocol::hash(form.get("code").map(String::as_str).unwrap_or_default());
        let found = self.inner.lock().codes.remove(&code_hash);
        let Some(code) = found else {
            // A code used twice: whoever has it isn't the client. End what it made.
            let replayed = self.inner.lock().used_codes.remove(&code_hash);
            if let Some((_, grant)) = replayed {
                self.inner.end(&grant).await;
            }
            return Err(OAuthError::new(
                "invalid_grant",
                "The code is invalid, expired or used.",
            ));
        };
        let client_id = form.get("client_id").cloned().unwrap_or_default();
        if code.host != self.host
            || code.expires <= Instant::now()
            || code.client_id != client_id
            || form.get("redirect_uri") != Some(&code.redirect_uri)
        {
            return Err(OAuthError::new(
                "invalid_grant",
                "The code doesn't match this request.",
            ));
        }
        if !protocol::pkce_ok(
            form.get("code_verifier")
                .map(String::as_str)
                .unwrap_or_default(),
            &code.challenge,
        ) {
            return Err(OAuthError::new(
                "invalid_grant",
                "The code verifier is wrong.",
            ));
        }
        self.check_resource(form)?;
        let client = self.inner.client(&self.host, &client_id).await?;
        Inner::authenticate(&client, form, headers)?;
        let grant_id = protocol::random("ttgr_")
            .map_err(|e| OAuthError::new("server_error", e.to_string()))?;
        let (access, refresh, access_expires, refresh_expires) =
            self.inner.issue(&grant_id, &self.host)?;
        let now = now_ms();
        let grant = Grant {
            id: grant_id.clone(),
            host: self.host.clone(),
            client_id,
            client_name: code.client_name,
            redirect_host: protocol::redirect_host(&code.redirect_uri),
            created_at: now,
            last_used_at: now,
            access_hash: Some(protocol::hash(&access)),
            access_expires_at: Some(access_expires),
            refresh_hash: protocol::hash(&refresh),
            refresh_expires_at: refresh_expires,
        };
        if let Err(err) = store::insert_grant(&self.inner.store, &grant).await {
            self.inner.lock().access.retain(|_, a| a.grant != grant_id);
            return Err(OAuthError::new("server_error", err.to_string()));
        }
        self.inner
            .lock()
            .used_codes
            .insert(code_hash, (Instant::now(), grant_id));
        Ok(tokens(&access, &refresh))
    }

    async fn refresh(
        &self,
        form: &HashMap<String, String>,
        headers: &HeaderMap,
    ) -> Result<Value, OAuthError> {
        let presented = protocol::hash(
            form.get("refresh_token")
                .map(String::as_str)
                .unwrap_or_default(),
        );
        let found = store::find_refresh(&self.inner.store, &self.host, &presented)
            .await
            .map_err(|e| OAuthError::new("server_error", e.to_string()))?;
        let grant = match found {
            Refresh::Current(grant) => grant,
            Refresh::Reused(grant) => {
                // An old refresh token again: it leaked. End the connection.
                self.inner.end(&grant).await;
                return Err(OAuthError::new(
                    "invalid_grant",
                    "This refresh token was already used; the connection was ended. Connect again.",
                ));
            }
            Refresh::Unknown => {
                return Err(OAuthError::new(
                    "invalid_grant",
                    "The refresh token is invalid or expired.",
                ));
            }
        };
        let client_id = form.get("client_id").cloned().unwrap_or_default();
        if grant.client_id != client_id || grant.refresh_expires_at <= now_ms() {
            return Err(OAuthError::new(
                "invalid_grant",
                "The refresh token is invalid or expired.",
            ));
        }
        self.check_resource(form)?;
        let client = self.inner.client(&self.host, &client_id).await?;
        Inner::authenticate(&client, form, headers)?;
        let (access, refresh, access_expires, refresh_expires) =
            self.inner.issue(&grant.id, &self.host)?;
        let rotated = store::rotate(
            &self.inner.store,
            &grant.id,
            (&protocol::hash(&access), access_expires),
            (&protocol::hash(&refresh), refresh_expires),
            now_ms(),
        )
        .await
        .map_err(|e| OAuthError::new("server_error", e.to_string()))?;
        let fresh = protocol::hash(&access);
        {
            let mut state = self.inner.lock();
            // Only the newest access token of a connection stays valid.
            state
                .access
                .retain(|hash, a| a.grant != grant.id || *hash == fresh);
            if !rotated {
                state.access.remove(&fresh);
            }
        }
        if !rotated {
            return Err(OAuthError::new(
                "invalid_grant",
                "The connection was ended.",
            ));
        }
        Ok(tokens(&access, &refresh))
    }

    async fn register(&self, request: &ReservedRequest) -> Response<LensBody> {
        if !self.inner.allow(request.client_ip) {
            return oauth_error(&OAuthError::new(
                "slow_down",
                "Too many registrations; wait a minute.",
            ));
        }
        let body: Value = match serde_json::from_slice(&request.body) {
            Ok(body) => body,
            Err(_) => {
                return oauth_error(&OAuthError::new(
                    "invalid_client_metadata",
                    "The registration must be JSON.",
                ));
            }
        };
        let registration = match protocol::registration(&body) {
            Ok(registration) => registration,
            Err(err) => return oauth_error(&OAuthError::new("invalid_client_metadata", err.0)),
        };
        let random = |prefix| protocol::random(prefix);
        let (Ok(id), Ok(secret)) = (random("ttcl_"), random("ttcs_")) else {
            return oauth_error(&OAuthError::new("server_error", "No randomness."));
        };
        let secret = (registration.auth_method != "none").then_some(secret);
        let client = Client {
            id: id.clone(),
            name: registration.name.clone(),
            redirect_uris: registration.redirect_uris.clone(),
            secret_hash: secret.as_deref().map(protocol::hash),
            published_by: None,
        };
        let now = now_ms();
        if let Err(err) = store::save_client(&self.inner.store, &self.host, &client, now).await {
            return oauth_error(&OAuthError::new("server_error", err.to_string()));
        }
        json_response(
            StatusCode::CREATED,
            &protocol::registered(&id, &registration, secret.as_deref(), now / 1000),
        )
    }

    async fn revoke(&self, request: &ReservedRequest) -> Response<LensBody> {
        let form = protocol::form(&String::from_utf8_lossy(&request.body));
        let hash = protocol::hash(form.get("token").map(String::as_str).unwrap_or_default());
        let by_access = self
            .inner
            .lock()
            .access
            .get(&hash)
            .filter(|a| a.host == self.host)
            .map(|a| a.grant.clone());
        let grant = match by_access {
            Some(grant) => Some(grant),
            None => match store::find_refresh(&self.inner.store, &self.host, &hash).await {
                Ok(Refresh::Current(grant)) => Some(grant.id),
                _ => None,
            },
        };
        if let Some(grant) = grant {
            self.inner.end(&grant).await;
        }
        // RFC 7009 §2.2: the same answer whether or not the token was known.
        json_response(StatusCode::OK, &json!({}))
    }
}

fn tokens(access: &str, refresh: &str) -> Value {
    json!({
        "access_token": access,
        "token_type": "Bearer",
        "expires_in": ACCESS_TTL.as_secs(),
        "refresh_token": refresh,
    })
}

/// The bytes of an answer (tests read them).
#[cfg(test)]
pub(crate) async fn body_of(response: Response<LensBody>) -> bytes::Bytes {
    use http_body_util::BodyExt as _;
    response
        .into_body()
        .collect()
        .await
        .map(http_body_util::Collected::to_bytes)
        .unwrap_or_default()
}
