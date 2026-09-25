//! Body types and the capturing tap that streams a body through while copying its first
//! bytes.

use std::{
    pin::Pin,
    sync::atomic::Ordering::Relaxed,
    task::{Context, Poll},
};

use bytes::{Bytes, BytesMut};
use http_body::{Body, Frame, SizeHint};
use http_body_util::{BodyExt, Empty, Full, combinators::UnsyncBoxBody};

use crate::{
    BodyRecord, Direction, ExchangeError,
    capture::ErrorKind,
    recorder::{Recorder, Side},
    stream::{PreviewLimits, SseObserver},
};

/// Errors carried by body streams.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// The body type of every response Lens produces (and of requests it sends upstream).
pub type LensBody = UnsyncBoxBody<Bytes, BoxError>;

/// A complete body from bytes.
pub fn full(data: impl Into<Bytes>) -> LensBody {
    Full::new(data.into())
        .map_err(|never| match never {})
        .boxed_unsync()
}

/// An empty body.
pub fn empty() -> LensBody {
    Empty::new().map_err(|never| match never {}).boxed_unsync()
}

/// Streams `inner` through unchanged, copying the first `cap` bytes for the capture
/// and counting the rest. Reports to the recorder when the body ends, fails, or is
/// dropped early.
pub(crate) struct TeeBody<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    inner: B,
    side: Side,
    recorder: Recorder,
    cap: usize,
    captured: BytesMut,
    total: u64,
    finished: bool,
    sse: Option<(SseObserver, PreviewLimits)>,
}

impl<B> TeeBody<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    pub(crate) fn new(inner: B, side: Side, recorder: Recorder, cap: usize) -> Self {
        let cap = if recorder.capturing() { cap } else { 0 };
        Self {
            inner,
            side,
            recorder,
            cap,
            captured: BytesMut::new(),
            total: 0,
            finished: false,
            sse: None,
        }
    }

    /// Also counts server-sent events in this (response) body.
    pub(crate) fn with_sse(mut self, limits: PreviewLimits) -> Self {
        self.sse = Some((SseObserver::new(limits.bytes), limits));
        self
    }

    fn record(&self, complete: bool) -> BodyRecord {
        let data = self.captured.clone().freeze();
        BodyRecord {
            truncated: (data.len() as u64) < self.total,
            data,
            size: self.total,
            complete,
        }
    }

    fn finish(&mut self, error: Option<ExchangeError>) {
        if self.finished {
            return;
        }
        self.finished = true;
        let complete = error.is_none();
        let body = self.record(complete);
        match self.side {
            Side::Request => {
                if let Some(error) = error.clone() {
                    self.recorder.note_error(error);
                }
                self.recorder.body_end(Side::Request, body, None);
            }
            Side::Response => self.recorder.body_end(Side::Response, body, error),
        }
    }

    fn on_data(&mut self, data: &Bytes) {
        self.total += data.len() as u64;
        let counter = match self.side {
            Side::Request => &self.recorder.metrics().bytes_in,
            Side::Response => &self.recorder.metrics().bytes_out,
        };
        counter.fetch_add(data.len() as u64, Relaxed);
        let room = self.cap.saturating_sub(self.captured.len());
        if room > 0 {
            self.captured
                .extend_from_slice(&data[..room.min(data.len())]);
        }
        if let Some((observer, limits)) = self.sse.as_mut() {
            let mut messages = Vec::new();
            observer.feed(data, &mut messages);
            if !messages.is_empty() {
                let limits = *limits;
                let at = self.recorder.elapsed_us();
                self.recorder.stream_update(|stats| {
                    let mut previewed = false;
                    for message in messages {
                        previewed |= message.record(stats, Direction::ServerToClient, at, limits);
                    }
                    previewed
                });
            }
        }
    }
}

impl<B> Body for TeeBody<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => {
                this.finish(None);
                Poll::Ready(None)
            }
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    this.on_data(data);
                }
                if this.inner.is_end_stream() {
                    this.finish(None);
                }
                Poll::Ready(Some(Ok(frame)))
            }
            Poll::Ready(Some(Err(err))) => {
                let err: BoxError = err.into();
                let kind = match this.side {
                    Side::Request => ErrorKind::ClientAborted,
                    Side::Response => ErrorKind::ConnectionReset,
                };
                this.finish(Some(ExchangeError {
                    kind,
                    message: err.to_string(),
                }));
                Poll::Ready(Some(Err(err)))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

impl<B> Drop for TeeBody<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        if self.inner.is_end_stream() {
            // Empty or fully sent bodies hyper never polled to the end.
            self.finish(None);
            return;
        }
        self.finished = true;
        let body = self.record(false);
        match self.side {
            // An unread request body (the upstream answered early) isn't a failure of
            // the exchange; record what arrived.
            Side::Request => self.recorder.body_end(Side::Request, body, None),
            Side::Response => self.recorder.body_end(
                Side::Response,
                body,
                Some(ExchangeError {
                    kind: ErrorKind::ClientAborted,
                    message: "the client went away before the response finished".into(),
                }),
            ),
        }
    }
}

/// Reads a body into memory, up to `limit` bytes (for login forms and stubs).
///
/// Returns the bytes read and whether the body ended within the limit.
pub(crate) async fn read_limited<B>(body: B, limit: usize) -> Result<(Bytes, bool), BoxError>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    let mut body = body;
    let mut out = BytesMut::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(Into::into)?;
        if let Some(data) = frame.data_ref() {
            let room = limit.saturating_sub(out.len());
            out.extend_from_slice(&data[..room.min(data.len())]);
            if data.len() > room {
                return Ok((out.freeze(), false));
            }
        }
    }
    Ok((out.freeze(), true))
}

/// Reads frames from `body` until more than `limit` bytes arrived or it ended, leaving
/// the rest in `body`. Returns the bytes read and whether the body ended.
pub(crate) async fn read_prefix<B>(body: &mut B, limit: usize) -> Result<(Bytes, bool), BoxError>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    let mut out = BytesMut::new();
    while let Some(frame) = body.frame().await {
        let frame = frame.map_err(Into::into)?;
        if let Some(data) = frame.data_ref() {
            out.extend_from_slice(data);
            if out.len() > limit {
                return Ok((out.freeze(), false));
            }
        }
    }
    Ok((out.freeze(), true))
}

/// Replays already-read bytes, then the rest of a body.
pub(crate) struct PrefixedBody<B> {
    prefix: Option<Bytes>,
    inner: B,
}

impl<B> PrefixedBody<B> {
    pub(crate) fn new(prefix: Bytes, inner: B) -> Self {
        Self {
            prefix: (!prefix.is_empty()).then_some(prefix),
            inner,
        }
    }
}

impl<B> Body for PrefixedBody<B>
where
    B: Body<Data = Bytes> + Unpin,
    B::Error: Into<BoxError>,
{
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        if let Some(prefix) = self.prefix.take() {
            return Poll::Ready(Some(Ok(Frame::data(prefix))));
        }
        Pin::new(&mut self.inner)
            .poll_frame(cx)
            .map(|frame| frame.map(|result| result.map_err(Into::into)))
    }

    fn is_end_stream(&self) -> bool {
        self.prefix.is_none() && self.inner.is_end_stream()
    }
}
