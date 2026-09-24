//! Replay: send a captured request again, as-is or edited, N times, to its upstream or
//! another one. Each replay is captured (marked `replay_of`) and returned when done.
//! Gates and stubs don't apply (the person replaying is the operator); the tap's
//! request header rules and `Host` setting do.

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, Response, Uri, header};
use http_body_util::BodyExt;

use crate::{
    BodyRecord, ClientInfo, Exchange, ExchangeError, ExchangeId, ExchangeKind, ExchangeState,
    ForwardedHeaders, LensError, RequestRecord, Responder, Timings, Upstream,
    body::{BoxError, TeeBody, full},
    capture::ErrorKind,
    forward,
    lens::Shared,
    recorder::{Recorder, Side},
    rules,
    tap::{Active, TapRuntime},
    upstream::ActiveUpstream,
    util::{now_unix_ms, now_unix_secs},
    webhook::{self, Provider, WebhookSecret},
};

/// Most repetitions in one replay call.
pub const MAX_REPLAYS: u32 = 100;

/// Changes applied to the captured request before sending it again.
#[derive(Debug, Clone, Default)]
pub struct RequestEdits {
    /// New method.
    pub method: Option<String>,
    /// New path and query (must start with `/`).
    pub path_and_query: Option<String>,
    /// Headers to set (replacing existing values).
    pub set_headers: Vec<(String, String)>,
    /// Headers to remove.
    pub remove_headers: Vec<String>,
    /// New body.
    pub body: Option<Bytes>,
}

/// Where a replay goes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ReplayTarget {
    /// The tap's current upstream.
    #[default]
    Original,
    /// Another upstream (e.g. a second dev server).
    Upstream(Upstream),
}

/// Recompute a webhook signature for each replay (fresh timestamp).
#[derive(Debug, Clone)]
pub struct Resign {
    /// Provider scheme.
    pub provider: Provider,
    /// Signing secret.
    pub secret: WebhookSecret,
}

/// How to replay.
#[derive(Debug, Clone)]
pub struct ReplayOptions {
    /// Edits.
    pub edits: RequestEdits,
    /// Repetitions (1–100), sent one after another.
    pub times: u32,
    /// Target.
    pub target: ReplayTarget,
    /// Re-sign webhooks.
    pub resign: Option<Resign>,
    /// Longest a single replay may take (response body included).
    pub timeout: Duration,
}

impl Default for ReplayOptions {
    fn default() -> Self {
        Self {
            edits: RequestEdits::default(),
            times: 1,
            target: ReplayTarget::Original,
            resign: None,
            timeout: Duration::from_secs(60),
        }
    }
}

struct Prepared {
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
}

fn prepare(original: &Exchange, edits: &RequestEdits) -> Result<Prepared, LensError> {
    let request = &original.request;
    let method = match &edits.method {
        Some(method) => Method::from_bytes(method.as_bytes())
            .map_err(|_| LensError::InvalidConfig(format!("invalid method {method:?}")))?,
        None => request.method.clone(),
    };
    let uri = match &edits.path_and_query {
        Some(target) if target.starts_with('/') => target
            .parse::<Uri>()
            .map_err(|_| LensError::InvalidConfig(format!("invalid path {target:?}")))?,
        Some(target) => {
            return Err(LensError::InvalidConfig(format!(
                "the path {target:?} must start with '/'"
            )));
        }
        None => request.uri.clone(),
    };
    let mut headers = request.headers.clone();
    for name in &edits.remove_headers {
        headers.remove(name.as_str());
    }
    for (name, value) in &edits.set_headers {
        let name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| LensError::InvalidConfig(format!("invalid header name {name:?}")))?;
        let value = HeaderValue::from_str(value)
            .map_err(|_| LensError::InvalidConfig(format!("invalid value for header {name}")))?;
        headers.insert(name, value);
    }
    let body = match &edits.body {
        Some(body) => {
            headers.remove(header::CONTENT_ENCODING);
            body.clone()
        }
        None if request.body.truncated || !request.body.complete => {
            return Err(LensError::BodyTruncated {
                captured: request.body.data.len() as u64,
                total: request.body.size,
            });
        }
        None => request.body.data.clone(),
    };
    headers.remove(header::CONTENT_LENGTH);
    headers.remove(header::TRANSFER_ENCODING);
    headers.remove(header::EXPECT);
    Ok(Prepared {
        method,
        uri,
        headers,
        body,
    })
}

pub(crate) async fn run(
    shared: &Arc<Shared>,
    runtime: &Arc<TapRuntime>,
    original: &Exchange,
    options: &ReplayOptions,
) -> Result<Vec<Arc<Exchange>>, LensError> {
    if !(1..=MAX_REPLAYS).contains(&options.times) {
        return Err(LensError::InvalidConfig(format!(
            "replay between 1 and {MAX_REPLAYS} times"
        )));
    }
    let prepared = prepare(original, &options.edits)?;
    let active = runtime.active();
    let upstream = match &options.target {
        ReplayTarget::Original => active.upstream.clone(),
        ReplayTarget::Upstream(upstream) => ActiveUpstream::build(upstream)?,
    };
    let mut out = Vec::with_capacity(usize::try_from(options.times).unwrap_or(1));
    for _ in 0..options.times {
        let mut headers = prepared.headers.clone();
        if let Some(resign) = &options.resign {
            webhook::resign(
                resign.provider,
                &mut headers,
                &prepared.body,
                &resign.secret,
                now_unix_secs(),
            )
            .map_err(|err| LensError::InvalidConfig(err.to_string()))?;
        }
        let exchange = once(
            shared,
            runtime,
            &active,
            &upstream,
            original,
            &prepared,
            headers,
            options.timeout,
        )
        .await;
        out.push(exchange);
    }
    Ok(out)
}

#[allow(clippy::too_many_arguments)]
async fn once(
    shared: &Arc<Shared>,
    runtime: &Arc<TapRuntime>,
    active: &Active,
    upstream: &ActiveUpstream,
    original: &Exchange,
    prepared: &Prepared,
    headers: HeaderMap,
    timeout: Duration,
) -> Arc<Exchange> {
    let t0 = Instant::now();
    let loopback = IpAddr::V4(Ipv4Addr::LOCALHOST);
    let cap = active.config.capture.max_body_bytes;
    let exchange = Exchange {
        id: ExchangeId::new(),
        seq: runtime.next_seq(),
        tap: runtime.id.clone(),
        kind: ExchangeKind::Http,
        state: ExchangeState::Pending,
        started_at_ms: now_unix_ms(),
        timings: Timings::default(),
        client: ClientInfo {
            ip: loopback,
            peer: SocketAddr::new(loopback, 0),
            cf_ray: None,
            country: None,
        },
        request: RequestRecord {
            method: prepared.method.clone(),
            uri: prepared.uri.clone(),
            scheme: original.request.scheme.clone(),
            host: original.request.host.clone(),
            version: http::Version::HTTP_11,
            headers: headers.clone(),
            body: BodyRecord {
                data: prepared.body.slice(..prepared.body.len().min(cap)),
                size: prepared.body.len() as u64,
                truncated: prepared.body.len() > cap,
                complete: true,
            },
        },
        response: None,
        responder: Responder::Upstream,
        error: None,
        stream: None,
        replay_of: Some(original.id),
        fault: None,
    };
    let id = exchange.id;
    // Replays are always recorded: the caller gets them back from the store.
    let recorder = Recorder::start(
        Arc::clone(&shared.hub),
        Arc::clone(&runtime.metrics),
        true,
        t0,
        exchange,
    );
    let work = async {
        let response = match upstream {
            ActiveUpstream::Folder(folder) => {
                let response = folder
                    .serve(&prepared.method, &prepared.uri, &headers)
                    .await;
                recorder.response_head(
                    response.status(),
                    response.version(),
                    response.headers(),
                    Responder::Folder,
                );
                response
            }
            ActiveUpstream::Origin(client) => {
                let incoming = forward::Incoming {
                    method: &prepared.method,
                    uri: &prepared.uri,
                    host: &original.request.host,
                    client_ip: loopback,
                    scheme: &original.request.scheme,
                    upgrade: false,
                };
                // The captured headers already carry cloudflared's forwarding headers.
                let mut request = forward::origin_request(
                    client.config(),
                    &active.config.host_header,
                    ForwardedHeaders::Off,
                    &incoming,
                    headers.clone(),
                    full(prepared.body.clone()),
                );
                rules::apply(&active.request_ops, request.headers_mut());
                let connected = recorder.clone();
                match client.send(request, move || connected.connected()).await {
                    Ok(response) => {
                        let (mut head, body) = response.into_parts();
                        forward::response_headers(&mut head.headers, false);
                        recorder.response_head(
                            head.status,
                            head.version,
                            &head.headers,
                            Responder::Upstream,
                        );
                        Response::from_parts(head, body)
                    }
                    Err(err) => {
                        recorder.fail(err.kind, err.message);
                        return;
                    }
                }
            }
        };
        drain(response, &recorder, cap).await;
    };
    if tokio::time::timeout(timeout, work).await.is_err() {
        recorder.note_error(ExchangeError {
            kind: ErrorKind::Timeout,
            message: format!("the replay didn't finish within {} s", timeout.as_secs()),
        });
        recorder.finish(None);
    }
    shared
        .hub
        .store
        .get(id)
        .unwrap_or_else(|| Arc::new(recorder.snapshot()))
}

/// Reads the response body to the end through a capturing tap (memory stays bounded by
/// the capture cap).
async fn drain(
    response: Response<http_body_util::combinators::UnsyncBoxBody<Bytes, BoxError>>,
    recorder: &Recorder,
    cap: usize,
) {
    let mut body = TeeBody::new(response.into_body(), Side::Response, recorder.clone(), cap);
    while let Some(frame) = body.frame().await {
        if frame.is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use http::Method;

    use super::*;
    use crate::store::tests::sample;

    #[test]
    fn edits_apply() {
        let original = sample("t", 1, Method::GET, "/a?b=1", 200);
        let edits = RequestEdits {
            method: Some("PUT".into()),
            path_and_query: Some("/c?d=2".into()),
            set_headers: vec![("x-new".into(), "1".into())],
            remove_headers: vec!["authorization".into()],
            body: Some(Bytes::from_static(b"new")),
        };
        let prepared = prepare(&original, &edits).unwrap();
        assert_eq!(prepared.method, Method::PUT);
        assert_eq!(prepared.uri, "/c?d=2");
        assert_eq!(prepared.headers["x-new"], "1");
        assert!(!prepared.headers.contains_key("authorization"));
        assert_eq!(&prepared.body[..], b"new");
    }

    #[test]
    fn rejects_bad_edits_and_truncated_bodies() {
        let original = sample("t", 1, Method::GET, "/", 200);
        let bad = |edits: RequestEdits| prepare(&original, &edits).is_err();
        assert!(bad(RequestEdits {
            method: Some("BAD METHOD".into()),
            ..RequestEdits::default()
        }));
        assert!(bad(RequestEdits {
            path_and_query: Some("no-slash".into()),
            ..RequestEdits::default()
        }));
        assert!(bad(RequestEdits {
            set_headers: vec![("bad name".into(), "v".into())],
            ..RequestEdits::default()
        }));
        let mut truncated = original.clone();
        truncated.request.body.truncated = true;
        assert!(matches!(
            prepare(&truncated, &RequestEdits::default()),
            Err(LensError::BodyTruncated { .. })
        ));
        // Replacing the body makes a truncated capture replayable.
        assert!(
            prepare(
                &truncated,
                &RequestEdits {
                    body: Some(Bytes::new()),
                    ..RequestEdits::default()
                }
            )
            .is_ok()
        );
    }
}
