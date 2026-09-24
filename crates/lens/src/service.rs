//! The request pipeline: route → record → gates → paused → reserved paths → sign-in
//! → CORS preflight → stubs → upstream (folder or origin, streaming, upgrades) →
//! response rules → capture → injection.

use std::{net::IpAddr, sync::Arc, time::Instant};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Request, Response, StatusCode, header};
use http_body::Body as _;
use http_body_util::BodyExt;
use hyper::body::Incoming;

use crate::{
    BodyRecord, ClientInfo, Exchange, ExchangeError, ExchangeId, ExchangeKind, ExchangeState,
    GateOutcome, LensBody, RequestRecord, Responder, Timings,
    body::{BoxError, PrefixedBody, TeeBody, empty, read_limited, read_prefix},
    capture::ErrorKind,
    forward,
    gate::{self, LOGIN_PATH},
    inject::{self, MAX_RESERVED_BODY, RESERVED_PREFIX, ReservedRequest},
    keepalive::KeepAliveBody,
    lens::Shared,
    listener::{ConnInfo, ListenerState, normalize_host},
    pages,
    recorder::{Recorder, Side},
    rules,
    sim::{self, FaultAction, ThrottledBody},
    stream::PreviewLimits,
    stub::{self, StubMode},
    tap::{Active, TapRuntime},
    tunnel,
    upstream::{ActiveUpstream, OriginClient},
    util::{now_unix_ms, now_unix_secs},
};

/// Largest sign-in form body.
const MAX_LOGIN_BODY: usize = 8 * 1024;

/// Handles one request from a listener.
pub(crate) async fn handle(
    shared: Arc<Shared>,
    listener: Arc<ListenerState>,
    conn: ConnInfo,
    request: Request<Incoming>,
) -> Result<Response<LensBody>, FaultReset> {
    let t0 = Instant::now();
    let host = request_host(&request, &conn);
    let routes = listener.routes();
    let normalized = normalize_host(&host);
    let Some(tap) = routes.resolve(&normalized).and_then(|id| shared.tap(id)) else {
        return Ok(pages::text(
            StatusCode::MISDIRECTED_REQUEST,
            format!("No site is set up for {normalized} here.\n"),
        ));
    };
    let active = tap.active();
    let listener_scheme = if listener.secure { "https" } else { "http" };
    let cf_ip = request
        .headers()
        .get("cf-connecting-ip")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<IpAddr>().ok());
    let client_ip = if active.config.trust_cf_connecting_ip {
        cf_ip.unwrap_or_else(|| conn.peer.ip())
    } else {
        conn.peer.ip()
    };
    let exchange = skeleton(
        &tap,
        &active,
        &request,
        &conn,
        host.clone(),
        cf_ip,
        listener_scheme,
    );
    let recorder = Recorder::start(
        Arc::clone(&shared.hub),
        Arc::clone(&tap.metrics),
        active.config.capture.enabled,
        t0,
        exchange,
    );
    let pipeline = Pipeline {
        shared,
        listener,
        tap,
        active,
        recorder,
        client_ip,
        host,
        scheme: listener_scheme,
    };
    let response = pipeline.run(request).await;
    if response.extensions().get::<ResetConnection>().is_some() {
        // A fault rule asked for a reset: failing the service makes hyper drop the
        // connection without writing a response.
        return Err(FaultReset);
    }
    Ok(response)
}

/// Marks a response that must not be sent: the connection is reset instead.
#[derive(Debug, Clone, Copy)]
struct ResetConnection;

/// The service error that makes hyper close the connection without a response.
#[derive(Debug, Clone, Copy, thiserror::Error)]
#[error("connection reset by a fault rule")]
pub(crate) struct FaultReset;

/// The visitor's host: `Host`, else the URI authority (HTTP/2), else the TLS name.
fn request_host(request: &Request<Incoming>, conn: &ConnInfo) -> String {
    request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned)
        .or_else(|| request.uri().authority().map(|a| a.as_str().to_owned()))
        .or_else(|| conn.server_name.clone())
        .unwrap_or_default()
}

fn skeleton(
    tap: &TapRuntime,
    active: &Active,
    request: &Request<Incoming>,
    conn: &ConnInfo,
    host: String,
    cf_ip: Option<IpAddr>,
    listener_scheme: &str,
) -> Exchange {
    let headers = request.headers();
    let text = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let scheme = match text("x-forwarded-proto").as_deref() {
        Some(proto) if proto.eq_ignore_ascii_case("https") => "https",
        Some(proto) if proto.eq_ignore_ascii_case("http") => "http",
        _ => listener_scheme,
    };
    let capture = active.config.capture.enabled;
    let body = if request.body().is_end_stream() {
        BodyRecord::empty()
    } else {
        BodyRecord::default()
    };
    Exchange {
        id: ExchangeId::new(),
        seq: tap.next_seq(),
        tap: tap.id.clone(),
        kind: ExchangeKind::Http,
        state: ExchangeState::Pending,
        started_at_ms: now_unix_ms(),
        timings: Timings::default(),
        client: ClientInfo {
            ip: cf_ip.unwrap_or_else(|| conn.peer.ip()),
            peer: conn.peer,
            cf_ray: text("cf-ray"),
            country: text("cf-ipcountry"),
        },
        request: RequestRecord {
            method: request.method().clone(),
            uri: request.uri().clone(),
            scheme: scheme.to_owned(),
            host,
            version: request.version(),
            headers: if capture {
                headers.clone()
            } else {
                HeaderMap::new()
            },
            body,
        },
        response: None,
        responder: Responder::Lens,
        error: None,
        stream: None,
        replay_of: None,
        fault: None,
    }
}

struct Pipeline {
    shared: Arc<Shared>,
    listener: Arc<ListenerState>,
    tap: Arc<TapRuntime>,
    active: Arc<Active>,
    recorder: Recorder,
    client_ip: IpAddr,
    host: String,
    scheme: &'static str,
}

impl Pipeline {
    fn limits(&self) -> PreviewLimits {
        PreviewLimits {
            count: self.active.config.capture.stream_previews,
            bytes: self.active.config.capture.preview_bytes,
            frames: self.active.config.capture.ws_frames,
            frame_bytes: self.active.config.capture.frame_preview_bytes,
        }
    }

    fn cap(&self) -> usize {
        self.active.config.capture.max_body_bytes
    }

    async fn run(self, mut request: Request<Incoming>) -> Response<LensBody> {
        let config = &self.active.config;
        let gates = &config.gates;
        let path = request.uri().path().to_owned();

        let target_len = request
            .uri()
            .path_and_query()
            .map_or(0, |pq| pq.as_str().len());
        if target_len > self.listener.limits.max_uri_bytes {
            return self.local(
                pages::text(StatusCode::URI_TOO_LONG, "URI too long\n".into()),
                Responder::Lens,
            );
        }
        if request.method() == Method::CONNECT {
            return self.local(
                pages::text(
                    StatusCode::METHOD_NOT_ALLOWED,
                    "CONNECT isn't supported\n".into(),
                ),
                Responder::Lens,
            );
        }

        // Network rules apply to every path, including bypassed ones.
        if let Some(outcome) = gates.check_ip(self.client_ip) {
            return self.blocked(
                outcome,
                "Access denied",
                "Your network isn't allowed to open this site.",
            );
        }
        if let Some(agent) = request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok())
            && gates.agent_blocked(agent)
        {
            return self.blocked(
                GateOutcome::UserAgentBlocked,
                "Access denied",
                "Automated clients aren't allowed on this site.",
            );
        }

        if let Some(page) = &config.paused {
            return self.local(pages::paused(page), Responder::Paused);
        }

        if path == LOGIN_PATH {
            return if request.method() == Method::POST && gates.password.is_some() {
                self.login(request).await
            } else {
                self.local(
                    pages::text(StatusCode::NOT_FOUND, "Not found\n".into()),
                    Responder::Lens,
                )
            };
        }

        let mut strip_authorization = false;
        if gates.requires_sign_in() && !gates.bypassed(&path) {
            match self.sign_in(&request) {
                SignIn::Admitted { basic } => strip_authorization = basic,
                SignIn::Respond(response, outcome) => {
                    return self.local(response, Responder::Gate { reason: outcome });
                }
            }
        }

        if path.starts_with(RESERVED_PREFIX) {
            return self.reserved(request).await;
        }

        if config.headers.cors && rules::is_preflight(request.method(), request.headers()) {
            return self.local(
                rules::preflight_response(request.headers()),
                Responder::Lens,
            );
        }

        if let Some(latency) = config.network.latency {
            tokio::time::sleep(latency.sample(self.shared.random.next_f64())).await;
        }
        if let Some(fault) = sim::pick_fault(
            &config.faults,
            request.method(),
            &path,
            &*self.shared.random,
        ) {
            let rule = fault.rule;
            self.recorder.set_fault(fault.clone());
            match fault.action {
                FaultAction::Status {
                    status,
                    retry_after_secs,
                } => {
                    let status =
                        StatusCode::from_u16(status).unwrap_or(StatusCode::SERVICE_UNAVAILABLE);
                    let mut response = pages::text(
                        status,
                        format!(
                            "{} {} (simulated by Teitunnel)\n",
                            status.as_u16(),
                            status.canonical_reason().unwrap_or_default()
                        ),
                    );
                    if let Some(seconds) = retry_after_secs {
                        response
                            .headers_mut()
                            .insert(header::RETRY_AFTER, HeaderValue::from(seconds));
                    }
                    return self.local(response, Responder::Fault { rule });
                }
                FaultAction::Reset => {
                    self.recorder.fail(
                        ErrorKind::ConnectionReset,
                        format!("the connection was reset by fault rule {rule}"),
                    );
                    let mut response = Response::new(empty());
                    response.extensions_mut().insert(ResetConnection);
                    return response;
                }
                FaultAction::Delay { ms } => {
                    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                }
                FaultAction::Timeout { after_ms } => {
                    tokio::time::sleep(std::time::Duration::from_millis(after_ms)).await;
                    self.recorder.note_error(ExchangeError {
                        kind: ErrorKind::Timeout,
                        message: format!("a simulated timeout (fault rule {rule})"),
                    });
                    return self.local(
                        error_page(ErrorKind::Timeout, request.headers()),
                        Responder::Fault { rule },
                    );
                }
            }
        }

        if let Some((index, rule)) =
            stub::find(&config.stubs, StubMode::Always, request.method(), &path)
        {
            let response = rule.response();
            let (_, body) = request.into_parts();
            self.capture_body_only(body).await;
            return self
                .finish(
                    response,
                    Responder::Stub {
                        rule: index,
                        fallback: false,
                    },
                    &HeaderMap::new(),
                    false,
                )
                .await;
        }

        let session_cookie = gates.requires_sign_in().then(|| gates.cookie_name());
        match self.active.upstream.clone() {
            ActiveUpstream::Folder(folder) => {
                let response = folder
                    .serve(request.method(), request.uri(), request.headers())
                    .await;
                let (parts, body) = request.into_parts();
                drop(body);
                self.finish(response, Responder::Folder, &parts.headers, false)
                    .await
            }
            ActiveUpstream::Origin(client) => {
                let upgrade = forward::is_upgrade(request.version(), request.headers());
                let client_upgrade = upgrade.then(|| hyper::upgrade::on(&mut request));
                self.forward(
                    client,
                    request,
                    client_upgrade,
                    session_cookie,
                    strip_authorization,
                )
                .await
            }
        }
    }

    /// Records a Lens-made response and returns it.
    fn local(&self, response: Response<LensBody>, responder: Responder) -> Response<LensBody> {
        self.recorder.response_head(
            response.status(),
            response.version(),
            response.headers(),
            responder,
        );
        let (parts, body) = response.into_parts();
        let body = TeeBody::new(body, Side::Response, self.recorder.clone(), self.cap());
        Response::from_parts(parts, body.boxed_unsync())
    }

    fn blocked(&self, outcome: GateOutcome, title: &str, text: &str) -> Response<LensBody> {
        self.local(
            pages::html(StatusCode::FORBIDDEN, pages::message(title, text)),
            Responder::Gate { reason: outcome },
        )
    }

    fn sign_in(&self, request: &Request<Incoming>) -> SignIn {
        let gates = &self.active.config.gates;
        let tap = self.tap.id.as_str();
        let now = now_unix_secs();
        // A secret link signs in and redirects to the same URL without the key.
        if gates.secret_link.is_some()
            && let Some(query) = request.uri().query()
        {
            let mut key = None;
            let rest: Vec<&str> = query
                .split('&')
                .filter(|pair| match pair.split_once('=') {
                    Some(("key", value)) => {
                        key = Some(value);
                        false
                    }
                    _ => true,
                })
                .collect();
            if let Some(key) = key
                && gates.link_ok(key)
            {
                let mut location = request.uri().path().to_owned();
                if !rest.is_empty() {
                    location.push('?');
                    location.push_str(&rest.join("&"));
                }
                let mut response = Response::new(empty());
                *response.status_mut() = StatusCode::SEE_OTHER;
                let headers = response.headers_mut();
                if let Ok(location) = HeaderValue::from_str(&gate::safe_next(&location)) {
                    headers.insert(header::LOCATION, location);
                }
                headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
                headers.insert(
                    header::REFERRER_POLICY,
                    HeaderValue::from_static("no-referrer"),
                );
                if let Some(cookie) =
                    self.shared
                        .sessions
                        .set_cookie(gates, tap, &self.active.fingerprint, now)
                {
                    headers.insert(header::SET_COOKIE, cookie);
                }
                return SignIn::Respond(response, GateOutcome::LinkAccepted);
            }
        }
        if let Some(cookie) = gate::cookie_value(request.headers(), gates.cookie_name())
            && self
                .shared
                .sessions
                .check(cookie, tap, &self.active.fingerprint, now)
        {
            return SignIn::Admitted { basic: false };
        }
        if gates.basic_ok(request.headers()) || gates.bearer_ok(request.headers()) {
            return SignIn::Admitted { basic: true };
        }
        if gates.password.is_some() {
            let next = request
                .uri()
                .path_and_query()
                .map_or("/", http::uri::PathAndQuery::as_str);
            let page = pages::password(LOGIN_PATH, &gate::safe_next(next), None);
            return SignIn::Respond(
                pages::html(StatusCode::UNAUTHORIZED, page),
                GateOutcome::PasswordRequired,
            );
        }
        if gates.basic.is_some() {
            let mut response = pages::html(
                StatusCode::UNAUTHORIZED,
                pages::message(
                    "Sign in required",
                    "This site needs a user name and password.",
                ),
            );
            response.headers_mut().insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Basic realm=\"Teitunnel\", charset=\"UTF-8\""),
            );
            return SignIn::Respond(response, GateOutcome::BasicAuthRequired);
        }
        if !gates.bearer.is_empty() {
            let mut response = Response::new(crate::body::full(
                "{\"error\":\"unauthorized\",\"message\":\"A bearer token is required.\"}\n",
            ));
            *response.status_mut() = StatusCode::UNAUTHORIZED;
            let headers = response.headers_mut();
            headers.insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/json"),
            );
            headers.insert(
                header::WWW_AUTHENTICATE,
                HeaderValue::from_static("Bearer realm=\"Teitunnel\""),
            );
            headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            return SignIn::Respond(response, GateOutcome::BearerRequired);
        }
        SignIn::Respond(
            pages::html(
                StatusCode::FORBIDDEN,
                pages::message(
                    "Link required",
                    "Open this site with the link you were given.",
                ),
            ),
            GateOutcome::LinkRequired,
        )
    }

    async fn login(&self, request: Request<Incoming>) -> Response<LensBody> {
        let gates = &self.active.config.gates;
        let ip = self.client_ip;
        let limiter = &self.tap.limiter;
        if limiter.limited(ip) {
            let mut response = pages::html(
                StatusCode::TOO_MANY_REQUESTS,
                pages::message(
                    "Too many attempts",
                    "Please wait a few minutes and try again.",
                ),
            );
            response.headers_mut().insert(
                header::RETRY_AFTER,
                HeaderValue::from(limiter.retry_after(ip)),
            );
            return self.local(
                response,
                Responder::Gate {
                    reason: GateOutcome::RateLimited,
                },
            );
        }
        let (_, body) = request.into_parts();
        // The form holds the password: never capture its bytes, only its size.
        let form = match read_limited(body, MAX_LOGIN_BODY).await {
            Ok((data, true)) => {
                self.recorder.body_end(
                    Side::Request,
                    BodyRecord {
                        data: Bytes::new(),
                        size: data.len() as u64,
                        truncated: !data.is_empty(),
                        complete: true,
                    },
                    None,
                );
                gate::parse_form(&data)
            }
            _ => {
                return self.local(
                    pages::text(StatusCode::PAYLOAD_TOO_LARGE, "Form too large\n".into()),
                    Responder::Lens,
                );
            }
        };
        let next = gate::safe_next(form.get("next").map_or("/", String::as_str));
        let password = form.get("password").cloned().unwrap_or_default();
        let Some(check) = gates.password.clone() else {
            return self.local(
                pages::text(StatusCode::NOT_FOUND, "Not found\n".into()),
                Responder::Lens,
            );
        };
        let valid = match self.shared.password_checks.acquire().await {
            Ok(_permit) => tokio::task::spawn_blocking(move || check.verify(&password))
                .await
                .unwrap_or(false),
            Err(_) => false,
        };
        if !valid {
            limiter.failed(ip);
            let page = pages::password(LOGIN_PATH, &next, Some("That password isn't right."));
            return self.local(
                pages::html(StatusCode::UNAUTHORIZED, page),
                Responder::Gate {
                    reason: GateOutcome::PasswordWrong,
                },
            );
        }
        limiter.succeeded(ip);
        let mut response = Response::new(empty());
        *response.status_mut() = StatusCode::SEE_OTHER;
        let headers = response.headers_mut();
        if let Ok(location) = HeaderValue::from_str(&next) {
            headers.insert(header::LOCATION, location);
        }
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
        if let Some(cookie) = self.shared.sessions.set_cookie(
            gates,
            self.tap.id.as_str(),
            &self.active.fingerprint,
            now_unix_secs(),
        ) {
            headers.insert(header::SET_COOKIE, cookie);
        }
        self.local(
            response,
            Responder::Gate {
                reason: GateOutcome::SignedIn,
            },
        )
    }

    async fn reserved(&self, request: Request<Incoming>) -> Response<LensBody> {
        let Some(handler) = self.active.config.reserved.clone() else {
            return self.local(
                pages::text(StatusCode::NOT_FOUND, "Not found\n".into()),
                Responder::Lens,
            );
        };
        let (parts, body) = request.into_parts();
        let Ok((body, true)) = read_limited(body, MAX_RESERVED_BODY).await else {
            return self.local(
                pages::text(StatusCode::PAYLOAD_TOO_LARGE, "Body too large\n".into()),
                Responder::Lens,
            );
        };
        self.recorder
            .body_end(Side::Request, capped(&body, self.cap()), None);
        let response = handler
            .handle(ReservedRequest {
                tap: self.tap.id.clone(),
                method: parts.method,
                uri: parts.uri,
                headers: parts.headers,
                body,
                client_ip: self.client_ip,
            })
            .await;
        self.local(response, Responder::Lens)
    }

    /// Reads (and records) a request body that won't be forwarded, up to the cap.
    async fn capture_body_only(&self, body: Incoming) {
        if body.is_end_stream() {
            return;
        }
        let limit = self.cap();
        let read = tokio::time::timeout(
            self.listener.limits.header_read_timeout,
            read_limited(body, limit),
        )
        .await;
        if let Ok(Ok((data, complete))) = read {
            let mut record = capped(&data, limit);
            record.complete = complete;
            record.truncated = !complete;
            self.recorder.body_end(Side::Request, record, None);
        }
    }

    async fn forward(
        &self,
        client: Arc<OriginClient>,
        request: Request<Incoming>,
        client_upgrade: Option<hyper::upgrade::OnUpgrade>,
        session_cookie: Option<&'static str>,
        strip_authorization: bool,
    ) -> Response<LensBody> {
        let config = &self.active.config;
        let (parts, body) = request.into_parts();
        let body: LensBody = match &self.active.up {
            Some(bucket) => ThrottledBody::new(body, Arc::clone(bucket)).boxed_unsync(),
            None => body.map_err(BoxError::from).boxed_unsync(),
        };
        let path = parts.uri.path();
        let fallback = stub::find(
            &config.stubs,
            StubMode::WhenUnreachable,
            &parts.method,
            path,
        )
        .map(|(index, rule)| (index, rule.clone()));

        let mut headers = parts.headers.clone();
        if let Some(name) = session_cookie {
            gate::strip_cookie(&mut headers, name);
        }
        if strip_authorization {
            headers.remove(header::AUTHORIZATION);
        }
        headers.remove(header::EXPECT);
        let inject = config
            .injection
            .as_ref()
            .filter(|_| inject::wants_injection(&parts.method, &parts.headers));
        if inject.is_some() {
            headers.remove(header::ACCEPT_ENCODING);
        }

        // With a fallback stub, hold a small body so it's still recorded (and could be
        // resent) if the origin is down; larger bodies stream as usual.
        let upstream_body = if fallback.is_some() && !body.is_end_stream() {
            let mut body = body;
            match read_prefix(&mut body, self.cap()).await {
                Ok((data, true)) => {
                    self.recorder
                        .body_end(Side::Request, BodyRecord::full(data.clone()), None);
                    crate::body::full(data)
                }
                Ok((prefix, false)) => TeeBody::new(
                    PrefixedBody::new(prefix, body),
                    Side::Request,
                    self.recorder.clone(),
                    self.cap(),
                )
                .boxed_unsync(),
                Err(err) => {
                    self.recorder.note_error(ExchangeError {
                        kind: ErrorKind::ClientAborted,
                        message: format!("reading the request body failed: {err}"),
                    });
                    return self.local(
                        pages::text(StatusCode::BAD_REQUEST, "Bad request body\n".into()),
                        Responder::Lens,
                    );
                }
            }
        } else {
            TeeBody::new(body, Side::Request, self.recorder.clone(), self.cap()).boxed_unsync()
        };

        let incoming = forward::Incoming {
            method: &parts.method,
            uri: &parts.uri,
            host: &self.host,
            client_ip: self.client_ip,
            scheme: self.scheme,
            upgrade: client_upgrade.is_some(),
        };
        let mut upstream_request = forward::origin_request(
            client.config(),
            &config.host_header,
            config.forwarded,
            &incoming,
            headers,
            upstream_body,
        );
        rules::apply(&self.active.request_ops, upstream_request.headers_mut());

        let recorder = self.recorder.clone();
        let result = client
            .send(upstream_request, move || recorder.connected())
            .await;
        let mut response = match result {
            Ok(response) => response,
            Err(err) => {
                if err.kind.is_unreachable()
                    && let Some((index, rule)) = fallback
                {
                    return self
                        .finish(
                            rule.response(),
                            Responder::Stub {
                                rule: index,
                                fallback: true,
                            },
                            &parts.headers,
                            false,
                        )
                        .await;
                }
                self.recorder.note_error(ExchangeError {
                    kind: err.kind,
                    message: err.message,
                });
                return self.local(error_page(err.kind, &parts.headers), Responder::Lens);
            }
        };

        if let Some(client_upgrade) = client_upgrade
            && response.status() == StatusCode::SWITCHING_PROTOCOLS
        {
            let server_upgrade = hyper::upgrade::on(&mut response);
            let (mut head, _) = response.into_parts();
            forward::response_headers(&mut head.headers, true);
            rules::apply(&self.active.response_ops, &mut head.headers);
            let websocket = head
                .headers
                .get(header::UPGRADE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| value.eq_ignore_ascii_case("websocket"));
            self.recorder.set_kind(if websocket {
                ExchangeKind::WebSocket
            } else {
                ExchangeKind::Upgrade
            });
            self.recorder.response_head(
                head.status,
                head.version,
                &head.headers,
                Responder::Upstream,
            );
            self.shared.tracker.spawn(tunnel::run(
                client_upgrade,
                server_upgrade,
                self.recorder.clone(),
                websocket,
                crate::stream::Deflate::negotiated(&head.headers),
                self.limits(),
                self.listener.cancel.clone(),
            ));
            head.version = http::Version::HTTP_11;
            return Response::from_parts(head, empty());
        }

        let (mut head, body) = response.into_parts();
        forward::response_headers(&mut head.headers, false);
        let response = Response::from_parts(head, body);
        self.finish(
            response,
            Responder::Upstream,
            &parts.headers,
            inject.is_some(),
        )
        .await
    }

    /// Response rules, CORS, capture and injection for a response headed to the client.
    async fn finish(
        &self,
        response: Response<LensBody>,
        responder: Responder,
        request_headers: &HeaderMap,
        inject: bool,
    ) -> Response<LensBody> {
        let config = &self.active.config;
        let (mut head, body) = response.into_parts();
        rules::apply(&self.active.response_ops, &mut head.headers);
        if config.headers.cors {
            rules::add_cors(request_headers, &mut head.headers);
        }
        let event_stream = head
            .headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .trim_start()
                    .to_ascii_lowercase()
                    .starts_with("text/event-stream")
            });
        if event_stream {
            self.recorder.set_kind(ExchangeKind::Sse);
        }
        // The client talks HTTP/1.1 or HTTP/2 to Lens regardless of the origin.
        head.version = http::Version::HTTP_11;
        self.recorder
            .response_head(head.status, head.version, &head.headers, responder);
        let mut tee = TeeBody::new(body, Side::Response, self.recorder.clone(), self.cap());
        if event_stream {
            tee = tee.with_sse(self.limits());
        }
        let status = head.status;
        let mut body = tee.boxed_unsync();
        if let Some(bucket) = &self.active.down {
            body = ThrottledBody::new(body, Arc::clone(bucket)).boxed_unsync();
        }
        if event_stream && let Some(idle) = config.sse_keepalive {
            body = KeepAliveBody::new(body, idle).boxed_unsync();
        }
        let response = Response::from_parts(head, body);
        match (&config.injection, inject) {
            (Some(injection), true) => inject::apply(injection, status, response).await,
            _ => response,
        }
    }
}

enum SignIn {
    Admitted { basic: bool },
    Respond(Response<LensBody>, GateOutcome),
}

/// The first `cap` bytes of `data` as a complete body record.
fn capped(data: &Bytes, cap: usize) -> BodyRecord {
    BodyRecord {
        data: data.slice(..data.len().min(cap)),
        size: data.len() as u64,
        truncated: data.len() > cap,
        complete: true,
    }
}

/// What a visitor sees when the origin fails. Details stay in the capture: visitors
/// shouldn't learn about the machine behind the tunnel.
fn error_page(kind: ErrorKind, request_headers: &HeaderMap) -> Response<LensBody> {
    let status = if kind == ErrorKind::Timeout {
        StatusCode::GATEWAY_TIMEOUT
    } else {
        StatusCode::BAD_GATEWAY
    };
    let (title, text) = if kind == ErrorKind::Timeout {
        (
            "This app is taking too long",
            "It didn't answer in time. Please try again in a moment.",
        )
    } else {
        (
            "This app isn't answering",
            "The app behind this address isn't reachable right now. Please try again in a moment.",
        )
    };
    let wants_html = request_headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"));
    if wants_html {
        pages::html(status, pages::message(title, text))
    } else {
        pages::text(status, format!("{title}. {text}\n"))
    }
}
