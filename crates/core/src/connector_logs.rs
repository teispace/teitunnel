//! Connector log files and per-route filtering.
//!
//! Always-on connectors log to a file launchd appends to. Nothing rotates it,
//! so the app keeps it bounded ([`rotate_if_large`]) and reads only its tail
//! ([`tail_lines`]), never the whole file.
//!
//! cloudflared tags every request-scoped event with the matched ingress rule's index
//! (`ingressRule`) and service (`originService`) (`proxy/logger.go`, verified
//! 2026-09-23), so a route's events are found by both ([`RouteFilter`]): the index alone
//! could name a different route in lines logged before the config changed.

use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

use cf_api::IngressRule;
use cloudflared::LogEvent;

/// A log file grows to this before it's cut back.
pub const MAX_BYTES: u64 = 8 * 1024 * 1024;
/// How much is read at a time when tailing, from the end backwards.
const BLOCK: u64 = 64 * 1024;
/// The most read from the end of a file for one tail, whatever `limit` asks.
const MAX_TAIL_BYTES: u64 = 2 * 1024 * 1024;

/// The last `limit` lines of the file at `path` (fewer if it's shorter; none if it's
/// missing). Reads backwards in blocks, at most 2 MiB.
pub fn tail_lines(path: &Path, limit: usize) -> Vec<String> {
    let Ok(mut file) = File::open(path) else {
        return Vec::new();
    };
    let Ok(len) = file.seek(SeekFrom::End(0)) else {
        return Vec::new();
    };
    let mut start = len;
    let mut buf: Vec<u8> = Vec::new();
    // One extra line, since the first one read is probably cut off.
    while start > 0 && len - start < MAX_TAIL_BYTES {
        let size = BLOCK.min(start);
        start -= size;
        let mut block = vec![0; usize::try_from(size).unwrap_or(0)];
        if file.seek(SeekFrom::Start(start)).is_err() || file.read_exact(&mut block).is_err() {
            return Vec::new();
        }
        block.extend_from_slice(&buf);
        buf = block;
        if buf.iter().filter(|b| **b == b'\n').count() > limit {
            break;
        }
    }
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<&str> = text.lines().collect();
    if start > 0 && !lines.is_empty() {
        lines.remove(0);
    }
    lines[lines.len().saturating_sub(limit)..]
        .iter()
        .map(|l| (*l).to_owned())
        .collect()
}

/// Cuts the log at `path` back once it passes [`MAX_BYTES`]: its current content moves
/// to `<path>.1` (replacing the previous one) and the file is emptied in place. The
/// writer appends, so it carries on at the new end; renaming the file instead would
/// leave launchd writing to the renamed one. Returns whether it rotated.
///
/// # Errors
/// File system errors.
pub fn rotate_if_large(path: &Path) -> std::io::Result<bool> {
    let len = match std::fs::metadata(path) {
        Ok(meta) => meta.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    if len <= MAX_BYTES {
        return Ok(false);
    }
    let mut old = path.as_os_str().to_owned();
    old.push(".1");
    std::fs::copy(path, &old)?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(path)?
        .set_len(0)?;
    Ok(true)
}

/// Picks out the events about requests for one route.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteFilter {
    index: u64,
    service: String,
}

impl RouteFilter {
    /// A filter for the route `hostname` + `path` in the ingress Teitunnel applied; None
    /// if the route isn't in it.
    pub fn new(ingress: &[IngressRule], hostname: &str, path: Option<&str>) -> Option<Self> {
        ingress.iter().enumerate().find_map(|(index, rule)| {
            (rule.hostname.as_deref() == Some(hostname) && rule.path.as_deref() == path).then(
                || Self {
                    index: u64::try_from(index).unwrap_or(u64::MAX),
                    service: rule.service.clone(),
                },
            )
        })
    }

    /// Whether `event` is about a request this route served.
    pub fn matches(&self, event: &LogEvent) -> bool {
        event
            .fields
            .get("ingressRule")
            .and_then(serde_json::Value::as_u64)
            == Some(self.index)
            && event.fields.get("originService").and_then(|v| v.as_str())
                == Some(self.service.as_str())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use serde_json::json;

    use super::*;

    #[test]
    fn tails_without_reading_everything() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.log");
        assert!(tail_lines(&path, 5).is_empty(), "missing file");
        let mut file = File::create(&path).unwrap();
        for i in 0..50_000 {
            writeln!(file, "line {i}").unwrap();
        }
        assert_eq!(
            tail_lines(&path, 3),
            ["line 49997", "line 49998", "line 49999"]
        );
        let many = tail_lines(&path, 20_000);
        assert_eq!(many.len(), 20_000);
        assert_eq!(many[0], "line 30000");
    }

    #[test]
    fn never_reads_more_than_the_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.log");
        let mut file = File::create(&path).unwrap();
        let padding = "x".repeat(90);
        for i in 0..40_000 {
            writeln!(file, "line {i:05} {padding}").unwrap();
        }
        // ~4 MB on disk; asking for everything returns only what fits in 2 MiB, and only
        // whole lines.
        let capped = tail_lines(&path, 1_000_000);
        assert!(capped.len() < 40_000);
        assert!(capped.len() > 15_000);
        assert!(
            capped
                .iter()
                .all(|l| l.starts_with("line ") && l.ends_with('x'))
        );
        assert_eq!(capped.last().unwrap(), &format!("line 39999 {padding}"));
    }

    #[test]
    fn short_files_come_back_whole() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.log");
        std::fs::write(&path, "a\nb\n").unwrap();
        assert_eq!(tail_lines(&path, 10), ["a", "b"]);
    }

    #[test]
    fn rotates_in_place_once_large() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.log");
        assert!(!rotate_if_large(&path).unwrap());
        std::fs::write(&path, "small\n").unwrap();
        assert!(!rotate_if_large(&path).unwrap());

        // A writer that appends (like launchd) keeps writing to the same file.
        let mut writer = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writer
            .write_all(&vec![b'x'; usize::try_from(MAX_BYTES).unwrap()])
            .unwrap();
        assert!(rotate_if_large(&path).unwrap());
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0);
        assert!(std::fs::metadata(dir.path().join("t.log.1")).unwrap().len() > MAX_BYTES);
        writer.write_all(b"after\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after\n");
    }

    fn event(rule: u64, service: &str) -> LogEvent {
        cloudflared::parse_line(
            &json!({ "level": "error", "error": "unreachable", "ingressRule": rule,
                     "originService": service, "message": "" })
            .to_string(),
        )
    }

    #[test]
    fn finds_a_routes_events_by_rule_and_service() {
        let ingress: Vec<IngressRule> = serde_json::from_value(json!([
            { "hostname": "a.xyz.com", "service": "http://localhost:3000" },
            { "hostname": "a.xyz.com", "path": "^/api", "service": "http://localhost:8000" },
            { "service": "http_status:404" }
        ]))
        .unwrap();
        let events = [
            event(0, "http://localhost:3000"),
            event(1, "http://localhost:8000"),
            // Logged before a config change moved the rules around.
            event(1, "http://localhost:3000"),
            cloudflared::parse_line("Registered tunnel connection"),
        ];
        let count = |path| {
            let filter = RouteFilter::new(&ingress, "a.xyz.com", path).unwrap();
            events.iter().filter(|e| filter.matches(e)).count()
        };
        assert_eq!(count(Some("^/api")), 1);
        assert_eq!(count(None), 1);
        assert!(RouteFilter::new(&ingress, "b.xyz.com", None).is_none());
    }
}
