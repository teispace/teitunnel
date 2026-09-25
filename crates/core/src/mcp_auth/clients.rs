//! Reading Client ID Metadata Documents safely: HTTPS only, public addresses only (the
//! connection is pinned to the address that was checked, so DNS can't be rebound), no
//! redirects, a small size limit and a short timeout, cached per the document's
//! `max-age`.

use std::{
    collections::HashMap,
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{Arc, Mutex, PoisonError},
    time::{Duration, Instant},
};

use super::protocol::{Client, ClientError, client_from_document};

/// Largest document read.
pub(crate) const MAX_DOCUMENT: usize = 10 * 1024;
/// How long a fetch may take.
const TIMEOUT: Duration = Duration::from_secs(5);
/// How long a document is kept without `max-age` (and at most, and at least).
const DEFAULT_TTL: Duration = Duration::from_secs(3600);
const MAX_TTL: Duration = Duration::from_secs(24 * 3600);
const MIN_TTL: Duration = Duration::from_secs(60);
/// Documents kept at most.
const MAX_CACHED: usize = 256;

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A fetched document's body and how long it may be kept, or why it couldn't be read.
pub(crate) type Fetched = Result<(Vec<u8>, Option<Duration>), String>;

/// Fetches a document: its body and how long it may be kept.
pub(crate) trait Fetch: Send + Sync + 'static {
    fn fetch<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Fetched>;
}

/// Whether an address is on the public internet (not this computer, a private or
/// shared network, a documentation or reserved range).
pub(crate) fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => public_v4(v4),
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public_v4(v4);
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (first & 0xfe00) == 0xfc00 // unique local
                || (first & 0xffc0) == 0xfe80 // link-local
                || (first & 0xffc0) == 0xfec0 // site-local
                || (first == 0x2001 && v6.segments()[1] == 0x0db8) // documentation
                || (first == 0x0064 && v6.segments()[1] == 0xff9b)) // NAT64
        }
    }
}

fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_loopback()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_unspecified()
        || ip.is_documentation()
        || a == 0
        || (a == 100 && (64..=127).contains(&b)) // shared address space
        || (a == 192 && b == 0 && c == 0) // IETF protocol assignments
        || (a == 198 && (18..=19).contains(&b)) // benchmarking
        || a >= 240)
}

/// `max-age` from a `Cache-Control` value, kept within bounds.
pub(crate) fn ttl(cache_control: Option<&str>) -> Duration {
    let Some(value) = cache_control else {
        return DEFAULT_TTL;
    };
    if value.split(',').any(|d| {
        matches!(
            d.trim().to_ascii_lowercase().as_str(),
            "no-store" | "no-cache"
        )
    }) {
        return MIN_TTL;
    }
    value
        .split(',')
        .find_map(|d| d.trim().strip_prefix("max-age=")?.parse::<u64>().ok())
        .map_or(DEFAULT_TTL, |secs| {
            Duration::from_secs(secs).clamp(MIN_TTL, MAX_TTL)
        })
}

/// The real fetcher: reqwest pinned to a checked public address.
#[derive(Debug, Default)]
pub(crate) struct Web;

impl Fetch for Web {
    fn fetch<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Fetched> {
        Box::pin(async move {
            let uri: http::Uri = url.parse().map_err(|_| "not a URL".to_owned())?;
            let host = uri.host().ok_or("no host")?.to_owned();
            let port = uri.port_u16().unwrap_or(443);
            let addresses: Vec<SocketAddr> = tokio::time::timeout(
                TIMEOUT,
                tokio::net::lookup_host((host.trim_matches(['[', ']']), port)),
            )
            .await
            .map_err(|_| "the name didn't resolve in time".to_owned())?
            .map_err(|e| format!("the name didn't resolve: {e}"))?
            .collect();
            // Every address must be public, or a rebinding answer could reach inside.
            if addresses.is_empty() || !addresses.iter().all(|a| is_public(a.ip())) {
                return Err("it doesn't resolve to a public address".into());
            }
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(TIMEOUT)
                .https_only(true)
                .no_proxy()
                .resolve_to_addrs(&host, &addresses)
                .user_agent(concat!("Teitunnel/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| e.to_string())?;
            let mut response = client
                .get(url)
                .header(http::header::ACCEPT, "application/json")
                .send()
                .await
                .map_err(|e| format!("couldn't be fetched: {e}"))?;
            if !response.status().is_success() {
                return Err(format!("answered {}", response.status()));
            }
            if response
                .content_length()
                .is_some_and(|n| n > MAX_DOCUMENT as u64)
            {
                return Err("it's too large".into());
            }
            let ttl = ttl(response
                .headers()
                .get(http::header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()));
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
                body.extend_from_slice(&chunk);
                if body.len() > MAX_DOCUMENT {
                    return Err("it's too large".into());
                }
            }
            Ok((body, Some(ttl)))
        })
    }
}

/// Client documents, fetched on demand and cached.
pub(crate) struct Documents {
    fetch: Arc<dyn Fetch>,
    cache: Mutex<HashMap<String, (Client, Instant)>>,
}

impl std::fmt::Debug for Documents {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Documents").finish_non_exhaustive()
    }
}

impl Documents {
    pub(crate) fn new(fetch: Arc<dyn Fetch>) -> Self {
        Self {
            fetch,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// The client whose document is at `url`.
    pub(crate) async fn client(&self, url: &str) -> Result<Client, ClientError> {
        let now = Instant::now();
        if let Some((client, until)) = self
            .cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(url)
            && *until > now
        {
            return Ok(client.clone());
        }
        let (body, ttl) = self
            .fetch
            .fetch(url)
            .await
            .map_err(|why| ClientError(format!("the client's metadata at {url}: {why}")))?;
        let document: serde_json::Value = serde_json::from_slice(&body)
            .map_err(|_| ClientError(format!("the client's metadata at {url} isn't JSON")))?;
        let client = client_from_document(url, &document)?;
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= MAX_CACHED {
            cache.retain(|_, (_, until)| *until > now);
            if cache.len() >= MAX_CACHED {
                cache.clear();
            }
        }
        cache.insert(
            url.to_owned(),
            (client.clone(), now + ttl.unwrap_or(DEFAULT_TTL)),
        );
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn only_public_addresses_are_fetched() {
        for private in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "192.0.2.1",
            "198.18.0.1",
            "240.0.0.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "2001:db8::1",
            "64:ff9b::a00:1",
        ] {
            assert!(!is_public(private.parse().unwrap()), "{private}");
        }
        for public in ["1.1.1.1", "160.79.104.10", "2606:4700::1111"] {
            assert!(is_public(public.parse().unwrap()), "{public}");
        }
    }

    #[test]
    fn keeps_documents_as_long_as_they_say_within_bounds() {
        assert_eq!(ttl(None), DEFAULT_TTL);
        assert_eq!(ttl(Some("public, max-age=600")), Duration::from_secs(600));
        assert_eq!(ttl(Some("max-age=5")), MIN_TTL);
        assert_eq!(ttl(Some("max-age=99999999")), MAX_TTL);
        assert_eq!(ttl(Some("no-store")), MIN_TTL);
    }

    struct Counting(AtomicUsize, serde_json::Value);

    impl Fetch for Counting {
        fn fetch<'a>(&'a self, _url: &'a str) -> BoxFuture<'a, Fetched> {
            self.0.fetch_add(1, Ordering::SeqCst);
            let body = serde_json::to_vec(&self.1).unwrap();
            Box::pin(async move { Ok((body, None)) })
        }
    }

    #[tokio::test]
    async fn fetches_once_and_checks_the_document() {
        let url = "https://client.test/oauth/metadata.json";
        let fetch = Arc::new(Counting(
            AtomicUsize::new(0),
            serde_json::json!({
                "client_id": url,
                "client_name": "Test Client",
                "redirect_uris": ["https://client.test/callback"],
            }),
        ));
        let documents = Documents::new(fetch.clone());
        assert_eq!(documents.client(url).await.unwrap().name, "Test Client");
        assert_eq!(documents.client(url).await.unwrap().name, "Test Client");
        assert_eq!(fetch.0.load(Ordering::SeqCst), 1, "cached");
        let other = Documents::new(fetch);
        assert!(
            other
                .client("https://client.test/other.json")
                .await
                .is_err(),
            "a document for another URL is refused"
        );
    }
}
