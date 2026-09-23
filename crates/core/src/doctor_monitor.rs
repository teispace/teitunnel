//! When to tell the user the Doctor found a new problem. Pure and time-driven, like
//! `health`: the app records every Doctor run here (whether the window or the background
//! schedule asked for it) and gets back at most one coalesced notice per run.
//!
//! Only errors notify, only once each until they're resolved, never ignored ones, and
//! never connector outages (those have their own notices, `health`) or network blips.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use crate::doctor::{Issue, Severity};
use crate::text::{Text, msg};

/// How often the Doctor runs in the background (and how fresh a window-triggered run
/// must be to skip one).
pub const INTERVAL: Duration = Duration::from_secs(5 * 60);

/// Checks that never notify: connector outages have their own notices (`health`), and
/// an unreachable account is usually the network blipping (the window still shows it).
const QUIET_CHECKS: &[&str] = &[
    "tunnel.crash_loop",
    "tunnel.no_connections",
    "tunnel.degraded",
    "account.unreachable",
];

/// A notification to post.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorNotice {
    /// Title.
    pub title: Text,
    /// Body.
    pub body: Text,
}

#[derive(Debug, Default)]
struct Inner {
    last_run: Option<Instant>,
    /// Errors present in the last run (a resolved one notifies again if it returns).
    known: HashSet<String>,
}

/// Tracks Doctor runs across the app. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct DoctorMonitor {
    inner: Arc<Mutex<Inner>>,
}

impl DoctorMonitor {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether a background run is due at `now` (nothing ran in the last [`INTERVAL`]).
    pub fn due(&self, now: Instant) -> bool {
        self.lock()
            .last_run
            .is_none_or(|last| now.saturating_duration_since(last) >= INTERVAL)
    }

    /// Records a run's issues at `now` and returns what to tell the user, if anything.
    pub fn record(
        &self,
        issues: &[Issue],
        ignored: &HashSet<String>,
        now: Instant,
    ) -> Option<DoctorNotice> {
        let errors: Vec<&Issue> = issues
            .iter()
            .filter(|i| i.severity == Severity::Error)
            .filter(|i| !ignored.contains(&i.id))
            .filter(|i| !QUIET_CHECKS.contains(&i.check.as_str()))
            .collect();
        let mut inner = self.lock();
        inner.last_run = Some(now);
        let new: Vec<&Issue> = errors
            .iter()
            .copied()
            .filter(|i| !inner.known.contains(&i.id))
            .collect();
        inner.known = errors.iter().map(|i| i.id.clone()).collect();
        match new.as_slice() {
            [] => None,
            [one] => Some(DoctorNotice {
                title: one.title.clone(),
                body: one.detail.clone(),
            }),
            [first, rest @ ..] => Some(DoctorNotice {
                title: msg::notify::doctor_many((rest.len() + 1) as u64),
                // The first title is rendered into the body in the user's language.
                body: msg::notify::doctor_many_body(rest.len() as u64, &first.title),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(id: &str, check: &str, severity: Severity) -> Issue {
        Issue {
            id: id.into(),
            check: check.into(),
            severity,
            account_id: None,
            subject: id.into(),
            label: msg::raw(id),
            title: msg::raw(format!("{id} is broken")),
            detail: msg::raw(format!("Fix {id}.")),
            evidence: Vec::new(),
            fixes: Vec::new(),
        }
    }

    #[test]
    fn notifies_new_errors_once_coalesced() {
        let monitor = DoctorMonitor::default();
        let t0 = Instant::now();
        assert!(monitor.due(t0));
        let a = issue("a", "dns.missing", Severity::Error);
        let b = issue("b", "dns.wrong_target", Severity::Error);
        let warning = issue("w", "dns.not_proxied", Severity::Warning);
        let none = HashSet::new();

        let notice = monitor
            .record(&[a.clone(), warning.clone()], &none, t0)
            .unwrap();
        assert_eq!(notice.title.english(), "a is broken");
        assert_eq!(notice.body.english(), "Fix a.");
        assert!(!monitor.due(t0 + Duration::from_secs(60)));
        assert!(monitor.due(t0 + INTERVAL));

        // Still there: quiet. A second one appears: only it is announced.
        assert_eq!(monitor.record(std::slice::from_ref(&a), &none, t0), None);
        let both = [
            a.clone(),
            b.clone(),
            issue("c", "dns.conflict", Severity::Error),
        ];
        let notice = monitor.record(&both, &none, t0).unwrap();
        assert_eq!(notice.title.english(), "Teitunnel found 2 problems");
        assert_eq!(
            notice.body.english(),
            "b is broken, and 1 more. Open the Doctor to fix them."
        );

        // Resolved, then back: announced again.
        assert_eq!(monitor.record(&[], &none, t0), None);
        assert!(monitor.record(&[a], &none, t0).is_some());
    }

    #[test]
    fn skips_ignored_issues_and_connector_outages() {
        let monitor = DoctorMonitor::default();
        let ignored = HashSet::from(["a".to_owned()]);
        let issues = [
            issue("a", "dns.missing", Severity::Error),
            issue("t", "tunnel.crash_loop", Severity::Error),
        ];
        assert_eq!(monitor.record(&issues, &ignored, Instant::now()), None);
    }
}
