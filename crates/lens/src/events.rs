//! Live updates: every change to a captured exchange is published to subscribers.

use std::sync::Arc;

use tokio::sync::broadcast;

use crate::{CaptureStore, Exchange, TapId};

/// What happened to an exchange.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Change {
    /// A new request arrived.
    Added,
    /// It progressed (response head, stream messages, late request body).
    Updated,
    /// It finished (complete or failed).
    Completed,
}

/// A live update from Lens.
///
/// Delivered through a bounded broadcast channel: a subscriber that falls behind gets
/// `RecvError::Lagged` and should re-read the list (the store holds the truth).
#[derive(Debug, Clone)]
pub enum LensEvent {
    /// An exchange changed.
    Exchange {
        /// What changed.
        change: Change,
        /// The exchange after the change.
        exchange: Arc<Exchange>,
    },
    /// Captures were cleared (for one tap, or all when `None`).
    Cleared {
        /// The tap, or all taps.
        tap: Option<TapId>,
    },
}

/// Where recorded exchanges go: the store, then subscribers.
#[derive(Debug)]
pub(crate) struct Hub {
    pub(crate) store: Arc<dyn CaptureStore>,
    pub(crate) events: broadcast::Sender<LensEvent>,
}

impl Hub {
    pub(crate) fn new(store: Arc<dyn CaptureStore>, buffer: usize) -> Self {
        let (events, _) = broadcast::channel(buffer.max(16));
        Self { store, events }
    }

    pub(crate) fn publish(&self, change: Change, exchange: Exchange) {
        let exchange = Arc::new(exchange);
        self.store.put(Arc::clone(&exchange));
        // No subscribers is fine.
        let _ = self.events.send(LensEvent::Exchange { change, exchange });
    }
}
