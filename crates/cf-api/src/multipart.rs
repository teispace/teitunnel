//! `multipart/form-data` bodies for Workers uploads (scripts, versions, static assets).
//! Built by hand: the parts are small in number and fully known, so a dependency isn't
//! worth it.

use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// One part of a form.
#[derive(Debug, Clone)]
pub(crate) struct Part<'a> {
    /// Field name.
    pub name: &'a str,
    /// File name, for file parts.
    pub filename: Option<&'a str>,
    /// Media type of the content.
    pub content_type: &'a str,
    /// The content.
    pub content: &'a [u8],
}

/// An encoded form with its `Content-Type` (which carries the boundary).
#[derive(Debug, Clone)]
pub(crate) struct Body {
    content_type: String,
    bytes: Vec<u8>,
}

impl Body {
    pub(crate) fn content_type(&self) -> &str {
        &self.content_type
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// Quotes a header parameter value (names are hashes and file names Teitunnel chose,
/// but never let one break out of its quotes).
fn quoted(value: &str) -> String {
    value.replace(['"', '\r', '\n'], "_")
}

/// Encodes `parts` with a boundary that appears in none of them.
pub(crate) fn encode(parts: &[Part<'_>]) -> Body {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut boundary;
    loop {
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        boundary = format!("teitunnel-{nanos:x}-{n:x}");
        if !parts
            .iter()
            .any(|p| contains(p.content, boundary.as_bytes()))
        {
            break;
        }
    }
    let mut bytes = Vec::new();
    for part in parts {
        bytes.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        let mut disposition = format!(
            "Content-Disposition: form-data; name=\"{}\"",
            quoted(part.name)
        );
        if let Some(filename) = part.filename {
            disposition.push_str(&format!("; filename=\"{}\"", quoted(filename)));
        }
        bytes.extend_from_slice(disposition.as_bytes());
        bytes.extend_from_slice(
            format!("\r\nContent-Type: {}\r\n\r\n", part.content_type).as_bytes(),
        );
        bytes.extend_from_slice(part.content);
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    Body {
        content_type: format!("multipart/form-data; boundary={boundary}"),
        bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_parts_with_a_unique_boundary() {
        let body = encode(&[
            Part {
                name: "metadata",
                filename: None,
                content_type: "application/json",
                content: br#"{"a":1}"#,
            },
            Part {
                name: "worker.js",
                filename: Some("worker.js"),
                content_type: "application/javascript+module",
                content: b"export default {}",
            },
        ]);
        let boundary = body
            .content_type()
            .strip_prefix("multipart/form-data; boundary=")
            .unwrap()
            .to_owned();
        let text = String::from_utf8(body.bytes().to_vec()).unwrap();
        assert_eq!(
            text,
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"metadata\"\r\nContent-Type: application/json\r\n\r\n{{\"a\":1}}\r\n\
                 --{boundary}\r\nContent-Disposition: form-data; name=\"worker.js\"; filename=\"worker.js\"\r\nContent-Type: application/javascript+module\r\n\r\nexport default {{}}\r\n\
                 --{boundary}--\r\n"
            )
        );
        assert_eq!(quoted("a\"b\r\nc"), "a_b__c");
    }
}
