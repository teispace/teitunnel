//! Parsing cloudflared's `--output json` log lines.
//!
//! Every line becomes a [`LogEvent`]. Unknown fields are kept, and lines that aren't
//! JSON (early startup output, Go panics) become `Level::Raw` events instead of being
//! dropped. Known messages are classified into [`EventKind`] so the supervisor and
//! Doctor can react to them without string matching of their own.

use std::sync::LazyLock;

use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value};

/// Log severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Level {
    /// Verbose diagnostics.
    Debug,
    /// Normal operation.
    Info,
    /// Something unexpected but recoverable.
    Warn,
    /// A failed operation.
    Error,
    /// cloudflared is about to exit.
    Fatal,
    /// A non-JSON line.
    Raw,
}

/// What a log line means for the connector, when we recognise it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum EventKind {
    /// An edge connection was registered.
    ConnectionRegistered {
        /// Connection slot (0–3).
        conn_index: u8,
        /// Edge location code, e.g. `ams01`.
        location: Option<String>,
        /// `quic` or `http2`.
        protocol: Option<String>,
    },
    /// An edge connection closed.
    ConnectionLost {
        /// Connection slot, when reported.
        conn_index: Option<u8>,
    },
    /// cloudflared is retrying a connection.
    Retrying,
    /// A Quick Share URL was announced.
    QuickTunnelUrl {
        /// The public URL.
        url: String,
    },
    /// A request couldn't reach the local origin.
    OriginUnreachable,
    /// cloudflared started shutting down.
    ShuttingDown,
    /// Anything else.
    Other,
}

/// One parsed log line.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEvent {
    /// Timestamp as printed by cloudflared (RFC 3339), if present.
    pub time: Option<String>,
    /// Severity.
    pub level: Level,
    /// The message (the whole line for raw events).
    pub message: String,
    /// The `error` field, when present.
    pub error: Option<String>,
    /// Classification of well-known messages.
    pub kind: EventKind,
    /// Remaining structured fields.
    pub fields: Map<String, Value>,
}

static QUICK_URL: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"https://[a-z0-9-]+\.trycloudflare\.com").ok());

/// Parses one line of cloudflared output. Never fails: malformed input becomes `Raw`.
pub fn parse_line(line: &str) -> LogEvent {
    let trimmed = line.trim_end();
    let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(trimmed) else {
        return raw(trimmed);
    };
    let mut take = |key: &str| match fields.remove(key) {
        Some(Value::String(value)) => Some(value),
        Some(other) => Some(other.to_string()),
        None => None,
    };
    let level = match take("level").as_deref() {
        Some("debug" | "trace") => Level::Debug,
        Some("warn" | "warning") => Level::Warn,
        Some("error") => Level::Error,
        Some("fatal" | "panic") => Level::Fatal,
        _ => Level::Info,
    };
    let message = take("message").unwrap_or_default();
    let time = take("time");
    let error = take("error");
    let kind = classify(&message, error.as_deref(), &fields);
    LogEvent {
        time,
        level,
        message,
        error,
        kind,
        fields,
    }
}

fn raw(line: &str) -> LogEvent {
    LogEvent {
        time: None,
        level: Level::Raw,
        message: line.to_owned(),
        error: None,
        kind: EventKind::Other,
        fields: Map::new(),
    }
}

fn conn_index(fields: &Map<String, Value>) -> Option<u8> {
    fields
        .get("connIndex")
        .and_then(Value::as_u64)
        .and_then(|index| u8::try_from(index).ok())
}

fn string_field(fields: &Map<String, Value>, key: &str) -> Option<String> {
    fields.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn classify(message: &str, error: Option<&str>, fields: &Map<String, Value>) -> EventKind {
    if message.starts_with("Registered tunnel connection")
        && let Some(conn_index) = conn_index(fields)
    {
        return EventKind::ConnectionRegistered {
            conn_index,
            location: string_field(fields, "location"),
            protocol: string_field(fields, "protocol"),
        };
    }
    if message.starts_with("Unregistered tunnel connection") || message == "Connection terminated" {
        return EventKind::ConnectionLost {
            conn_index: conn_index(fields),
        };
    }
    if message.starts_with("Retrying connection") {
        return EventKind::Retrying;
    }
    if message.starts_with("Initiating graceful shutdown") {
        return EventKind::ShuttingDown;
    }
    if error.is_some_and(|e| e.contains("Unable to reach the origin service")) {
        return EventKind::OriginUnreachable;
    }
    if let Some(url) = QUICK_URL.as_ref().and_then(|re| re.find(message)) {
        return EventKind::QuickTunnelUrl {
            url: url.as_str().to_owned(),
        };
    }
    EventKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUICK_TUNNEL: &str = include_str!("../fixtures/2026.9.1/quick-tunnel.jsonl");

    #[test]
    fn parses_every_line_of_a_real_quick_tunnel_run() {
        let events: Vec<_> = QUICK_TUNNEL.lines().map(parse_line).collect();
        assert_eq!(events.len(), QUICK_TUNNEL.lines().count());
        assert!(
            events.iter().all(|e| e.level != Level::Raw),
            "all lines are JSON"
        );
        assert!(events.iter().all(|e| e.time.is_some()));

        let kinds: Vec<_> = events.iter().map(|e| &e.kind).collect();
        assert!(kinds.contains(&&EventKind::QuickTunnelUrl {
            url: "https://quiet-river-lamp-orbit.trycloudflare.com".into()
        }));
        assert!(kinds.contains(&&EventKind::ConnectionRegistered {
            conn_index: 0,
            location: Some("ktm01".into()),
            protocol: Some("quic".into()),
        }));
        assert!(kinds.contains(&&EventKind::ShuttingDown));
        assert!(kinds.contains(&&EventKind::ConnectionLost {
            conn_index: Some(0)
        }));
        assert!(kinds.contains(&&EventKind::Retrying));
    }

    #[test]
    fn keeps_unknown_fields() {
        let event = parse_line(
            r#"{"component":"DNS Resolution","level":"info","message":"precheck","status":"pass","time":"t"}"#,
        );
        assert_eq!(event.fields["component"], "DNS Resolution");
        assert_eq!(event.fields["status"], "pass");
        assert!(!event.fields.contains_key("message"));
    }

    #[test]
    fn classifies_origin_errors() {
        let event = parse_line(
            r#"{"level":"error","error":"Unable to reach the origin service. The service may be down or it may not be responding to traffic from cloudflared: dial tcp [::1]:3000: connect: connection refused","originService":"http://localhost:3000","message":"Request failed","time":"t"}"#,
        );
        assert_eq!(event.level, Level::Error);
        assert_eq!(event.kind, EventKind::OriginUnreachable);
        assert_eq!(event.fields["originService"], "http://localhost:3000");
    }

    #[test]
    fn non_json_lines_become_raw() {
        for line in [
            "panic: runtime error: index out of range",
            "",
            "{not json",
            "[1,2]",
        ] {
            let event = parse_line(line);
            assert_eq!(event.level, Level::Raw);
            assert_eq!(event.message, line);
        }
    }

    #[test]
    fn tolerates_odd_types() {
        let event = parse_line(r#"{"level":7,"message":{"nested":true},"connIndex":"x"}"#);
        assert_eq!(event.level, Level::Info);
        assert_eq!(event.message, r#"{"nested":true}"#);
        assert_eq!(event.kind, EventKind::Other);
    }

    proptest::proptest! {
        #[test]
        fn never_panics(line in ".*") {
            let _ = parse_line(&line);
        }

        #[test]
        fn never_panics_on_json_objects(
            level in ".{0,8}",
            message in ".{0,40}",
            conn in proptest::option::of(0u64..300),
        ) {
            let mut map = serde_json::Map::new();
            map.insert("level".into(), level.into());
            map.insert("message".into(), message.into());
            if let Some(conn) = conn {
                map.insert("connIndex".into(), conn.into());
            }
            let _ = parse_line(&serde_json::Value::Object(map).to_string());
        }
    }
}
