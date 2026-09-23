//! A route's origin settings (`originRequest`), typed (M3-02, D-078).
//!
//! Every field Cloudflare's API accepts for a remotely managed tunnel
//! (`tunnel_originRequest` in `cloudflare/api-schemas`, checked 2026-09-23) except
//! `access`, which Teitunnel leaves as it finds it. Defaults are omitted when written,
//! and fields Teitunnel doesn't know are always kept: [`OriginOptions::apply`] only
//! touches the known ones.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::text::{Text, UserText, msg::origin_options as m};

/// Origin settings of a route. `None`/`false` means cloudflared's default.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "camelCase")]
pub struct OriginOptions {
    /// Host header sent to the origin (e.g. a dev server that checks it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http_host_header: Option<String>,
    /// Hostname expected on the origin's TLS certificate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin_server_name: Option<String>,
    /// Use the request's hostname as the TLS server name.
    #[serde(default, rename = "matchSNItoHost", skip_serializing_if = "is_false")]
    pub match_sni_to_host: bool,
    /// Accept any certificate from the origin (self-signed ones).
    #[serde(default, rename = "noTLSVerify", skip_serializing_if = "is_false")]
    pub no_tls_verify: bool,
    /// Certificate authority file for the origin's certificate (absolute path).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_pool: Option<String>,
    /// Speak HTTP/2 to the origin (HTTPS origins).
    #[serde(default, rename = "http2Origin", skip_serializing_if = "is_false")]
    pub http2_origin: bool,
    /// Don't use chunked transfer encoding (some WSGI servers need this).
    #[serde(default, skip_serializing_if = "is_false")]
    pub disable_chunked_encoding: bool,
    /// Seconds to wait for a TCP connection (default 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connect_timeout: Option<u32>,
    /// Seconds to wait for the TLS handshake (default 10).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_timeout: Option<u32>,
    /// Seconds between TCP keepalive packets (default 30).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tcp_keep_alive: Option<u32>,
    /// Seconds before an idle keepalive connection closes (default 90).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_alive_timeout: Option<u32>,
    /// Idle keepalive connections kept open (default 100).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keep_alive_connections: Option<u32>,
    /// Don't fall back between IPv4 and IPv6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub no_happy_eyeballs: bool,
    /// Proxy type for TCP origins: `socks` for a SOCKS5 proxy.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy_type: Option<String>,
}

#[allow(clippy::trivially_copy_pass_by_ref)] // serde's skip_serializing_if signature
fn is_false(value: &bool) -> bool {
    !value
}

/// The `originRequest` keys [`OriginOptions`] manages.
pub const KNOWN_KEYS: &[&str] = &[
    "httpHostHeader",
    "originServerName",
    "matchSNItoHost",
    "noTLSVerify",
    "caPool",
    "http2Origin",
    "disableChunkedEncoding",
    "connectTimeout",
    "tlsTimeout",
    "tcpKeepAlive",
    "keepAliveTimeout",
    "keepAliveConnections",
    "noHappyEyeballs",
    "proxyType",
];

/// Why origin settings are invalid; `field` is the JSON key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginOptionsError {
    /// The offending key, e.g. `connectTimeout`.
    pub field: &'static str,
    kind: ErrorKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ErrorKind {
    Host,
    Path,
    Seconds,
    Connections,
    ProxyType,
}

impl UserText for OriginOptionsError {
    fn text(&self) -> Text {
        match self.kind {
            ErrorKind::Host => m::host(),
            ErrorKind::Path => m::path(),
            ErrorKind::Seconds => m::seconds(MAX_SECONDS),
            ErrorKind::Connections => m::connections(MAX_CONNECTIONS),
            ErrorKind::ProxyType => m::proxy_type(),
        }
    }
}

impl std::fmt::Display for OriginOptionsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.text().english())
    }
}

impl std::error::Error for OriginOptionsError {}

const MAX_SECONDS: u32 = 24 * 60 * 60;
const MAX_CONNECTIONS: u32 = 10_000;

/// A host name, optionally with a port (`api.local`, `localhost:8080`, `[::1]:8080`).
fn valid_host(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 260
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':' | '[' | ']'))
}

fn trimmed(value: Option<&String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

impl OriginOptions {
    /// Whether every setting is cloudflared's default.
    pub fn is_default(&self) -> bool {
        self == &Self::default()
    }

    /// Checks every field and returns the settings with text trimmed and empty text
    /// dropped.
    ///
    /// # Errors
    /// The first invalid field.
    pub fn validated(&self) -> Result<Self, OriginOptionsError> {
        let error = |field, kind| OriginOptionsError { field, kind };
        let mut out = self.clone();
        out.http_host_header = trimmed(self.http_host_header.as_ref());
        out.origin_server_name = trimmed(self.origin_server_name.as_ref());
        out.ca_pool = trimmed(self.ca_pool.as_ref());
        out.proxy_type = trimmed(self.proxy_type.as_ref());
        if let Some(host) = &out.http_host_header
            && !valid_host(host)
        {
            return Err(error("httpHostHeader", ErrorKind::Host));
        }
        if let Some(host) = &out.origin_server_name
            && !valid_host(host)
        {
            return Err(error("originServerName", ErrorKind::Host));
        }
        if let Some(path) = &out.ca_pool
            && !(path.starts_with('/') || path.chars().nth(1) == Some(':'))
        {
            return Err(error("caPool", ErrorKind::Path));
        }
        for (field, value) in [
            ("connectTimeout", out.connect_timeout),
            ("tlsTimeout", out.tls_timeout),
            ("tcpKeepAlive", out.tcp_keep_alive),
            ("keepAliveTimeout", out.keep_alive_timeout),
        ] {
            if value.is_some_and(|s| s == 0 || s > MAX_SECONDS) {
                return Err(error(field, ErrorKind::Seconds));
            }
        }
        if out
            .keep_alive_connections
            .is_some_and(|n| n == 0 || n > MAX_CONNECTIONS)
        {
            return Err(error("keepAliveConnections", ErrorKind::Connections));
        }
        if out.proxy_type.as_deref().is_some_and(|p| p != "socks") {
            return Err(error("proxyType", ErrorKind::ProxyType));
        }
        Ok(out)
    }

    /// Reads the known settings from an `originRequest` object (remote config: seconds
    /// as numbers; a local `config.yml` may say `"30s"`).
    pub fn from_map(map: &Map<String, Value>) -> Self {
        let text = |key: &str| map.get(key).and_then(Value::as_str).map(str::to_owned);
        let flag = |key: &str| map.get(key).and_then(Value::as_bool).unwrap_or(false);
        let seconds = |key: &str| map.get(key).and_then(seconds_of);
        Self {
            http_host_header: text("httpHostHeader"),
            origin_server_name: text("originServerName"),
            match_sni_to_host: flag("matchSNItoHost"),
            no_tls_verify: flag("noTLSVerify"),
            ca_pool: text("caPool"),
            http2_origin: flag("http2Origin"),
            disable_chunked_encoding: flag("disableChunkedEncoding"),
            connect_timeout: seconds("connectTimeout"),
            tls_timeout: seconds("tlsTimeout"),
            tcp_keep_alive: seconds("tcpKeepAlive"),
            keep_alive_timeout: seconds("keepAliveTimeout"),
            keep_alive_connections: map
                .get("keepAliveConnections")
                .and_then(Value::as_u64)
                .and_then(|n| u32::try_from(n).ok()),
            no_happy_eyeballs: flag("noHappyEyeballs"),
            proxy_type: text("proxyType"),
        }
    }

    /// Writes these settings into `map`: known keys are set or removed (defaults are
    /// omitted); every other key stays.
    pub fn apply(&self, map: &mut Map<String, Value>) {
        for key in KNOWN_KEYS {
            map.remove(*key);
        }
        if let Ok(Value::Object(set)) = serde_json::to_value(self) {
            map.extend(set);
        }
    }

    /// `originRequest` keys in `map` that this type doesn't cover.
    pub fn unknown_keys(map: &Map<String, Value>) -> Vec<String> {
        map.keys()
            .filter(|k| !KNOWN_KEYS.contains(&k.as_str()))
            .cloned()
            .collect()
    }
}

/// Seconds from a number, or a Go duration such as `30s`, `1m30s`, `2h` or `500ms`
/// (rounded up to whole seconds).
fn seconds_of(value: &Value) -> Option<u32> {
    if let Some(n) = value.as_u64() {
        return u32::try_from(n).ok();
    }
    let text = value.as_str()?.trim();
    let mut total_ms: u64 = 0;
    let mut rest = text;
    while !rest.is_empty() {
        let digits = rest.find(|c: char| !c.is_ascii_digit())?;
        let number: u64 = rest[..digits].parse().ok()?;
        rest = &rest[digits..];
        let (unit_ms, len) = if rest.starts_with("ms") {
            (1, 2)
        } else if rest.starts_with('s') {
            (1_000, 1)
        } else if rest.starts_with('m') {
            (60_000, 1)
        } else if rest.starts_with('h') {
            (3_600_000, 1)
        } else {
            return None;
        };
        total_ms = total_ms.checked_add(number.checked_mul(unit_ms)?)?;
        rest = &rest[len..];
    }
    u32::try_from(total_ms.div_ceil(1_000))
        .ok()
        .filter(|s| *s > 0)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn map(value: Value) -> Map<String, Value> {
        let Value::Object(map) = value else {
            unreachable!("an object")
        };
        map
    }

    #[test]
    fn round_trips_every_field_with_the_api_names() {
        let options = OriginOptions {
            http_host_header: Some("api.local".into()),
            origin_server_name: Some("origin.example.com".into()),
            match_sni_to_host: true,
            no_tls_verify: true,
            ca_pool: Some("/etc/ssl/ca.pem".into()),
            http2_origin: true,
            disable_chunked_encoding: true,
            connect_timeout: Some(10),
            tls_timeout: Some(5),
            tcp_keep_alive: Some(15),
            keep_alive_timeout: Some(60),
            keep_alive_connections: Some(50),
            no_happy_eyeballs: true,
            proxy_type: Some("socks".into()),
        };
        let mut written = Map::new();
        options.apply(&mut written);
        let keys: Vec<&str> = written.keys().map(String::as_str).collect();
        for key in KNOWN_KEYS {
            assert!(keys.contains(key), "{key} missing from {keys:?}");
        }
        assert_eq!(written["noTLSVerify"], json!(true));
        assert_eq!(written["connectTimeout"], json!(10));
        assert_eq!(OriginOptions::from_map(&written), options);
    }

    #[test]
    fn omits_defaults_and_keeps_what_it_doesnt_know() {
        let mut existing = map(json!({
            "noTLSVerify": true,
            "access": {"required": true, "teamName": "t"},
            "bastionMode": false
        }));
        OriginOptions::default().apply(&mut existing);
        assert_eq!(
            existing,
            map(json!({"access": {"required": true, "teamName": "t"}, "bastionMode": false}))
        );
        assert_eq!(
            OriginOptions::unknown_keys(&existing),
            ["access", "bastionMode"]
        );
        assert!(OriginOptions::from_map(&existing).is_default());
    }

    #[test]
    fn reads_go_durations_from_local_configs() {
        let options = OriginOptions::from_map(&map(json!({
            "connectTimeout": "30s",
            "tlsTimeout": "1m30s",
            "keepAliveTimeout": "2h",
            "tcpKeepAlive": "500ms"
        })));
        assert_eq!(options.connect_timeout, Some(30));
        assert_eq!(options.tls_timeout, Some(90));
        assert_eq!(options.keep_alive_timeout, Some(7200));
        assert_eq!(options.tcp_keep_alive, Some(1), "rounded up");
        for bad in ["", "10", "1d", "s", "0s"] {
            assert_eq!(seconds_of(&json!(bad)), None, "{bad}");
        }
    }

    #[test]
    fn validates_each_field() {
        let check = |options: OriginOptions| options.validated().map_err(|e| e.field);
        let ok = check(OriginOptions {
            http_host_header: Some("  localhost:8080 ".into()),
            origin_server_name: Some(String::new()),
            ..OriginOptions::default()
        })
        .unwrap();
        assert_eq!(ok.http_host_header.as_deref(), Some("localhost:8080"));
        assert_eq!(ok.origin_server_name, None, "empty text is dropped");
        assert_eq!(
            check(OriginOptions {
                http_host_header: Some("bad host".into()),
                ..OriginOptions::default()
            }),
            Err("httpHostHeader")
        );
        assert_eq!(
            check(OriginOptions {
                ca_pool: Some("certs/ca.pem".into()),
                ..OriginOptions::default()
            }),
            Err("caPool")
        );
        assert_eq!(
            check(OriginOptions {
                connect_timeout: Some(0),
                ..OriginOptions::default()
            }),
            Err("connectTimeout")
        );
        assert_eq!(
            check(OriginOptions {
                keep_alive_connections: Some(MAX_CONNECTIONS + 1),
                ..OriginOptions::default()
            }),
            Err("keepAliveConnections")
        );
        assert_eq!(
            check(OriginOptions {
                proxy_type: Some("http".into()),
                ..OriginOptions::default()
            }),
            Err("proxyType")
        );
    }

    fn arb_options() -> impl proptest::strategy::Strategy<Value = OriginOptions> {
        use proptest::prelude::*;
        let text = proptest::option::of("[a-z0-9.:-]{0,12}");
        let secs = proptest::option::of(0u32..100_000);
        (
            (text.clone(), text.clone(), text, any::<[bool; 5]>()),
            (secs.clone(), secs.clone(), secs.clone(), secs.clone(), secs),
        )
            .prop_map(|((host, name, ca, flags), (a, b, c, d, n))| OriginOptions {
                http_host_header: host,
                origin_server_name: name,
                match_sni_to_host: flags[0],
                no_tls_verify: flags[1],
                ca_pool: ca.map(|p| format!("/{p}")),
                http2_origin: flags[2],
                disable_chunked_encoding: flags[3],
                connect_timeout: a,
                tls_timeout: b,
                tcp_keep_alive: c,
                keep_alive_timeout: d,
                keep_alive_connections: n,
                no_happy_eyeballs: flags[4],
                proxy_type: None,
            })
    }

    proptest::proptest! {
        /// Written and read back, settings are the same; validating twice changes nothing.
        #[test]
        fn round_trips_and_validation_is_idempotent(options in arb_options()) {
            if let Ok(valid) = options.validated() {
                let mut map = Map::new();
                valid.apply(&mut map);
                proptest::prop_assert_eq!(&OriginOptions::from_map(&map), &valid);
                proptest::prop_assert_eq!(valid.validated().unwrap(), valid);
            }
        }

        /// Durations never panic, and whole seconds read as themselves.
        #[test]
        fn durations(input in ".{0,12}", seconds in 1u32..86_400) {
            let _ = seconds_of(&Value::String(input));
            proptest::prop_assert_eq!(seconds_of(&Value::String(format!("{seconds}s"))), Some(seconds));
            proptest::prop_assert_eq!(seconds_of(&serde_json::json!(seconds)), Some(seconds));
        }
    }
}
