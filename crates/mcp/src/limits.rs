//! Bounds: how often each kind of tool may run, how much one answer may hold, and how
//! lists are paged.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use crate::registry::ToolClass;

/// Longest text a tool answers with before it's cut (characters).
pub const MAX_TEXT: usize = 48_000;
/// Default page size for list tools.
pub const DEFAULT_PAGE: usize = 50;
/// Largest page a list tool returns.
pub const MAX_PAGE: usize = 200;

/// Calls allowed per class in one window.
fn budget(class: ToolClass) -> u32 {
    match class {
        ToolClass::Read => 240,
        ToolClass::Wait | ToolClass::Change => 30,
        ToolClass::Destructive => 12,
    }
}

const WINDOW: Duration = Duration::from_secs(60);

/// Calls per tool class in a sliding minute, shared by every session of a server.
#[derive(Debug, Default)]
pub struct RateLimiter {
    calls: Mutex<HashMap<ToolClass, Vec<Instant>>>,
}

impl RateLimiter {
    /// Counts a call of `class` now. `Err` says how long to wait when over budget.
    ///
    /// # Errors
    /// The class's budget for the last minute is used up.
    pub fn check(&self, class: ToolClass) -> Result<(), Duration> {
        self.check_at(class, Instant::now())
    }

    fn check_at(&self, class: ToolClass, now: Instant) -> Result<(), Duration> {
        let mut calls = self.calls.lock().unwrap_or_else(PoisonError::into_inner);
        let list = calls.entry(class).or_default();
        list.retain(|at| now.duration_since(*at) < WINDOW);
        if list.len() >= usize::try_from(budget(class)).unwrap_or(usize::MAX) {
            let oldest = list.first().copied().unwrap_or(now);
            return Err(WINDOW.saturating_sub(now.duration_since(oldest)));
        }
        list.push(now);
        Ok(())
    }
}

/// A page of `items` starting at `cursor` (an offset this module wrote), at most
/// `limit` long, with the cursor of the next page.
///
/// # Errors
/// A cursor this module didn't write.
pub fn page<T>(
    items: Vec<T>,
    cursor: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<T>, Option<String>, usize), String> {
    let start = match cursor.map(str::trim).filter(|c| !c.is_empty()) {
        None => 0,
        Some(cursor) => cursor
            .strip_prefix("o")
            .and_then(|n| n.parse::<usize>().ok())
            .ok_or_else(|| {
                format!("\"{cursor}\" isn't a cursor from this tool. Omit it to start over.")
            })?,
    };
    let limit = limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE);
    let total = items.len();
    let items: Vec<T> = items.into_iter().skip(start).take(limit).collect();
    let next = (start + items.len() < total).then(|| format!("o{}", start + items.len()));
    Ok((items, next, total))
}

/// `text` cut to [`MAX_TEXT`] characters, with a note saying so.
pub fn truncate(text: String) -> String {
    if text.chars().count() <= MAX_TEXT {
        return text;
    }
    let mut cut: String = text.chars().take(MAX_TEXT).collect();
    cut.push_str("\n… [cut: the answer was longer than this. Narrow the request (a filter, a smaller limit, or a cursor) to see the rest.]");
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_with_cursors() {
        let items: Vec<u32> = (0..120).collect();
        let (first, next, total) = page(items.clone(), None, None).unwrap();
        assert_eq!((first.len(), total), (50, 120));
        assert_eq!(next.as_deref(), Some("o50"));
        let (second, next, _) = page(items.clone(), next.as_deref(), Some(100)).unwrap();
        assert_eq!(second.first(), Some(&50));
        assert_eq!(second.len(), 70);
        assert_eq!(next, None);
        assert!(page(items.clone(), Some("nope"), None).is_err());
        let (capped, _, _) = page(items, None, Some(10_000)).unwrap();
        assert_eq!(capped.len(), MAX_PAGE.min(120));
    }

    #[test]
    fn limits_each_class_separately() {
        let limiter = RateLimiter::default();
        let now = Instant::now();
        for _ in 0..budget(ToolClass::Destructive) {
            limiter.check_at(ToolClass::Destructive, now).unwrap();
        }
        let wait = limiter.check_at(ToolClass::Destructive, now).unwrap_err();
        assert!(wait <= WINDOW && wait > Duration::ZERO);
        assert!(limiter.check_at(ToolClass::Read, now).is_ok());
        // A minute later the budget is back.
        assert!(
            limiter
                .check_at(ToolClass::Destructive, now + WINDOW)
                .is_ok()
        );
    }

    #[test]
    fn cuts_long_text_with_a_note() {
        assert_eq!(truncate("short".into()), "short");
        let long = "x".repeat(MAX_TEXT + 10);
        let cut = truncate(long);
        assert!(cut.contains("[cut:"));
        assert!(cut.chars().count() < MAX_TEXT + 200);
    }
}
