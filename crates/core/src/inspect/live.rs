//! Live updates for a viewer (the app's inspector window, `teitunnel traffic watch`):
//! exchange changes coalesced into batches at a bounded rate, so a burst of requests
//! never floods the webview.

use std::{collections::HashMap, time::Duration};

use lens::{ExchangeId, LensEvent, TapId};
use serde::Serialize;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;

use super::{ExchangeRow, InspectError, InspectEvent, Inspector};

/// How often batches go out at most.
pub const LIVE_INTERVAL: Duration = Duration::from_millis(100);
/// Exchanges per batch at most (the rest wait for the next one).
const MAX_BATCH: usize = 500;

/// What changed since the last batch.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct LiveBatch {
    /// New or changed exchanges (their latest state), oldest change first.
    pub exchanges: Vec<ExchangeRow>,
    /// Captures were cleared: for these taps, or all when `null` is in the list.
    pub cleared: Vec<Option<TapId>>,
    /// Taps started, stopped or changed.
    pub taps_changed: bool,
    /// Updates were missed (too many at once): read the list again.
    pub lagged: bool,
}

impl LiveBatch {
    fn is_empty(&self) -> bool {
        self.exchanges.is_empty() && self.cleared.is_empty() && !self.taps_changed && !self.lagged
    }
}

/// Sends batches to `send` until `stop`, or until `send` returns `false` (the viewer
/// went away).
///
/// # Errors
/// The inspector couldn't start.
pub async fn follow(
    inspector: &Inspector,
    stop: CancellationToken,
    mut send: impl FnMut(LiveBatch) -> bool + Send,
) -> Result<(), InspectError> {
    let mut exchanges = inspector.live()?;
    let mut taps = inspector.subscribe();
    let mut tick = tokio::time::interval(LIVE_INTERVAL);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut pending: Vec<ExchangeId> = Vec::new();
    let mut latest: HashMap<ExchangeId, ExchangeRow> = HashMap::new();
    let mut batch = LiveBatch::default();
    loop {
        tokio::select! {
            () = stop.cancelled() => return Ok(()),
            event = exchanges.recv() => match event {
                Ok(LensEvent::Exchange { exchange, .. }) => {
                    let row = ExchangeRow::of(&exchange);
                    if latest.insert(row.id, row).is_none() {
                        pending.push(exchange.id);
                    }
                }
                Ok(LensEvent::Cleared { tap }) => {
                    latest.retain(|_, row| tap.as_ref().is_some_and(|t| *t != row.tap));
                    pending.retain(|id| latest.contains_key(id));
                    batch.cleared.push(tap);
                }
                Err(broadcast::error::RecvError::Lagged(_)) => batch.lagged = true,
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            event = taps.recv() => match event {
                Ok(InspectEvent::Taps) | Err(broadcast::error::RecvError::Lagged(_)) => {
                    batch.taps_changed = true;
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Closed) => return Ok(()),
            },
            _ = tick.tick() => {
                let take = pending.len().min(MAX_BATCH);
                for id in pending.drain(..take) {
                    if let Some(row) = latest.remove(&id) {
                        batch.exchanges.push(row);
                    }
                }
                if !batch.is_empty() && !send(std::mem::take(&mut batch)) {
                    return Ok(());
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use super::*;
    use crate::inspect::{
        TapScope, TapSpec,
        tests::{origin, send},
    };

    #[tokio::test(flavor = "multi_thread")]
    async fn batches_changes_at_a_bounded_rate() {
        let origin = origin().await;
        let inspector = Inspector::new(None, None, "app");
        let batches: Arc<Mutex<Vec<LiveBatch>>> = Arc::default();
        let stop = CancellationToken::new();
        let follower = {
            let (inspector, batches, stop) = (inspector.clone(), batches.clone(), stop.clone());
            tokio::spawn(async move {
                follow(&inspector, stop, move |batch| {
                    batches.lock().unwrap().push(batch);
                    true
                })
                .await
            })
        };
        let tap = inspector
            .start(TapSpec::new(
                TapScope::QuickShare {
                    share_id: "qs-live".into(),
                },
                "demo",
                &origin,
            ))
            .await
            .unwrap();
        for i in 0..20 {
            send(&tap.address, "GET", &format!("/{i}"), &[]).await;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
        inspector.clear(None).await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        stop.cancel();
        follower.await.unwrap().unwrap();
        let batches = batches.lock().unwrap().clone();
        assert!(batches.iter().any(|b| b.taps_changed));
        let rows: Vec<&ExchangeRow> = batches.iter().flat_map(|b| &b.exchanges).collect();
        let finished = rows
            .iter()
            .filter(|r| r.state == lens::ExchangeState::Complete)
            .count();
        assert!(finished >= 1, "latest states arrive");
        assert!(batches.len() < 20, "coalesced, not one per event");
        assert!(batches.iter().any(|b| b.cleared == [None]));
        inspector.shutdown().await;
    }
}
