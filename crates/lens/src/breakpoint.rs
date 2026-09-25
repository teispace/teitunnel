//! Breakpoints: stop matching requests before they reach the service, or their answers
//! before they reach the visitor, so someone can look, change and let them go.
//!
//! A paused exchange waits in [`Breaks`] until [`crate::Lens::resume`] (or
//! [`crate::Lens::resume_all`]) lets it go, the visitor leaves, Lens shuts down, or
//! [`BREAK_TIMEOUT`] passes; then it goes on unchanged. Only bodies that are complete,
//! known in size, text and at most [`MAX_EDIT_BODY`] can be changed; others go on as they
//! are. Upgrades and event streams never stop.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use bytes::Bytes;
use http::{HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri, header};
use serde::{Deserialize, Serialize};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::{ExchangeId, LensError, PathPattern, TapId};

/// How long an exchange waits before it goes on by itself: well within Cloudflare's
/// 100-second wait for an answer, leaving the service time to reply.
pub const BREAK_TIMEOUT: Duration = Duration::from_secs(60);

/// Exchanges paused at once, across taps; more go on without stopping, so a broad rule
/// can't pile up visitors.
pub const MAX_PAUSED: usize = 50;

/// Largest body that can be changed at a breakpoint.
pub const MAX_EDIT_BODY: usize = 1024 * 1024;

/// Where an exchange stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BreakStage {
    /// Before the request goes to the service.
    Request,
    /// Before the answer goes back to the visitor.
    Response,
}

/// Stops requests matching a method and path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BreakpointRule {
    /// Method to match (case-insensitive); `None` matches any.
    pub method: Option<String>,
    /// Path to match.
    #[cfg_attr(feature = "specta", specta(type = String))]
    pub path: PathPattern,
    /// Stop before the request goes to the service.
    pub request: bool,
    /// Stop before the answer goes back.
    pub response: bool,
}

impl BreakpointRule {
    /// Whether the rule applies to `method` and `path`.
    pub fn matches(&self, method: &Method, path: &str) -> bool {
        self.method
            .as_deref()
            .is_none_or(|m| m.eq_ignore_ascii_case(method.as_str()))
            && self.path.matches(path)
    }

    /// Checks the method and that the rule stops somewhere.
    ///
    /// # Errors
    /// [`LensError::InvalidConfig`].
    pub fn validate(&self) -> Result<(), LensError> {
        if !self.request && !self.response {
            return Err(LensError::InvalidConfig(
                "a breakpoint must stop the request, the response or both".into(),
            ));
        }
        if let Some(method) = &self.method {
            Method::from_bytes(method.as_bytes())
                .map_err(|_| LensError::InvalidConfig(format!("invalid method {method:?}")))?;
        }
        Ok(())
    }
}

/// Where a matching exchange stops: the stages of the first rule that matches.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    pub(crate) request: bool,
    pub(crate) response: bool,
}

pub(crate) fn plan(rules: &[BreakpointRule], method: &Method, path: &str) -> Plan {
    rules
        .iter()
        .find(|rule| rule.matches(method, path))
        .map_or_else(Plan::default, |rule| Plan {
            request: rule.request,
            response: rule.response,
        })
}

/// A breakpoint's mark on a captured exchange.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BreakRecord {
    /// Where it waits now (`None` once it went on).
    pub waiting: Option<BreakStage>,
    /// The request was changed at the breakpoint.
    pub request_edited: bool,
    /// The answer was changed at the breakpoint.
    pub response_edited: bool,
    /// It went on by itself after [`BREAK_TIMEOUT`].
    pub timed_out: bool,
}

/// Why a body can't be changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum BodyLock {
    /// It isn't text (or is compressed).
    Binary,
    /// It's larger than [`MAX_EDIT_BODY`].
    TooLarge,
    /// Its size isn't known up front (it's still arriving).
    Streamed,
}

/// A paused exchange, as it would go on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Paused {
    /// The exchange.
    pub exchange: ExchangeId,
    /// Its tap.
    pub tap: TapId,
    /// Where it waits.
    pub stage: BreakStage,
    /// When it stopped (Unix milliseconds).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub since_ms: u64,
    /// When it goes on by itself (Unix milliseconds).
    #[cfg_attr(feature = "specta", specta(type = f64))]
    pub resumes_at_ms: u64,
    /// Request method.
    pub method: String,
    /// Request path and query.
    pub target: String,
    /// Host the visitor asked for.
    pub host: String,
    /// Response status (at the response stage).
    pub status: Option<u16>,
    /// Headers of the request, or of the answer at the response stage, in order.
    pub headers: Vec<(String, String)>,
    /// The body as text, when it can be changed.
    pub body: Option<String>,
    /// Why the body can't be changed.
    pub body_locked: Option<BodyLock>,
}

/// Changes to a paused exchange; fields left out stay as they are.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct BreakEdit {
    /// Request method (request stage).
    pub method: Option<String>,
    /// Request path and query, starting with `/` (request stage).
    pub target: Option<String>,
    /// Response status (response stage).
    pub status: Option<u16>,
    /// Every header, replacing them all.
    pub headers: Option<Vec<(String, String)>>,
    /// The body (only when it could be changed).
    pub body: Option<String>,
}

/// How a paused exchange goes on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Resume {
    /// As it is.
    Continue,
    /// With changes.
    Edited {
        /// The changes.
        edit: BreakEdit,
    },
    /// Answer the visitor from here, without the service (request stage).
    Answer {
        /// Status.
        status: u16,
        /// Headers.
        headers: Vec<(String, String)>,
        /// Body (text).
        body: String,
    },
    /// Drop the connection: the visitor gets no answer.
    Abort,
}

/// Validated changes, ready to apply.
#[derive(Debug, Clone, Default)]
pub(crate) struct Changes {
    pub(crate) method: Option<Method>,
    pub(crate) uri: Option<Uri>,
    pub(crate) status: Option<StatusCode>,
    pub(crate) headers: Option<HeaderMap>,
    pub(crate) body: Option<Bytes>,
}

/// A validated [`Resume`].
#[derive(Debug, Clone)]
pub(crate) enum Resolved {
    Continue,
    Edited(Changes),
    Answer {
        status: StatusCode,
        headers: HeaderMap,
        body: Bytes,
    },
    Abort,
}

fn header_map(headers: &[(String, String)]) -> Result<HeaderMap, LensError> {
    let mut map = HeaderMap::with_capacity(headers.len());
    for (name, value) in headers {
        let name = HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|_| LensError::InvalidConfig(format!("invalid header name {name:?}")))?;
        let value = HeaderValue::from_str(value.trim()).map_err(|_| {
            LensError::InvalidConfig(format!("invalid value for header {}", name.as_str()))
        })?;
        map.append(name, value);
    }
    Ok(map)
}

fn status(code: u16) -> Result<StatusCode, LensError> {
    StatusCode::from_u16(code)
        .ok()
        .filter(|s| (200..=599).contains(&s.as_u16()))
        .ok_or_else(|| {
            LensError::InvalidConfig(format!("status {code} must be between 200 and 599"))
        })
}

/// Checks `resume` against what's paused.
pub(crate) fn resolve(paused: &Paused, resume: Resume) -> Result<Resolved, LensError> {
    let request = paused.stage == BreakStage::Request;
    Ok(match resume {
        Resume::Continue => Resolved::Continue,
        Resume::Abort => Resolved::Abort,
        Resume::Answer {
            status: code,
            headers,
            body,
        } => {
            if !request {
                return Err(LensError::InvalidConfig(
                    "only a paused request can be answered from here; change the answer instead"
                        .into(),
                ));
            }
            Resolved::Answer {
                status: status(code)?,
                headers: header_map(&headers)?,
                body: Bytes::from(body),
            }
        }
        Resume::Edited { edit } => {
            if request && edit.status.is_some() {
                return Err(LensError::InvalidConfig(
                    "a paused request has no status to change".into(),
                ));
            }
            if !request && (edit.method.is_some() || edit.target.is_some()) {
                return Err(LensError::InvalidConfig(
                    "the method and path can only change before the request goes on".into(),
                ));
            }
            if edit.body.is_some() && paused.body_locked.is_some() {
                return Err(LensError::InvalidConfig(
                    "this body can't be changed here".into(),
                ));
            }
            if edit.body.as_ref().is_some_and(|b| b.len() > MAX_EDIT_BODY) {
                return Err(LensError::InvalidConfig(format!(
                    "a changed body can be at most {MAX_EDIT_BODY} bytes"
                )));
            }
            let method = edit
                .method
                .map(|m| {
                    Method::from_bytes(m.trim().as_bytes())
                        .map_err(|_| LensError::InvalidConfig(format!("invalid method {m:?}")))
                })
                .transpose()?;
            let uri = edit
                .target
                .map(|target| {
                    let target = target.trim();
                    target
                        .parse::<Uri>()
                        .ok()
                        .filter(|uri| target.starts_with('/') && uri.authority().is_none())
                        .ok_or_else(|| {
                            LensError::InvalidConfig(format!(
                                "the path {target:?} must start with /"
                            ))
                        })
                })
                .transpose()?;
            Resolved::Edited(Changes {
                method,
                uri,
                status: edit.status.map(status).transpose()?,
                headers: edit.headers.as_deref().map(header_map).transpose()?,
                body: edit.body.map(Bytes::from),
            })
        }
    })
}

/// Sets `Content-Length` for a replaced body (and drops framing that no longer fits).
pub(crate) fn fit_length(headers: &mut HeaderMap, length: usize) {
    headers.remove(header::TRANSFER_ENCODING);
    headers.remove(header::CONTENT_ENCODING);
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(length));
}

/// Headers as text, in order (values that aren't text are shown lossily).
pub(crate) fn header_list(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_owned(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect()
}

/// How much of a body to read before stopping, from its headers: the whole of it when
/// its size is known and small enough, else why it can't be changed.
pub(crate) fn readable(headers: &HeaderMap, end_of_stream: bool) -> Result<usize, BodyLock> {
    if end_of_stream {
        return Ok(0);
    }
    if headers.contains_key(header::CONTENT_ENCODING) {
        return Err(BodyLock::Binary);
    }
    let length = headers
        .get(header::CONTENT_LENGTH)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.trim().parse::<usize>().ok())
        .ok_or(BodyLock::Streamed)?;
    if length > MAX_EDIT_BODY {
        Err(BodyLock::TooLarge)
    } else {
        Ok(length)
    }
}

#[derive(Debug)]
struct Waiting {
    paused: Paused,
    resume: oneshot::Sender<Resolved>,
}

/// The exchanges paused right now.
#[derive(Debug, Default)]
pub(crate) struct Breaks {
    waiting: Mutex<HashMap<ExchangeId, Waiting>>,
}

/// Removes its exchange from [`Breaks`] when the pipeline moves on (or is dropped
/// because the visitor left).
#[derive(Debug)]
pub(crate) struct Ticket {
    breaks: Arc<Breaks>,
    id: ExchangeId,
}

impl Drop for Ticket {
    fn drop(&mut self) {
        self.breaks.lock().remove(&self.id);
    }
}

impl Breaks {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<ExchangeId, Waiting>> {
        self.waiting.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Pauses `paused`; `None` when [`MAX_PAUSED`] are already waiting.
    pub(crate) fn park(
        self: &Arc<Self>,
        paused: Paused,
    ) -> Option<(Ticket, oneshot::Receiver<Resolved>)> {
        let mut waiting = self.lock();
        if waiting.len() >= MAX_PAUSED {
            return None;
        }
        let (resume, rx) = oneshot::channel();
        let id = paused.exchange;
        waiting.insert(id, Waiting { paused, resume });
        Some((
            Ticket {
                breaks: Arc::clone(self),
                id,
            },
            rx,
        ))
    }

    /// What's paused (one tap's, or all), oldest first.
    pub(crate) fn list(&self, tap: Option<&TapId>) -> Vec<Paused> {
        let mut list: Vec<Paused> = self
            .lock()
            .values()
            .filter(|w| tap.is_none_or(|tap| &w.paused.tap == tap))
            .map(|w| w.paused.clone())
            .collect();
        list.sort_by_key(|p| (p.since_ms, p.exchange));
        list
    }

    pub(crate) fn get(&self, id: ExchangeId) -> Option<Paused> {
        self.lock().get(&id).map(|w| w.paused.clone())
    }

    /// Lets one exchange go on, if `resume` fits it.
    pub(crate) fn resume(&self, id: ExchangeId, resume: Resume) -> Result<(), LensError> {
        let mut waiting = self.lock();
        let entry = waiting.get(&id).ok_or(LensError::NotPaused(id))?;
        let resolved = resolve(&entry.paused, resume)?;
        let entry = waiting.remove(&id).ok_or(LensError::NotPaused(id))?;
        entry
            .resume
            .send(resolved)
            .map_err(|_| LensError::NotPaused(id))
    }

    /// Lets every exchange (of one tap, or all) go on unchanged; returns how many.
    pub(crate) fn resume_all(&self, tap: Option<&TapId>) -> usize {
        let mut waiting = self.lock();
        let ids: Vec<ExchangeId> = waiting
            .iter()
            .filter(|(_, w)| tap.is_none_or(|tap| &w.paused.tap == tap))
            .map(|(id, _)| *id)
            .collect();
        ids.iter()
            .filter_map(|id| waiting.remove(id))
            .map(|entry| entry.resume.send(Resolved::Continue).is_ok())
            .filter(|sent| *sent)
            .count()
    }
}

/// Waits for someone to let the exchange go; `true` when it went on by itself.
pub(crate) async fn wait(
    rx: oneshot::Receiver<Resolved>,
    cancel: &CancellationToken,
    timeout: Duration,
) -> (Resolved, bool) {
    tokio::select! {
        resolved = rx => (resolved.unwrap_or(Resolved::Continue), false),
        () = tokio::time::sleep(timeout) => (Resolved::Continue, true),
        () = cancel.cancelled() => (Resolved::Continue, false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(method: Option<&str>, path: &str, request: bool, response: bool) -> BreakpointRule {
        BreakpointRule {
            method: method.map(str::to_owned),
            path: PathPattern::parse(path).unwrap(),
            request,
            response,
        }
    }

    fn paused(stage: BreakStage, body_locked: Option<BodyLock>) -> Paused {
        Paused {
            exchange: ExchangeId::new(),
            tap: TapId::new("t").unwrap(),
            stage,
            since_ms: 1,
            resumes_at_ms: 60_001,
            method: "POST".into(),
            target: "/hooks".into(),
            host: "app.test".into(),
            status: (stage == BreakStage::Response).then_some(200),
            headers: vec![("content-type".into(), "application/json".into())],
            body: body_locked.is_none().then(|| "{}".into()),
            body_locked,
        }
    }

    #[test]
    fn the_first_matching_rule_decides_where_to_stop() {
        let rules = vec![
            rule(Some("post"), "/hooks/*", true, false),
            rule(None, "/*", false, true),
        ];
        assert_eq!(
            plan(&rules, &Method::POST, "/hooks/stripe"),
            Plan {
                request: true,
                response: false
            }
        );
        assert_eq!(
            plan(&rules, &Method::GET, "/hooks/stripe"),
            Plan {
                request: false,
                response: true
            }
        );
        assert_eq!(plan(&[], &Method::GET, "/"), Plan::default());
        assert!(rule(None, "/", false, false).validate().is_err());
        assert!(
            rule(Some("BAD METHOD"), "/", true, false)
                .validate()
                .is_err()
        );
        assert!(rule(Some("PATCH"), "/", true, false).validate().is_ok());
    }

    #[test]
    fn changes_must_fit_the_stage_and_the_body() {
        let at_request = paused(BreakStage::Request, None);
        let edit = |edit: BreakEdit| Resume::Edited { edit };
        let Resolved::Edited(changes) = resolve(
            &at_request,
            edit(BreakEdit {
                method: Some("put".into()),
                target: Some("/hooks?x=1".into()),
                headers: Some(vec![("x-a".into(), "1".into()), ("x-a".into(), "2".into())]),
                body: Some("{\"a\":1}".into()),
                ..BreakEdit::default()
            }),
        )
        .unwrap() else {
            panic!("not an edit");
        };
        // An unknown method is still a valid token.
        assert_eq!(changes.method.unwrap().as_str(), "put");
        assert_eq!(changes.uri.unwrap().query(), Some("x=1"));
        assert_eq!(changes.headers.unwrap().get_all("x-a").iter().count(), 2);

        for bad in [
            BreakEdit {
                status: Some(500),
                ..BreakEdit::default()
            },
            BreakEdit {
                target: Some("http://elsewhere.test/".into()),
                ..BreakEdit::default()
            },
            BreakEdit {
                headers: Some(vec![("bad name".into(), "1".into())]),
                ..BreakEdit::default()
            },
        ] {
            assert!(resolve(&at_request, edit(bad)).is_err());
        }

        let at_response = paused(BreakStage::Response, None);
        assert!(
            resolve(
                &at_response,
                edit(BreakEdit {
                    method: Some("GET".into()),
                    ..BreakEdit::default()
                })
            )
            .is_err()
        );
        assert!(
            resolve(
                &at_response,
                Resume::Answer {
                    status: 200,
                    headers: vec![],
                    body: String::new()
                }
            )
            .is_err()
        );
        assert!(
            resolve(
                &at_response,
                edit(BreakEdit {
                    status: Some(99),
                    ..BreakEdit::default()
                })
            )
            .is_err()
        );

        let locked = paused(BreakStage::Request, Some(BodyLock::Binary));
        assert!(
            resolve(
                &locked,
                edit(BreakEdit {
                    body: Some("x".into()),
                    ..BreakEdit::default()
                })
            )
            .is_err()
        );
    }

    #[test]
    fn bodies_are_editable_only_when_small_known_and_plain() {
        let headers = |pairs: &[(&'static str, &'static str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.insert(*name, HeaderValue::from_static(value));
            }
            map
        };
        assert_eq!(readable(&HeaderMap::new(), true), Ok(0));
        assert_eq!(
            readable(&headers(&[("content-length", "12")]), false),
            Ok(12)
        );
        assert_eq!(readable(&HeaderMap::new(), false), Err(BodyLock::Streamed));
        assert_eq!(
            readable(&headers(&[("content-length", "2000000")]), false),
            Err(BodyLock::TooLarge)
        );
        assert_eq!(
            readable(
                &headers(&[("content-length", "12"), ("content-encoding", "gzip")]),
                false
            ),
            Err(BodyLock::Binary)
        );
    }

    #[tokio::test]
    async fn a_paused_exchange_goes_on_once_and_leaves_the_list() {
        let breaks = Arc::new(Breaks::default());
        let first = paused(BreakStage::Request, None);
        let id = first.exchange;
        let (ticket, rx) = breaks.park(first).unwrap();
        assert_eq!(breaks.list(None).len(), 1);
        assert!(breaks.resume(id, Resume::Continue).is_ok());
        assert!(matches!(
            wait(rx, &CancellationToken::new(), BREAK_TIMEOUT).await,
            (Resolved::Continue, false)
        ));
        assert!(breaks.list(None).is_empty());
        assert!(matches!(
            breaks.resume(id, Resume::Continue),
            Err(LensError::NotPaused(_))
        ));
        drop(ticket);

        // A rejected change leaves it waiting.
        let second = paused(BreakStage::Response, None);
        let id = second.exchange;
        let (ticket, _rx) = breaks.park(second).unwrap();
        let bad = Resume::Edited {
            edit: BreakEdit {
                method: Some("GET".into()),
                ..BreakEdit::default()
            },
        };
        assert!(breaks.resume(id, bad).is_err());
        assert!(breaks.get(id).is_some());
        // The visitor left: the ticket takes it off the list.
        drop(ticket);
        assert!(breaks.get(id).is_none());
    }

    #[tokio::test]
    async fn it_goes_on_by_itself_and_never_piles_up() {
        let breaks = Arc::new(Breaks::default());
        let (_ticket, rx) = breaks.park(paused(BreakStage::Request, None)).unwrap();
        assert!(matches!(
            wait(rx, &CancellationToken::new(), Duration::from_millis(10)).await,
            (Resolved::Continue, true)
        ));
        let tickets: Vec<_> = (1..MAX_PAUSED)
            .map(|_| breaks.park(paused(BreakStage::Request, None)).unwrap())
            .collect();
        assert!(breaks.park(paused(BreakStage::Request, None)).is_none());
        // The first one already went on by itself; the rest go on now.
        assert_eq!(breaks.resume_all(None), MAX_PAUSED - 1);
        drop(tickets);
    }
}
