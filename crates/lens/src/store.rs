//! Capture storage: the [`CaptureStore`] trait, the in-memory ring ([`MemoryStore`]) and
//! queries ([`Filter`], [`Query`], [`Page`]).

use std::{
    collections::{HashMap, VecDeque},
    fmt,
    sync::{Arc, Mutex, PoisonError},
};

use serde::{Deserialize, Serialize};

use crate::{
    Exchange, ExchangeId, ExchangeKind, Redaction, TapId,
    capture::decode_body,
    redact::{mask_json, mask_query, mask_text},
    util::find_ascii_ci,
};

/// Default number of exchanges kept per tap.
pub const DEFAULT_CAPACITY: usize = 1_000;
/// Largest page a query returns.
pub const MAX_PAGE: usize = 1_000;

/// Where captured exchanges live.
///
/// Lens calls [`CaptureStore::put`] whenever an exchange is added or changes (the same
/// id again means "replace"). The embedder can wrap [`MemoryStore`] to also persist
/// exchanges (e.g. to SQLite on a blocking thread); `put` is called on the proxy's hot
/// path, so it must not block.
pub trait CaptureStore: Send + Sync + fmt::Debug {
    /// Sets how many exchanges to keep for `tap` (called when a tap is added or changed).
    fn configure(&self, tap: &TapId, capacity: usize);
    /// Inserts or replaces an exchange.
    fn put(&self, exchange: Arc<Exchange>);
    /// Looks an exchange up by id.
    fn get(&self, id: ExchangeId) -> Option<Arc<Exchange>>;
    /// Newest-first page of exchanges matching `query`. Text search runs against the
    /// masked view when `redaction` masks, so hidden secrets can't be found by search.
    fn list(&self, query: &Query, redaction: &Redaction) -> Page;
    /// Forgets the exchanges of one tap, or of all taps.
    fn clear(&self, tap: Option<&TapId>);
}

/// Criteria for listing exchanges. Empty fields match everything.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Filter {
    /// Only this tap.
    pub tap: Option<TapId>,
    /// Any of these methods (case-insensitive).
    pub methods: Vec<String>,
    /// Any of these status classes (`2` for 2xx…).
    pub status_classes: Vec<u8>,
    /// Any of these exact statuses.
    pub statuses: Vec<u16>,
    /// Substring of the path (case-insensitive).
    pub path: Option<String>,
    /// Substring of the host (case-insensitive).
    pub host: Option<String>,
    /// Text anywhere in the URL, header values or bodies (case-insensitive).
    pub text: Option<String>,
    /// At least this long (milliseconds).
    pub min_duration_ms: Option<u64>,
    /// Started at or after (Unix milliseconds).
    pub since_ms: Option<u64>,
    /// Started before (Unix milliseconds).
    pub until_ms: Option<u64>,
    /// Any of these kinds.
    pub kinds: Vec<ExchangeKind>,
    /// Only exchanges with an error.
    pub errors_only: bool,
    /// Only finished exchanges.
    pub finished_only: bool,
}

impl Filter {
    /// Whether `exchange` matches, searching text in the view `redaction` produces.
    pub fn matches(&self, exchange: &Exchange, redaction: &Redaction) -> bool {
        self.matches_cheap(exchange) && self.matches_text(exchange, redaction)
    }

    fn matches_cheap(&self, exchange: &Exchange) -> bool {
        if self.tap.as_ref().is_some_and(|tap| *tap != exchange.tap) {
            return false;
        }
        if !self.methods.is_empty()
            && !self
                .methods
                .iter()
                .any(|m| m.eq_ignore_ascii_case(exchange.request.method.as_str()))
        {
            return false;
        }
        let status = exchange.status().map(|s| s.as_u16());
        if !self.status_classes.is_empty()
            && !status.is_some_and(|s| self.status_classes.iter().any(|&c| u16::from(c) == s / 100))
        {
            return false;
        }
        if !self.statuses.is_empty() && !status.is_some_and(|s| self.statuses.contains(&s)) {
            return false;
        }
        if let Some(path) = &self.path
            && find_ascii_ci(exchange.request.path().as_bytes(), path.as_bytes()).is_none()
        {
            return false;
        }
        if let Some(host) = &self.host
            && find_ascii_ci(exchange.request.host.as_bytes(), host.as_bytes()).is_none()
        {
            return false;
        }
        if let Some(min) = self.min_duration_ms {
            let ms = exchange
                .duration()
                .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
            if ms < min {
                return false;
            }
        }
        if self
            .since_ms
            .is_some_and(|since| exchange.started_at_ms < since)
            || self
                .until_ms
                .is_some_and(|until| exchange.started_at_ms >= until)
        {
            return false;
        }
        if !self.kinds.is_empty() && !self.kinds.contains(&exchange.kind) {
            return false;
        }
        if self.errors_only && exchange.error.is_none() {
            return false;
        }
        if self.finished_only && !exchange.is_finished() {
            return false;
        }
        true
    }

    fn matches_text(&self, exchange: &Exchange, redaction: &Redaction) -> bool {
        let Some(needle) = self.text.as_deref().filter(|t| !t.is_empty()) else {
            return true;
        };
        let needle = needle.as_bytes();
        searchable_parts(exchange, redaction)
            .iter()
            .any(|part| find_ascii_ci(part.as_bytes(), needle).is_some())
    }
}

/// The texts a search looks at, masked per `redaction`.
fn searchable_parts(exchange: &Exchange, redaction: &Redaction) -> Vec<String> {
    let request = &exchange.request;
    let mut parts = vec![mask_text(request.path(), redaction).into_owned()];
    if let Some(query) = request.query() {
        parts.push(mask_query(query, redaction).into_owned());
    }
    let mut add_headers = |headers: &http::HeaderMap| {
        for (name, value) in headers {
            let raw = String::from_utf8_lossy(value.as_bytes());
            parts.push(crate::redact::mask_header(name.as_str(), &raw, redaction).into_owned());
        }
    };
    add_headers(&request.headers);
    if let Some(response) = &exchange.response {
        add_headers(&response.headers);
    }
    let mut add_body = |headers: &http::HeaderMap, data: &[u8]| {
        if data.is_empty() {
            return;
        }
        if let Ok(decoded) = decode_body(headers, data) {
            let text = String::from_utf8_lossy(&decoded);
            parts.push(mask_json(&text, redaction).into_owned());
        }
    };
    add_body(&request.headers, &request.body.data);
    if let Some(response) = &exchange.response {
        add_body(&response.headers, &response.body.data);
    }
    if let Some(stream) = &exchange.stream {
        for preview in &stream.previews {
            parts.push(mask_json(&preview.preview, redaction).into_owned());
        }
    }
    parts
}

/// A page request: filter, size and cursor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    /// What to match.
    pub filter: Filter,
    /// Page size (default 100, at most [`MAX_PAGE`]).
    pub limit: Option<usize>,
    /// Only exchanges older than this one (the previous page's `next`).
    pub before: Option<ExchangeId>,
}

/// One page of results, newest first.
#[derive(Debug, Clone, Default)]
pub struct Page {
    /// Matching exchanges, newest first.
    pub items: Vec<Arc<Exchange>>,
    /// Cursor for the next (older) page, when there may be more.
    pub next: Option<ExchangeId>,
}

/// Runs `query` over `candidates` (any order).
pub(crate) fn run_query(
    mut candidates: Vec<Arc<Exchange>>,
    query: &Query,
    redaction: &Redaction,
) -> Page {
    let limit = query.limit.unwrap_or(100).clamp(1, MAX_PAGE);
    candidates.sort_unstable_by_key(|exchange| std::cmp::Reverse(exchange.id));
    let mut items = Vec::new();
    let mut more = false;
    for exchange in candidates {
        if query.before.is_some_and(|before| exchange.id >= before) {
            continue;
        }
        if !query.filter.matches(&exchange, redaction) {
            continue;
        }
        if items.len() == limit {
            more = true;
            break;
        }
        items.push(exchange);
    }
    let next = if more {
        items.last().map(|exchange| exchange.id)
    } else {
        None
    };
    Page { items, next }
}

#[derive(Debug)]
struct Ring {
    capacity: usize,
    items: VecDeque<Arc<Exchange>>,
}

#[derive(Debug, Default)]
struct Inner {
    rings: HashMap<TapId, Ring>,
    index: HashMap<ExchangeId, (TapId, u64)>,
}

/// The default store: a bounded ring of exchanges per tap, in memory.
#[derive(Debug)]
pub struct MemoryStore {
    default_capacity: usize,
    inner: Mutex<Inner>,
}

impl MemoryStore {
    /// A store keeping `default_capacity` exchanges per tap unless configured otherwise.
    pub fn new(default_capacity: usize) -> Self {
        Self {
            default_capacity: default_capacity.max(1),
            inner: Mutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Number of exchanges held for `tap`.
    pub fn len(&self, tap: &TapId) -> usize {
        self.lock()
            .rings
            .get(tap)
            .map_or(0, |ring| ring.items.len())
    }

    /// Whether nothing is held for `tap`.
    pub fn is_empty(&self, tap: &TapId) -> bool {
        self.len(tap) == 0
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

impl CaptureStore for MemoryStore {
    fn configure(&self, tap: &TapId, capacity: usize) {
        let capacity = capacity.max(1);
        let mut inner = self.lock();
        let Inner { rings, index } = &mut *inner;
        let ring = rings.entry(tap.clone()).or_insert_with(|| Ring {
            capacity,
            items: VecDeque::new(),
        });
        ring.capacity = capacity;
        while ring.items.len() > ring.capacity {
            if let Some(old) = ring.items.pop_front() {
                index.remove(&old.id);
            }
        }
    }

    fn put(&self, exchange: Arc<Exchange>) {
        let default_capacity = self.default_capacity;
        let mut inner = self.lock();
        let Inner { rings, index } = &mut *inner;
        let ring = rings.entry(exchange.tap.clone()).or_insert_with(|| Ring {
            capacity: default_capacity,
            items: VecDeque::new(),
        });
        match ring
            .items
            .binary_search_by(|probe| probe.seq.cmp(&exchange.seq))
        {
            Ok(position) => ring.items[position] = exchange,
            Err(position) => {
                if position == 0 && ring.items.len() >= ring.capacity {
                    // Older than everything in a full ring: an update of an exchange
                    // that already fell out, which would be evicted again at once.
                    return;
                }
                index.insert(exchange.id, (exchange.tap.clone(), exchange.seq));
                ring.items.insert(position, exchange);
                while ring.items.len() > ring.capacity {
                    if let Some(old) = ring.items.pop_front() {
                        index.remove(&old.id);
                    }
                }
            }
        }
    }

    fn get(&self, id: ExchangeId) -> Option<Arc<Exchange>> {
        let inner = self.lock();
        let (tap, seq) = inner.index.get(&id)?;
        let ring = inner.rings.get(tap)?;
        let position = ring
            .items
            .binary_search_by(|probe| probe.seq.cmp(seq))
            .ok()?;
        ring.items.get(position).cloned()
    }

    fn list(&self, query: &Query, redaction: &Redaction) -> Page {
        // Snapshot the Arcs under the lock, filter outside it, so a slow text search
        // never stalls recording.
        let candidates: Vec<Arc<Exchange>> = {
            let inner = self.lock();
            match &query.filter.tap {
                Some(tap) => inner
                    .rings
                    .get(tap)
                    .map(|ring| ring.items.iter().cloned().collect())
                    .unwrap_or_default(),
                None => inner
                    .rings
                    .values()
                    .flat_map(|ring| ring.items.iter().cloned())
                    .collect(),
            }
        };
        run_query(candidates, query, redaction)
    }

    fn clear(&self, tap: Option<&TapId>) {
        let mut inner = self.lock();
        match tap {
            Some(tap) => {
                if let Some(ring) = inner.rings.get_mut(tap) {
                    let removed: Vec<ExchangeId> = ring.items.drain(..).map(|e| e.id).collect();
                    for id in removed {
                        inner.index.remove(&id);
                    }
                }
            }
            None => {
                for ring in inner.rings.values_mut() {
                    ring.items.clear();
                }
                inner.index.clear();
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use bytes::Bytes;
    use http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, Version};

    use super::*;
    use crate::{
        BodyRecord, ClientInfo, ExchangeState, RequestRecord, Responder, ResponseRecord, Timings,
    };

    pub(crate) fn sample(tap: &str, seq: u64, method: Method, path: &str, status: u16) -> Exchange {
        let mut headers = HeaderMap::new();
        headers.insert("host", HeaderValue::from_static("app.example.com"));
        headers.insert(
            "authorization",
            HeaderValue::from_static("Bearer topsecret"),
        );
        Exchange {
            id: ExchangeId::new(),
            seq,
            tap: TapId::new(tap).unwrap(),
            kind: ExchangeKind::Http,
            state: ExchangeState::Complete,
            started_at_ms: 1_000 * seq,
            timings: Timings {
                complete_us: Some(seq * 10_000),
                ..Timings::default()
            },
            client: ClientInfo {
                ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                peer: SocketAddr::from(([127, 0, 0, 1], 5000)),
                cf_ray: None,
                country: None,
            },
            request: RequestRecord {
                method,
                uri: path.parse::<Uri>().unwrap(),
                scheme: "https".into(),
                host: "app.example.com".into(),
                version: Version::HTTP_11,
                headers,
                body: BodyRecord::full(Bytes::from_static(b"{\"hello\":\"world\"}")),
            },
            response: Some(ResponseRecord {
                status: StatusCode::from_u16(status).unwrap(),
                version: Version::HTTP_11,
                headers: HeaderMap::new(),
                body: BodyRecord::full(Bytes::from_static(b"ok")),
            }),
            responder: Responder::Upstream,
            error: None,
            stream: None,
            replay_of: None,
            fault: None,
        }
    }

    #[test]
    fn ring_is_bounded_and_newest_first() {
        let store = MemoryStore::new(3);
        for seq in 1..=5 {
            store.put(Arc::new(sample("t", seq, Method::GET, "/", 200)));
        }
        let tap = TapId::new("t").unwrap();
        assert_eq!(store.len(&tap), 3);
        let page = store.list(&Query::default(), &Redaction::masked());
        let seqs: Vec<u64> = page.items.iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![5, 4, 3]);
    }

    #[test]
    fn put_replaces_by_seq_and_ignores_evicted_updates() {
        let store = MemoryStore::new(2);
        let first = sample("t", 1, Method::GET, "/", 200);
        store.put(Arc::new(first.clone()));
        let mut updated = first.clone();
        updated.state = ExchangeState::Failed;
        store.put(Arc::new(updated));
        assert_eq!(store.get(first.id).unwrap().state, ExchangeState::Failed);

        store.put(Arc::new(sample("t", 2, Method::GET, "/", 200)));
        store.put(Arc::new(sample("t", 3, Method::GET, "/", 200)));
        assert!(store.get(first.id).is_none());
        // A late update of the evicted exchange doesn't come back.
        store.put(Arc::new(first.clone()));
        assert!(store.get(first.id).is_none());
        assert_eq!(store.len(&TapId::new("t").unwrap()), 2);
    }

    #[test]
    fn out_of_order_inserts_stay_sorted() {
        let store = MemoryStore::new(10);
        for seq in [2, 1, 3] {
            store.put(Arc::new(sample("t", seq, Method::GET, "/", 200)));
        }
        let page = store.list(&Query::default(), &Redaction::masked());
        assert_eq!(page.items.len(), 3);
    }

    #[test]
    fn filters_and_pagination() {
        let store = MemoryStore::new(100);
        for seq in 1..=10 {
            let method = if seq % 2 == 0 {
                Method::POST
            } else {
                Method::GET
            };
            let status = if seq % 3 == 0 { 500 } else { 200 };
            store.put(Arc::new(sample(
                "t",
                seq,
                method,
                &format!("/items/{seq}"),
                status,
            )));
        }
        let redaction = Redaction::masked();
        let posts = Query {
            filter: Filter {
                methods: vec!["post".into()],
                ..Filter::default()
            },
            ..Query::default()
        };
        assert_eq!(store.list(&posts, &redaction).items.len(), 5);

        let errors = Query {
            filter: Filter {
                status_classes: vec![5],
                ..Filter::default()
            },
            ..Query::default()
        };
        assert_eq!(store.list(&errors, &redaction).items.len(), 3);

        let slow = Query {
            filter: Filter {
                min_duration_ms: Some(80),
                path: Some("ITEMS".into()),
                ..Filter::default()
            },
            ..Query::default()
        };
        assert_eq!(store.list(&slow, &redaction).items.len(), 3);

        let mut query = Query {
            limit: Some(4),
            ..Query::default()
        };
        let first = store.list(&query, &redaction);
        assert_eq!(first.items.len(), 4);
        assert_eq!(first.items[0].seq, 10);
        query.before = first.next;
        let second = store.list(&query, &redaction);
        assert_eq!(second.items[0].seq, 6);
        query.before = second.next;
        query.limit = Some(10);
        let last = store.list(&query, &redaction);
        assert_eq!(last.items.len(), 2);
        assert!(last.next.is_none());
    }

    #[test]
    fn text_search_respects_masking() {
        let store = MemoryStore::new(10);
        store.put(Arc::new(sample("t", 1, Method::GET, "/", 200)));
        let query = |text: &str| Query {
            filter: Filter {
                text: Some(text.into()),
                ..Filter::default()
            },
            ..Query::default()
        };
        assert_eq!(
            store
                .list(&query("WORLD"), &Redaction::masked())
                .items
                .len(),
            1
        );
        assert_eq!(
            store
                .list(&query("topsecret"), &Redaction::masked())
                .items
                .len(),
            0
        );
        assert_eq!(
            store
                .list(&query("topsecret"), &Redaction::revealed())
                .items
                .len(),
            1
        );
    }

    #[test]
    fn clear_one_tap_or_all() {
        let store = MemoryStore::new(10);
        store.put(Arc::new(sample("a", 1, Method::GET, "/", 200)));
        store.put(Arc::new(sample("b", 1, Method::GET, "/", 200)));
        store.clear(Some(&TapId::new("a").unwrap()));
        assert_eq!(
            store
                .list(&Query::default(), &Redaction::masked())
                .items
                .len(),
            1
        );
        store.clear(None);
        assert!(
            store
                .list(&Query::default(), &Redaction::masked())
                .items
                .is_empty()
        );
    }
}
