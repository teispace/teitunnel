//! The comments API a live share answers under `/__teitunnel/comments/` (through Lens,
//! after the tap's gates admitted the visitor), and the overlay script.
//!
//! Same-origin only: writes must be `application/json` (a cross-site form can't send
//! that without a preflight, which is refused) and, when the browser says where the
//! request came from, from this site. Visitors are rate-limited per address; text is
//! checked and stored as typed; answers never include email addresses.

use std::{
    collections::{HashMap, VecDeque},
    net::IpAddr,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Response, StatusCode, header};
use lens::{HandlerFuture, LensBody, ReservedHandler, ReservedRequest};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Anchor, Author, BASE_PATH, Comments, CommentsError, OVERLAY_JS, Subject};
use crate::text::UserText;

/// Writes one visitor may make per minute.
pub const WRITES_PER_MINUTE: usize = 10;
/// Writes one visitor may make per hour.
pub const WRITES_PER_HOUR: usize = 60;
/// Largest API request body.
const MAX_REQUEST: usize = 16 * 1024;
/// Visitors remembered by the limiter.
const MAX_VISITORS: usize = 10_000;

/// Per-visitor write limits (sliding windows).
#[derive(Debug, Default)]
pub struct Limiter {
    seen: Mutex<HashMap<IpAddr, VecDeque<Instant>>>,
}

impl Limiter {
    /// Records a write from `ip` at `now` if it's allowed.
    pub fn allow(&self, ip: IpAddr, now: Instant) -> bool {
        let mut seen = self.seen.lock().unwrap_or_else(PoisonError::into_inner);
        if seen.len() >= MAX_VISITORS && !seen.contains_key(&ip) {
            seen.retain(|_, times| {
                times
                    .back()
                    .is_some_and(|t| now.duration_since(*t) < Duration::from_secs(3600))
            });
            if seen.len() >= MAX_VISITORS {
                return false;
            }
        }
        let times = seen.entry(ip).or_default();
        while times
            .front()
            .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(3600))
        {
            times.pop_front();
        }
        let last_minute = times
            .iter()
            .filter(|t| now.duration_since(**t) < Duration::from_secs(60))
            .count();
        if last_minute >= WRITES_PER_MINUTE || times.len() >= WRITES_PER_HOUR {
            return false;
        }
        times.push_back(now);
        true
    }
}

#[derive(Debug)]
struct Inner {
    comments: Comments,
    subject: Subject,
    trust_identity: bool,
}

/// Answers `/__teitunnel/comments/…` for one share or route.
#[derive(Debug, Clone)]
pub struct CommentsHandler {
    inner: Arc<Inner>,
}

impl CommentsHandler {
    /// A handler keeping `subject`'s comments in `comments`. `trust_identity`: the
    /// hostname is behind Teitunnel's Cloudflare Access login, so the
    /// `Cf-Access-Authenticated-User-Email` header comes from Access (never trust it
    /// otherwise: a visitor could send it).
    pub fn new(comments: Comments, subject: Subject, trust_identity: bool) -> Self {
        Self {
            inner: Arc::new(Inner {
                comments,
                subject,
                trust_identity,
            }),
        }
    }

    /// The subject.
    pub fn subject(&self) -> &Subject {
        &self.inner.subject
    }
}

impl ReservedHandler for CommentsHandler {
    fn handle(&self, request: ReservedRequest) -> HandlerFuture {
        let this = self.clone();
        Box::pin(async move { this.answer(request).await })
    }
}

fn respond(status: StatusCode, content_type: &'static str, body: Bytes) -> Response<LensBody> {
    let mut response = Response::new(lens::full(body));
    *response.status_mut() = status;
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-robots-tag", HeaderValue::from_static("noindex"));
    response
}

fn json_response(status: StatusCode, value: &Value) -> Response<LensBody> {
    respond(
        status,
        "application/json; charset=utf-8",
        Bytes::from(value.to_string()),
    )
}

fn error(status: StatusCode, err: &CommentsError) -> Response<LensBody> {
    json_response(status, &json!({ "error": err.text().english() }))
}

fn not_found() -> Response<LensBody> {
    json_response(StatusCode::NOT_FOUND, &json!({ "error": "Not found" }))
}

fn status_of(err: &CommentsError) -> StatusCode {
    match err {
        CommentsError::InvalidBody
        | CommentsError::InvalidName
        | CommentsError::InvalidPath
        | CommentsError::InvalidAnchor => StatusCode::BAD_REQUEST,
        CommentsError::TooMany => StatusCode::CONFLICT,
        CommentsError::NotFound => StatusCode::NOT_FOUND,
        CommentsError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
        CommentsError::Disabled | CommentsError::NoDatabase | CommentsError::NoAccount => {
            StatusCode::FORBIDDEN
        }
        CommentsError::Store(_) | CommentsError::Api(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
}

/// Whether a write comes from a page on this site as JSON (see the module docs).
pub(crate) fn same_origin_json(headers: &HeaderMap) -> bool {
    let header = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    let json = header("content-type").is_some_and(|t| {
        t.split(';')
            .next()
            .is_some_and(|m| m.trim().eq_ignore_ascii_case("application/json"))
    });
    if !json {
        return false;
    }
    if let Some(site) = header("sec-fetch-site") {
        return site.eq_ignore_ascii_case("same-origin");
    }
    match (header("origin"), header("host")) {
        (Some(origin), Some(host)) => origin
            .split_once("://")
            .is_some_and(|(_, rest)| rest.eq_ignore_ascii_case(host)),
        // No Origin: not a browser's cross-site request.
        (None, _) => true,
        (Some(_), None) => false,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct NewThread {
    path: String,
    #[serde(default)]
    anchor: Option<Anchor>,
    body: String,
    #[serde(default)]
    author: Option<String>,
}

#[derive(Deserialize)]
struct NewReply {
    body: String,
    #[serde(default)]
    author: Option<String>,
}

#[derive(Deserialize)]
struct Resolution {
    resolved: bool,
    #[serde(default)]
    author: Option<String>,
}

/// The query parameter `name` of a path-and-query.
fn query_param(uri: &http::Uri, name: &str) -> Option<String> {
    let url = reqwest::Url::parse(&format!("http://local{uri}")).ok()?;
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

impl CommentsHandler {
    fn identity(&self, headers: &HeaderMap) -> Option<String> {
        if !self.inner.trust_identity {
            return None;
        }
        headers
            .get("cf-access-authenticated-user-email")
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|e| e.contains('@') && e.len() <= 254)
            .map(str::to_owned)
    }

    fn author(&self, headers: &HeaderMap, typed: Option<&str>) -> Result<Author, CommentsError> {
        match self.identity(headers) {
            Some(email) => Ok(Author::verified(&email, typed)),
            None => Author::reviewer(typed.unwrap_or_default()),
        }
    }

    async fn answer(&self, request: ReservedRequest) -> Response<LensBody> {
        let path = request.uri.path();
        let Some(rest) = path.strip_prefix(BASE_PATH) else {
            return not_found();
        };
        match (&request.method, rest) {
            (&Method::GET | &Method::HEAD, "overlay.js") => {
                let mut response = respond(
                    StatusCode::OK,
                    "text/javascript; charset=utf-8",
                    Bytes::from_static(OVERLAY_JS.as_bytes()),
                );
                response
                    .headers_mut()
                    .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
                response
            }
            (&Method::GET, "api/threads") => self.list(&request).await,
            (&Method::POST, _) if rest.starts_with("api/") => {
                if !same_origin_json(&request.headers) {
                    return json_response(
                        StatusCode::FORBIDDEN,
                        &json!({ "error": "Comments can only be posted from this site." }),
                    );
                }
                if request.body.len() > MAX_REQUEST {
                    return error(StatusCode::PAYLOAD_TOO_LARGE, &CommentsError::InvalidBody);
                }
                if !self
                    .inner
                    .comments
                    .limiter()
                    .allow(request.client_ip, Instant::now())
                {
                    return error(StatusCode::TOO_MANY_REQUESTS, &CommentsError::RateLimited);
                }
                let result = match rest {
                    "api/threads" => self.start(&request).await,
                    _ => match rest
                        .strip_prefix("api/threads/")
                        .and_then(|r| r.split_once('/'))
                    {
                        Some((id, "replies")) => self.reply(&request, id).await,
                        Some((id, "resolve")) => self.resolve(&request, id).await,
                        _ => return not_found(),
                    },
                };
                match result {
                    Ok(thread) => json_response(
                        StatusCode::OK,
                        &serde_json::to_value(thread.public()).unwrap_or_default(),
                    ),
                    Err(err) => error(status_of(&err), &err),
                }
            }
            _ => not_found(),
        }
    }

    async fn list(&self, request: &ReservedRequest) -> Response<LensBody> {
        let path = match query_param(&request.uri, "path") {
            Some(path) => match super::clean_path(&path) {
                Ok(path) => Some(path),
                Err(err) => return error(status_of(&err), &err),
            },
            None => None,
        };
        let identity = self.identity(&request.headers);
        match self
            .inner
            .comments
            .local_threads(&self.inner.subject.key, path.as_deref())
            .await
        {
            Ok(threads) => {
                let threads: Vec<_> = threads.into_iter().map(super::Thread::public).collect();
                json_response(
                    StatusCode::OK,
                    &json!({
                        "threads": threads,
                        "me": { "verified": identity.is_some(), "name": identity },
                    }),
                )
            }
            Err(err) => error(status_of(&err), &err),
        }
    }

    async fn start(&self, request: &ReservedRequest) -> Result<super::Thread, CommentsError> {
        let input: NewThread =
            serde_json::from_slice(&request.body).map_err(|_| CommentsError::InvalidBody)?;
        let author = self.author(&request.headers, input.author.as_deref())?;
        self.inner
            .comments
            .local_start(
                &self.inner.subject,
                &input.path,
                input.anchor.as_ref(),
                &input.body,
                &author,
            )
            .await
    }

    async fn reply(
        &self,
        request: &ReservedRequest,
        id: &str,
    ) -> Result<super::Thread, CommentsError> {
        let input: NewReply =
            serde_json::from_slice(&request.body).map_err(|_| CommentsError::InvalidBody)?;
        let author = self.author(&request.headers, input.author.as_deref())?;
        self.inner
            .comments
            .local_reply(&self.inner.subject, id, &input.body, &author)
            .await
    }

    async fn resolve(
        &self,
        request: &ReservedRequest,
        id: &str,
    ) -> Result<super::Thread, CommentsError> {
        let input: Resolution =
            serde_json::from_slice(&request.body).map_err(|_| CommentsError::InvalidBody)?;
        let by = self
            .identity(&request.headers)
            .or_else(|| input.author.and_then(|a| super::clean_name(&a).ok()))
            .unwrap_or_default();
        self.inner
            .comments
            .local_resolve(&self.inner.subject, id, input.resolved, &by)
            .await
    }
}
