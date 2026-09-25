//! Builds one [`Exchange`] as the request flows and publishes each step.
//!
//! A [`Recorder`] is shared by the request pipeline and the two body taps (request and
//! response). It updates metrics even when capture is disabled.

use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError, atomic::Ordering::Relaxed},
    time::{Duration, Instant},
};

use http::{HeaderMap, Method, StatusCode, Uri, Version};

use crate::{
    BodyRecord, BreakRecord, Exchange, ExchangeError, ExchangeId, ExchangeKind, ExchangeState,
    Responder, ResponseRecord, StreamStats,
    capture::ErrorKind,
    events::{Change, Hub},
    metrics::TapMetrics,
};

/// Minimum interval between live updates for a busy stream.
const STREAM_UPDATE_INTERVAL: Duration = Duration::from_millis(500);

/// Which body a tap watches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Side {
    Request,
    Response,
}

#[derive(Debug)]
struct State {
    exchange: Exchange,
    finished: bool,
    last_stream_publish: Option<Instant>,
}

#[derive(Debug)]
struct Inner {
    hub: Arc<Hub>,
    metrics: Arc<TapMetrics>,
    capture: bool,
    t0: Instant,
    state: Mutex<State>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        // Every path that forgot to finish (a panic in a handler, a dropped future)
        // still leaves a finished record and balanced gauges.
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        if !state.finished {
            state.finished = true;
            TapMetrics::dec(&self.metrics.active_requests);
            self.metrics.errors.fetch_add(1, Relaxed);
            if self.capture {
                let exchange = &mut state.exchange;
                exchange.state = ExchangeState::Failed;
                exchange.error.get_or_insert(ExchangeError {
                    kind: ErrorKind::ClientAborted,
                    message: "the exchange ended before it completed".into(),
                });
                exchange.timings.complete_us =
                    Some(u64::try_from(self.t0.elapsed().as_micros()).unwrap_or(u64::MAX));
                self.hub.publish(Change::Completed, exchange.clone());
            }
        }
    }
}

/// Shared handle on an exchange being recorded.
#[derive(Debug, Clone)]
pub(crate) struct Recorder(Arc<Inner>);

impl Recorder {
    /// Starts recording `exchange` (publishes `Added` when capture is on).
    pub(crate) fn start(
        hub: Arc<Hub>,
        metrics: Arc<TapMetrics>,
        capture: bool,
        t0: Instant,
        exchange: Exchange,
    ) -> Self {
        metrics.requests.fetch_add(1, Relaxed);
        metrics.active_requests.fetch_add(1, Relaxed);
        if capture {
            hub.publish(Change::Added, exchange.clone());
        }
        Self(Arc::new(Inner {
            hub,
            metrics,
            capture,
            t0,
            state: Mutex::new(State {
                exchange,
                finished: false,
                last_stream_publish: None,
            }),
        }))
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.0.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn metrics(&self) -> &Arc<TapMetrics> {
        &self.0.metrics
    }

    pub(crate) fn capturing(&self) -> bool {
        self.0.capture
    }

    /// Microseconds since the request head arrived.
    pub(crate) fn elapsed_us(&self) -> u64 {
        u64::try_from(self.0.t0.elapsed().as_micros()).unwrap_or(u64::MAX)
    }

    /// A copy of the exchange as recorded so far.
    pub(crate) fn snapshot(&self) -> Exchange {
        self.lock().exchange.clone()
    }

    fn publish(&self, change: Change, exchange: &Exchange) {
        if self.0.capture {
            self.0.hub.publish(change, exchange.clone());
        }
    }

    /// The exchange's id.
    pub(crate) fn id(&self) -> ExchangeId {
        self.lock().exchange.id
    }

    /// Updates the breakpoint mark and publishes it (so a pause shows right away).
    pub(crate) fn breakpoint(&self, update: impl FnOnce(&mut BreakRecord)) {
        let mut state = self.lock();
        update(
            state
                .exchange
                .breakpoint
                .get_or_insert_with(BreakRecord::default),
        );
        let snapshot = state.exchange.clone();
        drop(state);
        self.publish(Change::Updated, &snapshot);
    }

    /// The request as changed at a breakpoint (what the service gets).
    pub(crate) fn edit_request(&self, method: &Method, uri: &Uri, headers: &HeaderMap) {
        let mut state = self.lock();
        let exchange = &mut state.exchange;
        exchange.request.method = method.clone();
        exchange.request.uri = uri.clone();
        if self.0.capture {
            exchange.request.headers = headers.clone();
        }
        exchange
            .breakpoint
            .get_or_insert_with(BreakRecord::default)
            .request_edited = true;
    }

    /// The upstream connection is ready.
    pub(crate) fn connected(&self) {
        let at = self.elapsed_us();
        self.lock().exchange.timings.upstream_connected_us = Some(at);
    }

    /// Changes the exchange kind (e.g. to WebSocket once the upgrade succeeds).
    pub(crate) fn set_kind(&self, kind: ExchangeKind) {
        let mut state = self.lock();
        state.exchange.kind = kind;
        if matches!(
            kind,
            ExchangeKind::WebSocket | ExchangeKind::Sse | ExchangeKind::Upgrade
        ) && state.exchange.stream.is_none()
        {
            state.exchange.stream = Some(StreamStats::default());
        }
    }

    /// The response head is known.
    pub(crate) fn response_head(
        &self,
        status: StatusCode,
        version: Version,
        headers: &HeaderMap,
        responder: Responder,
    ) {
        let at = self.elapsed_us();
        self.0.metrics.record_latency(Duration::from_micros(at));
        let mut state = self.lock();
        let exchange = &mut state.exchange;
        exchange.timings.first_byte_us = Some(at);
        exchange.state = ExchangeState::Streaming;
        match &responder {
            Responder::Gate { .. } => {
                self.0.metrics.blocked.fetch_add(1, Relaxed);
            }
            Responder::Stub { .. } => {
                self.0.metrics.stubbed.fetch_add(1, Relaxed);
            }
            _ => {}
        }
        exchange.responder = responder;
        exchange.response = Some(ResponseRecord {
            status,
            version,
            headers: if self.0.capture {
                headers.clone()
            } else {
                HeaderMap::new()
            },
            body: BodyRecord::default(),
        });
        let snapshot = exchange.clone();
        drop(state);
        self.publish(Change::Updated, &snapshot);
    }

    /// A body finished (or was cut off, when `error` is set).
    pub(crate) fn body_end(&self, side: Side, body: BodyRecord, error: Option<ExchangeError>) {
        let at = self.elapsed_us();
        let mut state = self.lock();
        match side {
            Side::Request => {
                state.exchange.request.body = body;
                state.exchange.timings.request_done_us = Some(at);
                if state.finished {
                    // The response finished first (e.g. an early 413): a late update.
                    let snapshot = state.exchange.clone();
                    drop(state);
                    self.publish(Change::Updated, &snapshot);
                }
            }
            Side::Response => {
                if let Some(response) = state.exchange.response.as_mut() {
                    response.body = body;
                }
                drop(state);
                self.finish(error);
            }
        }
    }

    /// Marks the exchange with the fault rule applied to it.
    pub(crate) fn set_fault(&self, fault: crate::FaultRecord) {
        self.lock().exchange.fault = Some(fault);
    }

    /// Records an error without finishing (e.g. a request body error while the
    /// response still streams); the first error wins.
    pub(crate) fn note_error(&self, error: ExchangeError) {
        let mut state = self.lock();
        state.exchange.error.get_or_insert(error);
    }

    /// Updates stream statistics, publishing at most every 500 ms (or when a preview
    /// was added).
    pub(crate) fn stream_update(&self, update: impl FnOnce(&mut StreamStats) -> bool) {
        if !self.0.capture {
            return;
        }
        let mut state = self.lock();
        let stats = state
            .exchange
            .stream
            .get_or_insert_with(StreamStats::default);
        let important = update(stats);
        let now = Instant::now();
        let due = state
            .last_stream_publish
            .is_none_or(|last| now.duration_since(last) >= STREAM_UPDATE_INTERVAL);
        if important || due {
            state.last_stream_publish = Some(now);
            let snapshot = state.exchange.clone();
            drop(state);
            self.publish(Change::Updated, &snapshot);
        }
    }

    /// Finishes the exchange: complete, or failed with `error`. Idempotent.
    pub(crate) fn finish(&self, error: Option<ExchangeError>) {
        let at = self.elapsed_us();
        let mut state = self.lock();
        if state.finished {
            return;
        }
        state.finished = true;
        let exchange = &mut state.exchange;
        if let Some(error) = error {
            exchange.error.get_or_insert(error);
        }
        exchange.timings.complete_us = Some(at);
        if let Some(stream) = exchange.stream.as_mut() {
            stream.closed = true;
        }
        let failed = exchange.error.is_some();
        exchange.state = if failed {
            ExchangeState::Failed
        } else {
            ExchangeState::Complete
        };
        let metrics = &self.0.metrics;
        TapMetrics::dec(&metrics.active_requests);
        if let Some(response) = exchange.response.as_ref() {
            metrics.record_status(response.status.as_u16());
        }
        if failed {
            metrics.errors.fetch_add(1, Relaxed);
        }
        let snapshot = exchange.clone();
        drop(state);
        self.publish(Change::Completed, &snapshot);
    }

    /// Fails before any response head (upstream unreachable…).
    pub(crate) fn fail(&self, kind: ErrorKind, message: impl Into<String>) {
        self.finish(Some(ExchangeError {
            kind,
            message: message.into(),
        }));
    }
}
