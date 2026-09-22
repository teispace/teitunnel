use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use cloudflared::LogEvent;

/// A bounded ring buffer of a connector's recent log events. Cheap to clone.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<VecDeque<Arc<LogEvent>>>>,
    capacity: usize,
}

impl LogBuffer {
    /// A buffer that keeps the newest `capacity` events.
    pub fn new(capacity: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(capacity.min(1024)))),
            capacity: capacity.max(1),
        }
    }

    /// Appends an event, dropping the oldest when full.
    pub fn push(&self, event: Arc<LogEvent>) {
        let mut events = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if events.len() == self.capacity {
            events.pop_front();
        }
        events.push_back(event);
    }

    /// The newest `limit` events, oldest first.
    pub fn tail(&self, limit: usize) -> Vec<Arc<LogEvent>> {
        let events = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let skip = events.len().saturating_sub(limit);
        events.iter().skip(skip).cloned().collect()
    }

    /// Number of buffered events.
    pub fn len(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// Whether the buffer is empty.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_events() {
        let buffer = LogBuffer::new(3);
        for i in 0..5 {
            buffer.push(Arc::new(cloudflared::parse_line(&format!("line {i}"))));
        }
        let tail: Vec<_> = buffer.tail(10).iter().map(|e| e.message.clone()).collect();
        assert_eq!(tail, ["line 2", "line 3", "line 4"]);
        assert_eq!(buffer.tail(1)[0].message, "line 4");
    }
}
