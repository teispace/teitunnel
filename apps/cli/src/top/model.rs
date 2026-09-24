//! What `teitunnel top` shows and how keys change it (no terminal here, so it's tested
//! directly).

use std::collections::{HashMap, VecDeque};

/// The longest traffic history kept (samples).
const HISTORY: usize = 120;
/// The most recent requests kept.
const REQUESTS: usize = 200;

/// A share row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ShareRow {
    /// Pass to stop it.
    pub(crate) id: String,
    /// Public URL.
    pub(crate) url: Option<String>,
    /// The local service.
    pub(crate) origin: String,
    /// `app`, `terminal` or `domain`.
    pub(crate) by: &'static str,
    /// `live`, `starting`, …
    pub(crate) status: String,
    /// Milliseconds since the epoch.
    pub(crate) started_at: u64,
    /// Requests so far, when known.
    pub(crate) requests: Option<u64>,
    /// Requests per second over the last refresh.
    pub(crate) rate: Option<f64>,
}

/// How a route is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    /// Serving (or its last check passed).
    Up,
    /// Starting or reconnecting.
    Pending,
    /// Not serving (or its last check failed).
    Down,
    /// Not known yet.
    Unknown,
}

/// A route row.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RouteRow {
    /// Hostname (and path rule).
    pub(crate) hostname: String,
    /// Where it goes.
    pub(crate) origin: String,
    /// Its state.
    pub(crate) health: Health,
    /// Share of passing checks over 24 hours (0–1), from the uptime records.
    pub(crate) uptime: Option<f64>,
}

/// A request seen by the inspector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestRow {
    /// The share.
    pub(crate) share: String,
    /// Method.
    pub(crate) method: String,
    /// Path.
    pub(crate) path: String,
    /// Status, once answered.
    pub(crate) status: Option<u16>,
    /// Duration.
    pub(crate) duration_ms: Option<u64>,
}

/// Fresh data from the source.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Snapshot {
    /// Every share.
    pub(crate) shares: Vec<ShareRow>,
    /// Routes, when refreshed this time.
    pub(crate) routes: Option<Vec<RouteRow>>,
    /// The whole traffic history, when the source keeps it (per minute, from the
    /// connectors' records); otherwise it's built from the shares' request counts.
    pub(crate) traffic: Option<Vec<u64>>,
    /// Requests that arrived since the last snapshot.
    pub(crate) requests: Vec<RequestRow>,
}

/// The panes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Pane {
    Shares,
    Routes,
    Requests,
}

/// What the keyboard is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Normal,
    /// Typing a filter.
    Filter,
    /// Typing a port to share.
    Share,
    /// Showing the keys.
    Help,
}

/// A key, independent of the terminal library.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Key {
    Char(char),
    Enter,
    Esc,
    Tab,
    BackTab,
    Up,
    Down,
    Backspace,
    /// Ctrl-C.
    Interrupt,
}

/// Something for the runner to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Command {
    Quit,
    Share(String),
    Stop(String),
    Copy(String),
}

/// Where the data comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Origin {
    /// The running app.
    App { version: String },
    /// This machine's records (the app isn't running).
    Local,
}

/// The dashboard's state.
#[derive(Debug, Clone)]
pub(crate) struct Dashboard {
    pub(crate) origin: Origin,
    pub(crate) shares: Vec<ShareRow>,
    pub(crate) routes: Vec<RouteRow>,
    /// Newest last.
    pub(crate) traffic: VecDeque<u64>,
    /// Whether `traffic` is per minute (the connectors' records) or per refresh.
    pub(crate) traffic_per_minute: bool,
    /// `None` until the inspector reports a request: the pane stays hidden.
    pub(crate) requests: Option<VecDeque<RequestRow>>,
    pub(crate) focus: Pane,
    pub(crate) selected: HashMap<&'static str, usize>,
    pub(crate) filter: String,
    pub(crate) mode: Mode,
    pub(crate) input: String,
    pub(crate) message: Option<String>,
    /// Colours (off with `NO_COLOR`).
    pub(crate) color: bool,
    /// The share `x` was pressed on once (pressing it again stops it).
    pub(crate) pending_stop: Option<String>,
    /// Milliseconds since the epoch at the last snapshot.
    pub(crate) now_ms: u64,
    last_counts: HashMap<String, u64>,
}

fn pane_key(pane: Pane) -> &'static str {
    match pane {
        Pane::Shares => "shares",
        Pane::Routes => "routes",
        Pane::Requests => "requests",
    }
}

impl Dashboard {
    pub(crate) fn new(origin: Origin, color: bool) -> Self {
        Self {
            origin,
            shares: Vec::new(),
            routes: Vec::new(),
            traffic: VecDeque::new(),
            traffic_per_minute: false,
            requests: None,
            focus: Pane::Shares,
            selected: HashMap::new(),
            filter: String::new(),
            mode: Mode::Normal,
            input: String::new(),
            message: None,
            color,
            pending_stop: None,
            now_ms: 0,
            last_counts: HashMap::new(),
        }
    }

    /// Takes a snapshot taken at `now_ms`, `elapsed` seconds after the previous one.
    pub(crate) fn update(&mut self, mut snapshot: Snapshot, now_ms: u64, elapsed: f64) {
        let mut total = 0;
        for share in &mut snapshot.shares {
            if let Some(count) = share.requests {
                let delta = self
                    .last_counts
                    .get(&share.id)
                    .map(|before| count.saturating_sub(*before));
                if let Some(delta) = delta {
                    total += delta;
                    #[allow(clippy::cast_precision_loss)]
                    let rate = delta as f64 / elapsed.max(0.001);
                    share.rate = Some(rate);
                }
                self.last_counts.insert(share.id.clone(), count);
            }
        }
        self.last_counts
            .retain(|id, _| snapshot.shares.iter().any(|s| &s.id == id));
        self.shares = snapshot.shares;
        if let Some(routes) = snapshot.routes {
            self.routes = routes;
        }
        match snapshot.traffic {
            Some(history) => {
                self.traffic = history.into_iter().rev().take(HISTORY).rev().collect();
                self.traffic_per_minute = true;
            }
            None if !self.traffic_per_minute => {
                self.traffic.push_back(total);
                while self.traffic.len() > HISTORY {
                    self.traffic.pop_front();
                }
            }
            None => {}
        }
        if !snapshot.requests.is_empty() {
            let requests = self.requests.get_or_insert_with(VecDeque::new);
            requests.extend(snapshot.requests);
            while requests.len() > REQUESTS {
                requests.pop_front();
            }
        }
        if self
            .pending_stop
            .as_ref()
            .is_some_and(|id| !self.shares.iter().any(|s| &s.id == id))
        {
            self.pending_stop = None;
        }
        self.now_ms = now_ms;
        self.clamp();
    }

    fn matches(&self, text: &str) -> bool {
        self.filter.is_empty() || text.to_lowercase().contains(&self.filter.to_lowercase())
    }

    /// Shares passing the filter.
    pub(crate) fn visible_shares(&self) -> Vec<&ShareRow> {
        self.shares
            .iter()
            .filter(|s| {
                self.matches(&s.origin) || s.url.as_deref().is_some_and(|u| self.matches(u))
            })
            .collect()
    }

    /// Routes passing the filter.
    pub(crate) fn visible_routes(&self) -> Vec<&RouteRow> {
        self.routes
            .iter()
            .filter(|r| self.matches(&r.hostname) || self.matches(&r.origin))
            .collect()
    }

    /// Requests passing the filter, newest first.
    pub(crate) fn visible_requests(&self) -> Vec<&RequestRow> {
        self.requests
            .iter()
            .flatten()
            .rev()
            .filter(|r| self.matches(&r.path) || self.matches(&r.share))
            .collect()
    }

    fn count(&self, pane: Pane) -> usize {
        match pane {
            Pane::Shares => self.visible_shares().len(),
            Pane::Routes => self.visible_routes().len(),
            Pane::Requests => self.visible_requests().len(),
        }
    }

    /// The selected row of a pane.
    pub(crate) fn selection(&self, pane: Pane) -> usize {
        self.selected.get(pane_key(pane)).copied().unwrap_or(0)
    }

    fn clamp(&mut self) {
        for pane in [Pane::Shares, Pane::Routes, Pane::Requests] {
            let count = self.count(pane);
            let selected = self.selection(pane).min(count.saturating_sub(1));
            self.selected.insert(pane_key(pane), selected);
        }
    }

    fn panes(&self) -> Vec<Pane> {
        let mut panes = vec![Pane::Shares, Pane::Routes];
        if self.requests.is_some() {
            panes.push(Pane::Requests);
        }
        panes
    }

    fn move_focus(&mut self, forward: bool) {
        let panes = self.panes();
        let at = panes.iter().position(|p| *p == self.focus).unwrap_or(0);
        let next = if forward {
            (at + 1) % panes.len()
        } else {
            (at + panes.len() - 1) % panes.len()
        };
        self.focus = panes.get(next).copied().unwrap_or(Pane::Shares);
    }

    fn selected_share(&self) -> Option<&ShareRow> {
        self.visible_shares()
            .get(self.selection(Pane::Shares))
            .copied()
    }

    /// Handles a key; returns what the runner should do.
    pub(crate) fn key(&mut self, key: Key) -> Option<Command> {
        if key == Key::Interrupt {
            return Some(Command::Quit);
        }
        match self.mode {
            Mode::Filter | Mode::Share => self.typing(key),
            Mode::Help => {
                self.mode = Mode::Normal;
                None
            }
            Mode::Normal => self.normal(key),
        }
    }

    fn typing(&mut self, key: Key) -> Option<Command> {
        match key {
            Key::Esc => {
                if self.mode == Mode::Filter {
                    self.filter.clear();
                }
                self.input.clear();
                self.mode = Mode::Normal;
                self.clamp();
                None
            }
            Key::Enter => {
                let input = std::mem::take(&mut self.input);
                let mode = std::mem::replace(&mut self.mode, Mode::Normal);
                if mode == Mode::Share && !input.trim().is_empty() {
                    self.message = Some(format!("Sharing {}…", input.trim()));
                    return Some(Command::Share(input.trim().to_owned()));
                }
                None
            }
            Key::Backspace => {
                self.input.pop();
                if self.mode == Mode::Filter {
                    self.filter.clone_from(&self.input);
                    self.clamp();
                }
                None
            }
            Key::Char(c) if !c.is_control() && self.input.chars().count() < 200 => {
                self.input.push(c);
                if self.mode == Mode::Filter {
                    self.filter.clone_from(&self.input);
                    self.clamp();
                }
                None
            }
            _ => None,
        }
    }

    fn normal(&mut self, key: Key) -> Option<Command> {
        if !matches!(key, Key::Char('x')) {
            self.pending_stop = None;
        }
        match key {
            Key::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.clamp();
            }
            Key::Char('q') | Key::Esc => return Some(Command::Quit),
            Key::Tab => self.move_focus(true),
            Key::BackTab => self.move_focus(false),
            Key::Up | Key::Char('k') => {
                let selected = self.selection(self.focus).saturating_sub(1);
                self.selected.insert(pane_key(self.focus), selected);
            }
            Key::Down | Key::Char('j') => {
                let last = self.count(self.focus).saturating_sub(1);
                let selected = (self.selection(self.focus) + 1).min(last);
                self.selected.insert(pane_key(self.focus), selected);
            }
            Key::Char('/') => {
                self.mode = Mode::Filter;
                self.input.clone_from(&self.filter);
            }
            Key::Char('s') => {
                self.mode = Mode::Share;
                self.input.clear();
            }
            Key::Char('?') => self.mode = Mode::Help,
            Key::Char('x') => {
                let share = self.selected_share()?;
                let id = share.id.clone();
                let what = share.url.clone().unwrap_or_else(|| share.origin.clone());
                if self.pending_stop.as_deref() == Some(id.as_str()) {
                    self.pending_stop = None;
                    self.message = Some(format!("Stopping {what}…"));
                    return Some(Command::Stop(id));
                }
                self.message = Some(format!("Press x again to stop {what}."));
                self.pending_stop = Some(id);
            }
            Key::Char('c') => {
                let url = match self.focus {
                    Pane::Routes => self
                        .visible_routes()
                        .get(self.selection(Pane::Routes))
                        .map(|r| format!("https://{}", r.hostname)),
                    _ => self.selected_share().and_then(|s| s.url.clone()),
                }?;
                self.message = Some(format!("Copied {url}"));
                return Some(Command::Copy(url));
            }
            _ => {}
        }
        None
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn share(id: &str, requests: u64) -> ShareRow {
        ShareRow {
            id: id.into(),
            url: Some(format!("https://{id}.trycloudflare.com")),
            origin: "http://localhost:3000".into(),
            by: "app",
            status: "live".into(),
            started_at: 0,
            requests: Some(requests),
            rate: None,
        }
    }

    pub(crate) fn route(hostname: &str, health: Health) -> RouteRow {
        RouteRow {
            hostname: hostname.into(),
            origin: "http://localhost:8080".into(),
            health,
            uptime: Some(0.998),
        }
    }

    fn dashboard() -> Dashboard {
        let mut d = Dashboard::new(Origin::Local, true);
        d.update(
            Snapshot {
                shares: vec![share("a", 10), share("b", 0)],
                routes: Some(vec![
                    route("app.example.com", Health::Up),
                    route("api.example.com", Health::Down),
                ]),
                ..Snapshot::default()
            },
            1_000,
            1.0,
        );
        d
    }

    #[test]
    fn computes_request_rates_and_traffic() {
        let mut d = dashboard();
        assert_eq!(d.shares[0].rate, None, "no rate before a second sample");
        d.update(
            Snapshot {
                shares: vec![share("a", 30), share("b", 5)],
                ..Snapshot::default()
            },
            3_000,
            2.0,
        );
        assert_eq!(d.shares[0].rate, Some(10.0));
        assert_eq!(d.shares[1].rate, Some(2.5));
        assert_eq!(d.traffic.back(), Some(&25));
        assert_eq!(d.routes.len(), 2, "routes kept when not refreshed");
        // Per-minute history from the connectors replaces the running one.
        d.update(
            Snapshot {
                traffic: Some(vec![1, 2, 3]),
                ..Snapshot::default()
            },
            4_000,
            1.0,
        );
        assert_eq!(d.traffic, [1, 2, 3]);
        assert!(d.traffic_per_minute);
    }

    #[test]
    fn filters_and_selects() {
        let mut d = dashboard();
        d.key(Key::Char('/'));
        for c in "api".chars() {
            d.key(Key::Char(c));
        }
        d.key(Key::Enter);
        assert_eq!(d.visible_routes().len(), 1);
        assert_eq!(d.visible_shares().len(), 0);
        assert_eq!(d.key(Key::Esc), None, "Esc clears the filter first");
        assert_eq!(d.visible_routes().len(), 2);
        d.key(Key::Tab);
        assert_eq!(d.focus, Pane::Routes);
        d.key(Key::Down);
        d.key(Key::Down);
        assert_eq!(d.selection(Pane::Routes), 1, "stops at the last row");
        assert_eq!(
            d.key(Key::Char('c')),
            Some(Command::Copy("https://api.example.com".into()))
        );
        d.key(Key::Tab);
        assert_eq!(
            d.focus,
            Pane::Shares,
            "no requests pane without the inspector"
        );
        assert_eq!(d.key(Key::Esc), Some(Command::Quit));
    }

    #[test]
    fn shares_a_port_and_stops_after_a_second_press() {
        let mut d = dashboard();
        d.key(Key::Char('s'));
        for c in "5173".chars() {
            d.key(Key::Char(c));
        }
        assert_eq!(d.key(Key::Enter), Some(Command::Share("5173".into())));
        assert_eq!(d.mode, Mode::Normal);
        assert_eq!(d.key(Key::Char('x')), None);
        assert!(d.message.as_deref().is_some_and(|m| m.contains("again")));
        assert_eq!(d.key(Key::Char('x')), Some(Command::Stop("a".into())));
        // Anything else in between cancels.
        d.key(Key::Char('x'));
        d.key(Key::Down);
        assert_eq!(d.key(Key::Char('x')), None);
        assert_eq!(d.key(Key::Interrupt), Some(Command::Quit));
    }

    #[test]
    fn shows_requests_once_the_inspector_reports_them() {
        let mut d = dashboard();
        assert!(d.requests.is_none());
        d.update(
            Snapshot {
                shares: d.shares.clone(),
                requests: vec![RequestRow {
                    share: "a".into(),
                    method: "GET".into(),
                    path: "/".into(),
                    status: Some(200),
                    duration_ms: Some(12),
                }],
                ..Snapshot::default()
            },
            2_000,
            1.0,
        );
        assert_eq!(d.visible_requests().len(), 1);
        d.key(Key::Tab);
        d.key(Key::Tab);
        assert_eq!(d.focus, Pane::Requests);
    }
}
