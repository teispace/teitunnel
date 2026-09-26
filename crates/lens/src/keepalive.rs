//! SSE keep-alive: Cloudflare ends a response that sends nothing for 125 s (error 524;
//! only Enterprise zones can raise it). For `text/event-stream` responses, Lens writes an SSE
//! comment (`: keep-alive`) when nothing has flowed downstream for a while, and only
//! at an event boundary: never inside an event the origin is still writing.

use std::{
    future::Future as _,
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http_body::{Body, Frame, SizeHint};
use tokio::time::{Instant, Sleep};

use crate::{LensBody, body::BoxError};

/// Default idle time before a heartbeat.
pub const DEFAULT_SSE_KEEPALIVE: Duration = Duration::from_secs(25);

const HEARTBEAT: &[u8] = b": keep-alive\n\n";

/// Adds heartbeats to an event stream.
pub(crate) struct KeepAliveBody {
    inner: LensBody,
    idle: Duration,
    sleep: Pin<Box<Sleep>>,
    /// The last bytes forwarded (enough to see a blank line).
    tail: [u8; 4],
    tail_len: usize,
}

impl KeepAliveBody {
    pub(crate) fn new(inner: LensBody, idle: Duration) -> Self {
        Self {
            inner,
            idle,
            sleep: Box::pin(tokio::time::sleep(idle)),
            tail: [0; 4],
            tail_len: 0,
        }
    }

    fn remember(&mut self, data: &[u8]) {
        for &b in &data[data.len().saturating_sub(4)..] {
            if self.tail_len < 4 {
                self.tail[self.tail_len] = b;
                self.tail_len += 1;
            } else {
                self.tail.rotate_left(1);
                self.tail[3] = b;
            }
        }
    }

    /// Whether the stream is between events: nothing sent yet, or the last bytes end
    /// with a blank line.
    fn at_boundary(&self) -> bool {
        let tail = &self.tail[..self.tail_len];
        tail.is_empty()
            || tail.ends_with(b"\n\n")
            || tail.ends_with(b"\r\r")
            || tail.ends_with(b"\r\n\r\n")
    }

    fn rearm(&mut self) {
        self.sleep.as_mut().reset(Instant::now() + self.idle);
    }
}

impl Body for KeepAliveBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = &mut *self;
        match Pin::new(&mut this.inner).poll_frame(cx) {
            Poll::Ready(Some(Ok(frame))) => {
                if let Some(data) = frame.data_ref() {
                    if !data.is_empty() {
                        this.remember(data);
                    }
                    this.rearm();
                }
                return Poll::Ready(Some(Ok(frame)));
            }
            Poll::Ready(other) => return Poll::Ready(other),
            Poll::Pending => {}
        }
        loop {
            if this.sleep.as_mut().poll(cx).is_pending() {
                return Poll::Pending;
            }
            this.rearm();
            if this.at_boundary() {
                this.remember(HEARTBEAT);
                return Poll::Ready(Some(Ok(Frame::data(Bytes::from_static(HEARTBEAT)))));
            }
            // Mid-event: wait for the origin to finish it (the timer is re-armed and
            // polled again, so this task wakes when it fires).
        }
    }

    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

#[cfg(test)]
mod tests {
    use http_body_util::{BodyExt, StreamBody};
    use tokio::sync::mpsc;

    use super::*;

    fn channel() -> (mpsc::Sender<&'static [u8]>, LensBody) {
        let (tx, rx) = mpsc::channel::<&'static [u8]>(8);
        let stream = futures_util::stream::unfold(rx, |mut rx| async move {
            rx.recv().await.map(|chunk| {
                (
                    Ok::<_, BoxError>(Frame::data(Bytes::from_static(chunk))),
                    rx,
                )
            })
        });
        (tx, StreamBody::new(stream).boxed_unsync())
    }

    async fn next(body: &mut KeepAliveBody) -> Bytes {
        body.frame().await.unwrap().unwrap().into_data().unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn heartbeats_only_when_idle_at_a_boundary() {
        let (tx, inner) = channel();
        let mut body = KeepAliveBody::new(inner, Duration::from_secs(25));

        // Idle from the start: a heartbeat after 25 s.
        let start = Instant::now();
        assert_eq!(&next(&mut body).await[..], HEARTBEAT);
        assert_eq!(start.elapsed(), Duration::from_secs(25));

        tx.send(b"data: one\n\n").await.unwrap();
        assert_eq!(&next(&mut body).await[..], b"data: one\n\n");

        // A partial event: no heartbeat until it ends, however long it takes.
        tx.send(b"data: tw").await.unwrap();
        assert_eq!(&next(&mut body).await[..], b"data: tw");
        let sender = tx.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(80)).await;
            sender.send(b"o\n\n").await.unwrap();
        });
        let before = Instant::now();
        assert_eq!(&next(&mut body).await[..], b"o\n\n");
        assert_eq!(before.elapsed(), Duration::from_secs(80));

        // Idle again at a boundary: heartbeats resume.
        assert_eq!(&next(&mut body).await[..], HEARTBEAT);
        assert_eq!(&next(&mut body).await[..], HEARTBEAT);
        drop(tx);
        assert!(body.frame().await.is_none());
    }

    #[tokio::test(start_paused = true)]
    async fn traffic_postpones_heartbeats() {
        let (tx, inner) = channel();
        let mut body = KeepAliveBody::new(inner, Duration::from_secs(25));
        tokio::spawn(async move {
            for _ in 0..3 {
                tokio::time::sleep(Duration::from_secs(20)).await;
                tx.send(b"data: tick\r\n\r\n").await.unwrap();
            }
        });
        for _ in 0..3 {
            assert_eq!(&next(&mut body).await[..], b"data: tick\r\n\r\n");
        }
        // The origin finished without going idle for 25 s: no heartbeat was needed.
        assert!(body.frame().await.is_none());
    }

    #[tokio::test]
    async fn boundary_detection() {
        let (_tx, inner) = channel();
        let mut body = KeepAliveBody::new(inner, Duration::from_secs(1));
        assert!(body.at_boundary());
        body.remember(b"data: x\n");
        assert!(!body.at_boundary());
        body.remember(b"\n");
        assert!(body.at_boundary());
        body.remember(b"data: y\r\n\r");
        assert!(!body.at_boundary());
        body.remember(b"\n");
        assert!(body.at_boundary());
    }
}
