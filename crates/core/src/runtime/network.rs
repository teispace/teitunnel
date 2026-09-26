//! Sleep, wake and network changes, seen from inside the process: the wall clock jumping
//! ahead of the tick (the computer slept) and this computer's addresses changing (a new
//! Wi-Fi, a VPN, a cable). Connectors are nudged on either, so they reconnect in seconds
//! instead of waiting out a backoff or a stale connection.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    net::IpAddr,
    time::{Duration, SystemTime},
};

/// How often the clock and the addresses are looked at.
pub const TICK: Duration = Duration::from_secs(5);
/// A tick this much later than due means the computer slept (or the process was frozen).
const WAKE_GAP: Duration = Duration::from_secs(20);
/// How long after a disruption a connector may take to come back before anyone is told
/// it's down: reconnecting after a wake or a network change takes a little while.
pub const SETTLE: Duration = Duration::from_secs(90);

/// Something that makes existing connections stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disruption {
    /// The computer woke from sleep.
    Woke,
    /// The network came back after there was none.
    Online,
    /// The addresses changed (another network, a VPN on or off).
    NetworkChanged,
}

/// Turns ticks into [`Disruption`]s. Pure, so it's testable without sleeping.
#[derive(Debug, Clone)]
pub struct Observer {
    last_tick: SystemTime,
    /// Fingerprint of the routable addresses (`None`: offline); unknown before the first
    /// look.
    network: Option<Option<u64>>,
}

impl Observer {
    /// Starts observing at `now`.
    pub fn new(now: SystemTime) -> Self {
        Self {
            last_tick: now,
            network: None,
        }
    }

    /// Records a tick at `now` with this computer's `addresses`, returning what happened
    /// since the last one, if anything worth reconnecting for.
    pub fn observe(&mut self, now: SystemTime, addresses: &[IpAddr]) -> Option<Disruption> {
        let woke = now
            .duration_since(self.last_tick)
            .is_ok_and(|gap| gap > TICK + WAKE_GAP);
        self.last_tick = now;
        let current = fingerprint(addresses);
        let previous = self.network.replace(current);
        if woke {
            return Some(Disruption::Woke);
        }
        match (previous, current) {
            // The first look, or nothing changed, or it went offline (nothing to do yet).
            (None, _) | (_, None) => None,
            (Some(before), Some(now)) if before == Some(now) => None,
            (Some(None), Some(_)) => Some(Disruption::Online),
            (Some(Some(_)), Some(_)) => Some(Disruption::NetworkChanged),
        }
    }
}

/// Whether an address can reach the internet at all (not loopback or link-local).
fn routable(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => !v4.is_loopback() && !v4.is_link_local() && !v4.is_unspecified(),
        IpAddr::V6(v6) => {
            !v6.is_loopback() && !v6.is_unspecified() && (v6.segments()[0] & 0xffc0) != 0xfe80
        }
    }
}

/// A fingerprint of the routable addresses, or `None` when there are none (offline).
fn fingerprint(addresses: &[IpAddr]) -> Option<u64> {
    let mut routable: Vec<IpAddr> = addresses
        .iter()
        .copied()
        .filter(|ip| routable(*ip))
        .collect();
    if routable.is_empty() {
        return None;
    }
    routable.sort_unstable();
    routable.dedup();
    let mut hasher = DefaultHasher::new();
    routable.hash(&mut hasher);
    Some(hasher.finish())
}

/// This computer's interface addresses.
pub(crate) fn addresses() -> Vec<IpAddr> {
    if_addrs::get_if_addrs()
        .map(|interfaces| interfaces.into_iter().map(|i| i.ip()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::net::{Ipv4Addr, Ipv6Addr};

    use super::*;

    const LOOPBACK: IpAddr = IpAddr::V4(Ipv4Addr::LOCALHOST);
    const HOME: IpAddr = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20));
    const CAFE: IpAddr = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 7));
    const LINK_LOCAL: IpAddr = IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1));

    #[test]
    fn sees_sleep_and_network_changes() {
        let start = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
        let mut observer = Observer::new(start);
        let at = |secs| start + Duration::from_secs(secs);
        assert_eq!(
            observer.observe(at(5), &[LOOPBACK, HOME]),
            None,
            "first look"
        );
        assert_eq!(
            observer.observe(at(10), &[HOME, LOOPBACK]),
            None,
            "same network"
        );
        assert_eq!(
            observer.observe(at(15), &[HOME, LINK_LOCAL]),
            None,
            "link-local addresses come and go"
        );
        assert_eq!(
            observer.observe(at(20), &[LOOPBACK, CAFE]),
            Some(Disruption::NetworkChanged)
        );
        assert_eq!(observer.observe(at(25), &[LOOPBACK]), None, "went offline");
        assert_eq!(observer.observe(at(30), &[LOOPBACK]), None);
        assert_eq!(
            observer.observe(at(35), &[LOOPBACK, CAFE]),
            Some(Disruption::Online)
        );
        // Asleep for an hour, back on the same network.
        assert_eq!(
            observer.observe(at(3_635), &[LOOPBACK, CAFE]),
            Some(Disruption::Woke)
        );
        assert_eq!(observer.observe(at(3_640), &[LOOPBACK, CAFE]), None);
        // A late tick that isn't sleep (a busy machine) is nothing.
        assert_eq!(observer.observe(at(3_660), &[LOOPBACK, CAFE]), None);
    }
}
