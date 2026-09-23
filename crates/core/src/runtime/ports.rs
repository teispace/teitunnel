use std::{
    collections::HashSet,
    net::{Ipv4Addr, SocketAddrV4, TcpListener},
    ops::Range,
    sync::{Arc, Mutex},
};

/// Stable metrics ports for tunnel connectors (ARCHITECTURE §5.1). They avoid
/// cloudflared's defaults (20241–20245), so adopted foreign processes don't collide.
pub const TUNNEL_PORTS: Range<u16> = 20300..20400;

/// Ephemeral metrics ports for Quick Shares.
pub const QUICK_SHARE_PORTS: Range<u16> = 20400..20500;

/// Hands out free loopback ports from a range. A port is "free" if we haven't handed
/// it out and it can be bound right now.
#[derive(Debug, Clone)]
pub struct PortAllocator {
    range: Range<u16>,
    /// Where scanning starts, relative to the range's start.
    offset: u16,
    taken: Arc<Mutex<HashSet<u16>>>,
}

impl PortAllocator {
    /// An allocator over `range`.
    pub fn new(range: Range<u16>) -> Self {
        Self {
            range,
            offset: 0,
            taken: Arc::default(),
        }
    }

    /// Starts scanning at an offset derived from `seed` (e.g. the process id). Separate
    /// processes allocating from the same range at the same moment (the app and a CLI
    /// share) then rarely try the same port first, which would fail for one of them.
    #[must_use]
    pub fn spread(mut self, seed: u32) -> Self {
        let len = u32::from(self.range.len().try_into().unwrap_or(u16::MAX)).max(1);
        self.offset = u16::try_from(seed % len).unwrap_or(0);
        self
    }

    /// Reserves a free port, or `None` if the whole range is busy.
    pub fn allocate(&self) -> Option<u16> {
        let mut taken = self
            .taken
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let len = self.range.end.saturating_sub(self.range.start);
        let port = (0..len)
            .map(|i| self.range.start + (self.offset + i) % len)
            .filter(|port| !taken.contains(port))
            .find(|port| is_free(*port))?;
        taken.insert(port);
        Some(port)
    }

    /// Reserves `port` if it's in range, not handed out, and bindable right now.
    pub fn claim(&self, port: u16) -> bool {
        let mut taken = self
            .taken
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.range.contains(&port) && !taken.contains(&port) && is_free(port) {
            taken.insert(port);
            true
        } else {
            false
        }
    }

    /// Returns a port to the pool.
    pub fn release(&self, port: u16) {
        self.taken
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&port);
    }
}

/// Whether `127.0.0.1:port` can be bound right now.
pub(crate) fn is_free(port: u16) -> bool {
    TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Each test has its own block, away from the ranges a running Teitunnel uses
    // (20300–20500), so tests don't collide with the app or with each other.

    #[test]
    fn skips_taken_and_busy_ports() {
        let allocator = PortAllocator::new(24000..24010);
        let first = allocator.allocate().unwrap();
        let _busy = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, first + 1));
        let second = allocator.allocate().unwrap();
        assert_ne!(first, second);
        assert!((24000..24010).contains(&second));
        allocator.release(first);
        assert_eq!(allocator.allocate(), Some(first));
    }

    #[test]
    fn spreads_processes_over_the_range() {
        let a = PortAllocator::new(24010..24020).spread(3);
        let b = PortAllocator::new(24010..24020).spread(17);
        let (pa, pb) = (a.allocate().unwrap(), b.allocate().unwrap());
        assert_ne!(pa, pb, "different processes start in different places");
        assert!((24010..24020).contains(&pa) && (24010..24020).contains(&pb));
        // It still wraps around and hands out every port.
        let all = PortAllocator::new(24020..24024).spread(3);
        let mut ports: Vec<u16> = std::iter::from_fn(|| all.allocate()).collect();
        ports.sort_unstable();
        assert_eq!(ports.len(), 4);
    }

    #[test]
    fn exhausts_gracefully() {
        let allocator = PortAllocator::new(24030..24031);
        let only = allocator.allocate();
        assert!(only.is_some());
        assert_eq!(allocator.allocate(), None);
    }

    #[test]
    fn claims_a_remembered_port_once() {
        let allocator = PortAllocator::new(24040..24045);
        assert!(allocator.claim(24041));
        assert!(!allocator.claim(24041), "already handed out");
        assert!(!allocator.claim(24100), "outside the range");
        assert_ne!(allocator.allocate(), Some(24041));
    }
}
