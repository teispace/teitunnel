//! `requestArrived` for control subscribers (editor extensions, launchers, `teitunnel
//! top`), from this process's inspector.
//!
//! One event per finished request (so it carries the status and duration), masked like
//! the inspector's list, never a replay. Bounded and coalesced: events go out at most
//! every [`REQUEST_TICK`], at most [`MAX_REQUESTS_PER_TICK`] at a time (the newest; the
//! rest are dropped), and identical requests within a tick are sent once. Nothing is
//! collected while no client is subscribed, and Lens is never started just for this.

use std::{collections::VecDeque, sync::Arc, time::Duration};

use teitunnel_control::protocol::Event;
use tokio::sync::broadcast;

use super::CoreHost;
use crate::inspect::{
    ExchangeRow, Inspector, TapScope,
    lens::{Change, Exchange, LensEvent},
};

/// How often events go out at most.
pub const REQUEST_TICK: Duration = Duration::from_millis(250);
/// Events per tick at most (40 a second).
pub const MAX_REQUESTS_PER_TICK: usize = 10;

/// The event for a finished exchange, if control clients should hear about it.
pub fn request_event(inspector: &Inspector, exchange: &Exchange) -> Option<Event> {
    if exchange.replay_of.is_some() {
        return None;
    }
    let share = match inspector.tap_scope(&exchange.tap)? {
        TapScope::QuickShare { share_id } => share_id,
        TapScope::Route { hostname, .. } => hostname,
    };
    let row = ExchangeRow::of(exchange);
    Some(Event::RequestArrived {
        share,
        method: row.method,
        path: row.path,
        status: row.status,
        duration_ms: row.duration_ms.map(|ms| {
            // Durations are non-negative and far below u64::MAX milliseconds.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let ms = ms.max(0.0).round() as u64;
            ms
        }),
    })
}

/// Events waiting for the next tick.
#[derive(Debug, Default)]
pub(crate) struct Coalescer {
    pending: VecDeque<Event>,
    dropped: u64,
}

impl Coalescer {
    /// Queues an event: a repeat of one already waiting is merged into it, and the
    /// oldest is dropped when the queue is full.
    pub(crate) fn push(&mut self, event: Event) {
        if self.pending.contains(&event) {
            return;
        }
        if self.pending.len() == MAX_REQUESTS_PER_TICK {
            self.pending.pop_front();
            self.dropped += 1;
        }
        self.pending.push_back(event);
    }

    /// Everything waiting, oldest first.
    pub(crate) fn drain(&mut self) -> Vec<Event> {
        self.pending.drain(..).collect()
    }

    /// Events dropped so far (for logs and tests).
    pub(crate) fn dropped(&self) -> u64 {
        self.dropped
    }
}

impl CoreHost {
    /// Publishes `requestArrived` for the inspector's requests until the inspector shuts
    /// down. Waits for Lens to start (the first inspected share or route) instead of
    /// starting it.
    pub async fn forward_requests(self: Arc<Self>, inspector: Inspector) {
        let mut taps = inspector.subscribe();
        let lens = loop {
            if let Some(lens) = inspector.running() {
                break lens;
            }
            match taps.recv().await {
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => return,
            }
        };
        drop(taps);
        let mut events = lens.subscribe();
        let mut queue = Coalescer::default();
        let mut deadline: Option<tokio::time::Instant> = None;
        let mut reported = 0;
        loop {
            tokio::select! {
                event = events.recv() => match event {
                    Ok(LensEvent::Exchange { change: Change::Completed, exchange }) => {
                        if self.events.receiver_count() == 0 {
                            continue;
                        }
                        if let Some(event) = request_event(&inspector, &exchange) {
                            queue.push(event);
                            deadline.get_or_insert_with(|| {
                                tokio::time::Instant::now() + REQUEST_TICK
                            });
                        }
                    }
                    Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                },
                () = async {
                    match deadline {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                }, if deadline.is_some() => {
                    deadline = None;
                    for event in queue.drain() {
                        self.publish(event);
                    }
                    if queue.dropped() > reported {
                        reported = queue.dropped();
                        tracing::debug!(dropped = reported, "requestArrived events dropped in bursts");
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str) -> Event {
        Event::RequestArrived {
            share: "qs-1".into(),
            method: "GET".into(),
            path: path.into(),
            status: Some(200),
            duration_ms: Some(3),
        }
    }

    #[test]
    fn merges_repeats_and_keeps_the_newest() {
        let mut queue = Coalescer::default();
        queue.push(request("/a"));
        queue.push(request("/a"));
        assert_eq!(queue.drain(), [request("/a")]);
        assert!(queue.drain().is_empty());
        for i in 0..(MAX_REQUESTS_PER_TICK + 5) {
            queue.push(request(&format!("/{i}")));
        }
        let sent = queue.drain();
        assert_eq!(sent.len(), MAX_REQUESTS_PER_TICK);
        assert_eq!(sent[0], request("/5"), "the oldest were dropped");
        assert_eq!(queue.dropped(), 5);
    }
}
