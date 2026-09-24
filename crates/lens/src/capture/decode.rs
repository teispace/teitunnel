//! Body decoding for display: `Content-Encoding` decompression (bounded) and content
//! type detection.

use std::{borrow::Cow, io::Read};

use http::{HeaderMap, header};
use serde::{Deserialize, Serialize};

/// Decompressed bodies larger than this are cut off (a defence against zip bombs).
pub const MAX_DECODED_BYTES: usize = 8 * 1024 * 1024;

/// Why a body couldn't be decoded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    /// The `Content-Encoding` isn't one Lens understands.
    #[error("unsupported content encoding {0:?}")]
    Unsupported(String),
    /// The data is corrupt or cut off (a truncated capture can't always be decoded).
    #[error("the {0} data is corrupt or incomplete")]
    Corrupt(&'static str),
}

/// Decodes `data` per the `Content-Encoding` in `headers` (gzip, x-gzip, deflate, br,
/// identity; chained encodings are undone in reverse order).
///
/// Borrows when there's nothing to decode. Output is capped at [`MAX_DECODED_BYTES`].
/// A truncated capture decodes as far as the data allows when the format permits it.
///
/// # Errors
/// [`DecodeError`] for unknown encodings or corrupt data.
pub fn decode_body<'a>(headers: &HeaderMap, data: &'a [u8]) -> Result<Cow<'a, [u8]>, DecodeError> {
    let encodings: Vec<String> = headers
        .get_all(header::CONTENT_ENCODING)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty() && token != "identity")
        .collect();
    let mut out = Cow::Borrowed(data);
    for encoding in encodings.iter().rev() {
        out = Cow::Owned(decode_one(encoding, &out)?);
    }
    Ok(out)
}

fn decode_one(encoding: &str, data: &[u8]) -> Result<Vec<u8>, DecodeError> {
    match encoding {
        "gzip" | "x-gzip" => read_bounded(flate2::read::MultiGzDecoder::new(data), "gzip"),
        "deflate" => {
            // RFC 9110 says zlib-wrapped; some servers send raw deflate. Try both.
            read_bounded(flate2::read::ZlibDecoder::new(data), "deflate")
                .or_else(|_| read_bounded(flate2::read::DeflateDecoder::new(data), "deflate"))
        }
        "br" => read_bounded(brotli_decompressor::Decompressor::new(data, 4096), "brotli"),
        other => Err(DecodeError::Unsupported(other.to_owned())),
    }
}

fn read_bounded(reader: impl Read, name: &'static str) -> Result<Vec<u8>, DecodeError> {
    let mut out = Vec::new();
    let limit = u64::try_from(MAX_DECODED_BYTES).unwrap_or(u64::MAX);
    match reader.take(limit).read_to_end(&mut out) {
        Ok(_) => Ok(out),
        // A truncated capture ends mid-stream: keep what decoded cleanly.
        Err(_) if !out.is_empty() => Ok(out),
        Err(_) => Err(DecodeError::Corrupt(name)),
    }
}

/// A coarse classification of a body, for display and export choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContentKind {
    /// No body.
    Empty,
    /// JSON (including `+json` types).
    Json,
    /// HTML.
    Html,
    /// XML (including `+xml` types).
    Xml,
    /// `application/x-www-form-urlencoded`.
    Form,
    /// `multipart/*`.
    Multipart,
    /// `text/event-stream`.
    EventStream,
    /// JavaScript or CSS or other text.
    Text,
    /// An image.
    Image,
    /// Anything else.
    Binary,
}

impl ContentKind {
    /// Whether the body is text a person can read.
    pub fn is_text(self) -> bool {
        !matches!(self, Self::Image | Self::Binary)
    }
}

/// Classifies a (decoded) body from its `Content-Type`, falling back to sniffing.
pub fn content_kind(headers: &HeaderMap, decoded: &[u8]) -> ContentKind {
    if decoded.is_empty() {
        return ContentKind::Empty;
    }
    let mime = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| {
            value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
        })
        .unwrap_or_default();
    let from_mime = match mime.as_str() {
        "" | "application/octet-stream" => None,
        m if m == "application/json" || m.ends_with("+json") || m == "application/x-ndjson" => {
            Some(ContentKind::Json)
        }
        "text/html" | "application/xhtml+xml" => Some(ContentKind::Html),
        m if m.ends_with("/xml") || m.ends_with("+xml") => Some(ContentKind::Xml),
        "application/x-www-form-urlencoded" => Some(ContentKind::Form),
        m if m.starts_with("multipart/") => Some(ContentKind::Multipart),
        "text/event-stream" => Some(ContentKind::EventStream),
        m if m.starts_with("text/")
            || m == "application/javascript"
            || m == "application/graphql"
            || m == "application/x-yaml"
            || m == "application/yaml" =>
        {
            Some(ContentKind::Text)
        }
        m if m.starts_with("image/") => Some(ContentKind::Image),
        _ => Some(ContentKind::Binary),
    };
    match from_mime {
        Some(ContentKind::Binary) | None => sniff(decoded),
        Some(kind) => kind,
    }
}

fn sniff(data: &[u8]) -> ContentKind {
    const IMAGE_MAGIC: [&[u8]; 5] = [
        b"\x89PNG\r\n\x1a\n",
        b"\xff\xd8\xff",
        b"GIF87a",
        b"GIF89a",
        b"RIFF",
    ];
    if IMAGE_MAGIC.iter().any(|magic| data.starts_with(magic)) {
        return ContentKind::Image;
    }
    let head = &data[..data.len().min(512)];
    if std::str::from_utf8(head).is_err() && !is_cut_utf8(head) {
        return ContentKind::Binary;
    }
    if head.contains(&0) {
        return ContentKind::Binary;
    }
    let trimmed = head.trim_ascii_start();
    if trimmed.starts_with(b"{") || trimmed.starts_with(b"[") {
        if serde_json::from_slice::<serde_json::Value>(data).is_ok() {
            return ContentKind::Json;
        }
        return ContentKind::Text;
    }
    let lower: Vec<u8> = trimmed
        .iter()
        .take(16)
        .map(u8::to_ascii_lowercase)
        .collect();
    if lower.starts_with(b"<!doctype html") || lower.starts_with(b"<html") {
        return ContentKind::Html;
    }
    if lower.starts_with(b"<?xml") {
        return ContentKind::Xml;
    }
    ContentKind::Text
}

/// UTF-8 that is valid except for a multi-byte character cut at the end of the sample.
fn is_cut_utf8(data: &[u8]) -> bool {
    match std::str::from_utf8(data) {
        Ok(_) => true,
        Err(err) => err.error_len().is_none() && err.valid_up_to() + 4 > data.len(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &'static str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.append(*name, HeaderValue::from_static(value));
        }
        map
    }

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    #[test]
    fn identity_borrows() {
        let out = decode_body(&HeaderMap::new(), b"plain").unwrap();
        assert!(matches!(out, Cow::Borrowed(_)));
    }

    #[test]
    fn decodes_gzip_and_deflate() {
        let data = b"hello hello hello hello";
        let compressed = gzip(data);
        let out = decode_body(&headers(&[("content-encoding", "gzip")]), &compressed).unwrap();
        assert_eq!(&*out, data);

        let mut zlib = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        zlib.write_all(data).unwrap();
        let zlib = zlib.finish().unwrap();
        let out = decode_body(&headers(&[("content-encoding", "deflate")]), &zlib).unwrap();
        assert_eq!(&*out, data);

        let mut raw = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
        raw.write_all(data).unwrap();
        let raw = raw.finish().unwrap();
        let out = decode_body(&headers(&[("content-encoding", "deflate")]), &raw).unwrap();
        assert_eq!(&*out, data);
    }

    #[test]
    fn decodes_brotli() {
        // "hello" compressed with `brotli -q 11`.
        let compressed = [0x21, 0x10, 0x00, 0x04, 0x68, 0x65, 0x6c, 0x6c, 0x6f, 0x03];
        let out = decode_body(&headers(&[("content-encoding", "br")]), &compressed).unwrap();
        assert_eq!(&*out, b"hello");
    }

    #[test]
    fn chained_encodings_decode_in_reverse() {
        let twice = gzip(&gzip(b"layered"));
        let out = decode_body(&headers(&[("content-encoding", "gzip, gzip")]), &twice).unwrap();
        assert_eq!(&*out, b"layered");
    }

    #[test]
    fn unknown_and_corrupt() {
        assert_eq!(
            decode_body(&headers(&[("content-encoding", "zstd")]), b"x"),
            Err(DecodeError::Unsupported("zstd".into()))
        );
        assert!(decode_body(&headers(&[("content-encoding", "gzip")]), b"not gzip").is_err());
    }

    #[test]
    fn truncated_gzip_keeps_the_prefix() {
        let data = "abcdefghij".repeat(10_000);
        let compressed = gzip(data.as_bytes());
        let cut = &compressed[..compressed.len() / 2];
        let out = decode_body(&headers(&[("content-encoding", "gzip")]), cut).unwrap();
        assert!(!out.is_empty());
        assert!(data.as_bytes().starts_with(&out));
    }

    #[test]
    fn bomb_is_capped() {
        let zeros = vec![0u8; MAX_DECODED_BYTES + 1024];
        let compressed = gzip(&zeros);
        let out = decode_body(&headers(&[("content-encoding", "gzip")]), &compressed).unwrap();
        assert_eq!(out.len(), MAX_DECODED_BYTES);
    }

    #[test]
    fn classifies_content() {
        let json = headers(&[("content-type", "application/vnd.api+json; charset=utf-8")]);
        assert_eq!(content_kind(&json, b"{}"), ContentKind::Json);
        assert_eq!(content_kind(&HeaderMap::new(), b""), ContentKind::Empty);
        assert_eq!(
            content_kind(&HeaderMap::new(), b" {\"a\":1}"),
            ContentKind::Json
        );
        assert_eq!(
            content_kind(&HeaderMap::new(), b"<!DOCTYPE html><html>"),
            ContentKind::Html
        );
        assert_eq!(
            content_kind(&HeaderMap::new(), b"\x89PNG\r\n\x1a\n...."),
            ContentKind::Image
        );
        assert_eq!(
            content_kind(&HeaderMap::new(), b"\x00\x01\x02"),
            ContentKind::Binary
        );
        let octet = headers(&[("content-type", "application/octet-stream")]);
        assert_eq!(content_kind(&octet, b"just text"), ContentKind::Text);
        let form = headers(&[("content-type", "application/x-www-form-urlencoded")]);
        assert_eq!(content_kind(&form, b"a=1"), ContentKind::Form);
        let sse = headers(&[("content-type", "text/event-stream")]);
        assert_eq!(content_kind(&sse, b"data: x\n\n"), ContentKind::EventStream);
    }
}
