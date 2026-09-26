//! When to tell the user a connector went down or came back. Pure and time-driven, so
//! the policy is testable: brief blips (a reconnect, a restart) never notify, reconnecting
//! after a wake or a network change gets longer ([`crate::runtime::network::SETTLE`]), and
//! each outage produces at most one "down" and one "back".

use std::{collections::HashMap, time::Duration};

use crate::runtime::ConnectorState;

/// How long a connector must be unhealthy before it's reported down.
pub const GRACE: Duration = Duration::from_secs(20);

/// Something worth a notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// Routes through this connector stopped working.
    Down,
    /// They work again.
    Back,
    /// The connector keeps crashing and won't be restarted automatically.
    CrashLoop,
}

#[derive(Debug, Clone, Copy, Default)]
struct Track {
    /// When it became unhealthy (ms), if it is.
    unhealthy_since: Option<u64>,
    /// A "down" was sent and no "back" yet.
    reported: bool,
    /// A crash loop was reported.
    looped: bool,
}

/// Tracks connectors across observations.
#[derive(Debug, Default)]
pub struct HealthWatch {
    tracks: HashMap<String, Track>,
    /// Nothing is reported down before this (ms): connectors are reconnecting.
    settle_until: u64,
}

fn healthy(state: Option<&ConnectorState>) -> bool {
    matches!(state, Some(ConnectorState::Healthy { .. }))
}

impl HealthWatch {
    /// Records `state` for `tunnel` at `now_ms` (None: not running at all) and returns what
    /// to tell the user, if anything.
    pub fn observe(
        &mut self,
        tunnel: &str,
        state: Option<&ConnectorState>,
        now_ms: u64,
    ) -> Option<Notice> {
        let track = self.tracks.entry(tunnel.to_owned()).or_default();
        if healthy(state) {
            let was_reported = track.reported;
            *track = Track::default();
            return was_reported.then_some(Notice::Back);
        }
        let since = *track.unhealthy_since.get_or_insert(now_ms);
        // Waking up or changing networks: reconnecting takes a moment, and an outage that
        // outlasts it is still reported once it has.
        if now_ms < self.settle_until {
            return None;
        }
        if matches!(state, Some(ConnectorState::CrashLoop { .. })) && !track.looped {
            track.looped = true;
            track.reported = true;
            return Some(Notice::CrashLoop);
        }
        let grace = u64::try_from(GRACE.as_millis()).unwrap_or(u64::MAX);
        if !track.reported && now_ms.saturating_sub(since) >= grace {
            track.reported = true;
            return Some(Notice::Down);
        }
        None
    }

    /// The computer woke or changed networks at `at_ms`: give connectors
    /// [`SETTLE`](crate::runtime::network::SETTLE) to reconnect before reporting any down.
    pub fn disrupted(&mut self, at_ms: u64) {
        let settle = u64::try_from(crate::runtime::network::SETTLE.as_millis()).unwrap_or(0);
        self.settle_until = self.settle_until.max(at_ms.saturating_add(settle));
    }

    /// Forgets a tunnel (deleted, or stopped on purpose).
    pub fn forget(&mut self, tunnel: &str) {
        self.tracks.remove(tunnel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UP: ConnectorState = ConnectorState::Healthy { connections: 4 };

    #[test]
    fn blips_are_quiet_and_outages_report_once() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe("t", Some(&UP), 0), None);
        // A 10 s reconnect: nothing.
        assert_eq!(
            watch.observe("t", Some(&ConnectorState::Connecting), 1_000),
            None
        );
        assert_eq!(watch.observe("t", Some(&UP), 11_000), None);
        // A real outage: one "down" after the grace period, then silence, then "back".
        assert_eq!(
            watch.observe("t", Some(&ConnectorState::Degraded), 20_000),
            None
        );
        assert_eq!(watch.observe("t", None, 41_000), Some(Notice::Down));
        assert_eq!(watch.observe("t", None, 60_000), None);
        assert_eq!(watch.observe("t", Some(&UP), 70_000), Some(Notice::Back));
        assert_eq!(watch.observe("t", Some(&UP), 80_000), None);
    }

    #[test]
    fn crash_loops_report_immediately_once() {
        let mut watch = HealthWatch::default();
        let looping = ConnectorState::CrashLoop { exit_code: Some(1) };
        assert_eq!(
            watch.observe("t", Some(&looping), 0),
            Some(Notice::CrashLoop)
        );
        assert_eq!(watch.observe("t", Some(&looping), 60_000), None);
        assert_eq!(watch.observe("t", Some(&UP), 70_000), Some(Notice::Back));
        watch.forget("t");
        assert_eq!(watch.observe("t", None, 0), None);
    }

    #[test]
    fn reconnecting_after_a_wake_is_quiet_unless_it_lasts() {
        let mut watch = HealthWatch::default();
        assert_eq!(watch.observe("t", Some(&UP), 0), None);
        assert_eq!(watch.observe("u", Some(&UP), 0), None);
        watch.disrupted(1_000);
        let looping = ConnectorState::CrashLoop { exit_code: Some(1) };
        assert_eq!(
            watch.observe("t", Some(&ConnectorState::Degraded), 1_000),
            None
        );
        assert_eq!(
            watch.observe("u", Some(&ConnectorState::Degraded), 1_000),
            None
        );
        assert_eq!(
            watch.observe("t", Some(&looping), 60_000),
            None,
            "offline for a bit"
        );
        assert_eq!(
            watch.observe("u", Some(&UP), 70_000),
            None,
            "back in time: silence"
        );
        assert_eq!(watch.observe("t", None, 85_000), None);
        // Still down once the settling time is over: reported.
        assert_eq!(watch.observe("t", None, 91_000), Some(Notice::Down));
        assert_eq!(watch.observe("u", Some(&UP), 91_000), None);
    }
}
