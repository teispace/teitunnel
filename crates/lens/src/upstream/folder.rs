//! Static file server for folder upstreams.
//!
//! Safety: the request path is percent-decoded once, split into segments, and `..`,
//! empty and (by default) dot segments are refused before touching the file system;
//! the result is canonicalized (resolving symlinks) and must still lie inside the
//! canonical root. Files stream in 64 KiB chunks.

use std::{
    io,
    path::{Path, PathBuf},
    pin::Pin,
    task::{Context, Poll},
    time::SystemTime,
};

use bytes::Bytes;
use http::{HeaderMap, HeaderValue, Method, Response, StatusCode, Uri, header};
use http_body::{Body, Frame, SizeHint};
use http_body_util::BodyExt;
use tokio::io::{AsyncRead, AsyncSeekExt, ReadBuf};

use crate::{
    FolderConfig, LensBody, LensError,
    body::{BoxError, empty, full},
    pages,
    util::{hex_encode, html_escape},
};

const CHUNK: usize = 64 * 1024;
const MAX_LISTING: usize = 5_000;

/// A folder being served.
#[derive(Debug)]
pub(crate) struct Folder {
    config: FolderConfig,
    root: PathBuf,
}

impl Folder {
    pub(crate) fn new(config: FolderConfig) -> Result<Self, LensError> {
        let invalid = |reason: String| LensError::InvalidFolder {
            path: config.root.clone(),
            reason,
        };
        let root = std::fs::canonicalize(&config.root).map_err(|err| invalid(err.to_string()))?;
        if !root.is_dir() {
            return Err(invalid("it isn't a folder".into()));
        }
        Ok(Self { config, root })
    }

    /// Answers a request for `uri`.
    pub(crate) async fn serve(
        &self,
        method: &Method,
        uri: &Uri,
        headers: &HeaderMap,
    ) -> Response<LensBody> {
        if method != Method::GET && method != Method::HEAD {
            let mut response = plain(StatusCode::METHOD_NOT_ALLOWED, "Method not allowed");
            response
                .headers_mut()
                .insert(header::ALLOW, HeaderValue::from_static("GET, HEAD"));
            return response;
        }
        let head = method == Method::HEAD;
        let Some(segments) = self.segments(uri.path()) else {
            return not_found();
        };
        let mut path = self.root.clone();
        path.extend(&segments);
        match self.resolve(&path).await {
            Some((canonical, meta)) if meta.is_dir() => {
                if !uri.path().ends_with('/') {
                    let location = match uri.query() {
                        Some(query) => format!("{}/?{query}", uri.path()),
                        None => format!("{}/", uri.path()),
                    };
                    return redirect(&location);
                }
                if self.config.index {
                    let index = canonical.join("index.html");
                    if let Some((file, meta)) = self.resolve(&index).await
                        && meta.is_file()
                    {
                        return serve_file(&file, &meta, headers, head).await;
                    }
                }
                if self.config.listing {
                    return self.listing(&canonical, uri.path(), head).await;
                }
                not_found()
            }
            Some((canonical, meta)) => serve_file(&canonical, &meta, headers, head).await,
            None => {
                if self.config.spa_fallback && wants_html(uri.path(), headers) {
                    let index = self.root.join("index.html");
                    if let Some((file, meta)) = self.resolve(&index).await
                        && meta.is_file()
                    {
                        return serve_file(&file, &meta, headers, head).await;
                    }
                }
                not_found()
            }
        }
    }

    /// Safe path segments, or `None` when the path must not be served.
    fn segments(&self, raw_path: &str) -> Option<Vec<String>> {
        let decoded = percent_encoding::percent_decode_str(raw_path)
            .decode_utf8()
            .ok()?;
        if decoded.contains(['\0', '\\']) {
            return None;
        }
        let mut segments = Vec::new();
        for segment in decoded.split('/') {
            match segment {
                "" | "." => {}
                ".." => return None,
                s if s.starts_with('.') && !self.config.hidden && s != ".well-known" => {
                    return None;
                }
                // Windows drive letters and alternate data streams.
                s if s.contains(':') => return None,
                s => segments.push(s.to_owned()),
            }
        }
        Some(segments)
    }

    /// Canonicalizes `path` and checks it's inside the root.
    async fn resolve(&self, path: &Path) -> Option<(PathBuf, std::fs::Metadata)> {
        let canonical = tokio::fs::canonicalize(path).await.ok()?;
        if !canonical.starts_with(&self.root) {
            return None;
        }
        let meta = tokio::fs::metadata(&canonical).await.ok()?;
        Some((canonical, meta))
    }

    async fn listing(&self, dir: &Path, url_path: &str, head: bool) -> Response<LensBody> {
        let mut entries = Vec::new();
        if let Ok(mut reader) = tokio::fs::read_dir(dir).await {
            while let Ok(Some(entry)) = reader.next_entry().await {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') && !self.config.hidden {
                    continue;
                }
                let meta = entry.metadata().await.ok();
                let is_dir = meta.as_ref().is_some_and(std::fs::Metadata::is_dir);
                let size = meta.as_ref().map_or(0, std::fs::Metadata::len);
                entries.push((is_dir, name, size));
                if entries.len() >= MAX_LISTING {
                    break;
                }
            }
        }
        entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
        let mut rows = String::new();
        if url_path != "/" {
            rows.push_str("<li><a href=\"../\">../</a></li>\n");
        }
        for (is_dir, name, size) in &entries {
            let href: String =
                percent_encoding::utf8_percent_encode(name, percent_encoding::NON_ALPHANUMERIC)
                    .collect();
            let slash = if *is_dir { "/" } else { "" };
            let detail = if *is_dir {
                String::new()
            } else {
                format!(" <span class=\"size\">{}</span>", human_size(*size))
            };
            rows.push_str(&format!(
                "<li><a href=\"{href}{slash}\">{}{slash}</a>{detail}</li>\n",
                html_escape(name)
            ));
        }
        let title = format!("Index of {}", html_escape(url_path));
        let html = pages::document(
            &title,
            &format!("<h1>{title}</h1>\n<ul class=\"listing\">\n{rows}</ul>"),
        );
        let mut response = Response::new(if head { empty() } else { full(html) });
        let headers = response.headers_mut();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("text/html; charset=utf-8"),
        );
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        response
    }
}

fn wants_html(path: &str, headers: &HeaderMap) -> bool {
    let accepts_html = headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains("text/html"));
    let last = path.rsplit('/').next().unwrap_or_default();
    accepts_html || !last.contains('.')
}

fn human_size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let value = bytes as f64;
    match bytes {
        0..1_024 => format!("{bytes} B"),
        1_024..1_048_576 => format!("{:.1} KB", value / 1_024.0),
        1_048_576..1_073_741_824 => format!("{:.1} MB", value / 1_048_576.0),
        _ => format!("{:.1} GB", value / 1_073_741_824.0),
    }
}

fn plain(status: StatusCode, text: &'static str) -> Response<LensBody> {
    let mut response = Response::new(full(text));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn not_found() -> Response<LensBody> {
    plain(StatusCode::NOT_FOUND, "Not found")
}

fn redirect(location: &str) -> Response<LensBody> {
    let mut response = Response::new(empty());
    *response.status_mut() = StatusCode::MOVED_PERMANENTLY;
    if let Ok(value) = HeaderValue::from_str(location) {
        response.headers_mut().insert(header::LOCATION, value);
    }
    response
}

/// Validators for a file.
struct Validators {
    etag: String,
    modified: Option<SystemTime>,
}

impl Validators {
    fn of(meta: &std::fs::Metadata) -> Self {
        let modified = meta.modified().ok();
        let nanos = modified
            .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_nanos());
        let mut tag = meta.len().to_be_bytes().to_vec();
        tag.extend_from_slice(&nanos.to_be_bytes());
        let digest = ring::digest::digest(&ring::digest::SHA256, &tag);
        Self {
            etag: format!("\"{}\"", hex_encode(&digest.as_ref()[..8])),
            modified,
        }
    }

    /// `If-None-Match` / `If-Modified-Since` say the client's copy is current.
    fn not_modified(&self, headers: &HeaderMap) -> bool {
        if let Some(value) = headers
            .get(header::IF_NONE_MATCH)
            .and_then(|v| v.to_str().ok())
        {
            return value
                .split(',')
                .map(str::trim)
                .any(|tag| tag == "*" || tag.trim_start_matches("W/") == self.etag);
        }
        match (
            headers
                .get(header::IF_MODIFIED_SINCE)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| httpdate::parse_http_date(v).ok()),
            self.modified,
        ) {
            (Some(since), Some(modified)) => truncate_secs(modified) <= since,
            _ => false,
        }
    }

    /// `If-Range` allows a partial response.
    fn range_allowed(&self, headers: &HeaderMap) -> bool {
        let Some(value) = headers.get(header::IF_RANGE).and_then(|v| v.to_str().ok()) else {
            return true;
        };
        if value.starts_with('"') {
            return value == self.etag;
        }
        match (httpdate::parse_http_date(value).ok(), self.modified) {
            (Some(date), Some(modified)) => truncate_secs(modified) == date,
            _ => false,
        }
    }
}

fn truncate_secs(time: SystemTime) -> SystemTime {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(time, |d| {
            SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(d.as_secs())
        })
}

/// A parsed single byte range (inclusive).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Range {
    Satisfiable(u64, u64),
    Unsatisfiable,
    /// Not a single byte range Lens honours (multiple ranges, other units, malformed):
    /// serve the whole file, as RFC 9110 allows.
    Ignore,
}

fn parse_range(value: &str, len: u64) -> Range {
    let Some(spec) = value.trim().strip_prefix("bytes=") else {
        return Range::Ignore;
    };
    if spec.contains(',') {
        return Range::Ignore;
    }
    let Some((start, end)) = spec.trim().split_once('-') else {
        return Range::Ignore;
    };
    let (start, end) = (start.trim(), end.trim());
    let parse = |s: &str| s.parse::<u64>().ok();
    match (start.is_empty(), end.is_empty()) {
        (true, true) => Range::Ignore,
        (true, false) => match parse(end) {
            Some(0) => Range::Unsatisfiable,
            Some(_) if len == 0 => Range::Unsatisfiable,
            Some(n) => Range::Satisfiable(len.saturating_sub(n), len - 1),
            None => Range::Ignore,
        },
        (false, _) => {
            let Some(first) = parse(start) else {
                return Range::Ignore;
            };
            let last = if end.is_empty() {
                Some(len.saturating_sub(1))
            } else {
                parse(end)
            };
            match last {
                Some(_) if first >= len => Range::Unsatisfiable,
                Some(last) if last < first => Range::Ignore,
                Some(last) => Range::Satisfiable(first, last.min(len - 1)),
                None => Range::Ignore,
            }
        }
    }
}

async fn serve_file(
    path: &Path,
    meta: &std::fs::Metadata,
    headers: &HeaderMap,
    head: bool,
) -> Response<LensBody> {
    let len = meta.len();
    let validators = Validators::of(meta);
    let mut response = Response::new(empty());
    let out = response.headers_mut();
    if let Ok(etag) = HeaderValue::from_str(&validators.etag) {
        out.insert(header::ETAG, etag);
    }
    if let Some(modified) = validators.modified
        && let Ok(value) = HeaderValue::from_str(&httpdate::fmt_http_date(modified))
    {
        out.insert(header::LAST_MODIFIED, value);
    }
    out.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    out.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    out.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if validators.not_modified(headers) {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return response;
    }
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let content_type = if mime.type_() == "text"
        || mime.essence_str() == "application/javascript"
        || mime.essence_str() == "application/json"
    {
        format!("{}; charset=utf-8", mime.essence_str())
    } else {
        mime.essence_str().to_owned()
    };
    if let Ok(value) = HeaderValue::from_str(&content_type) {
        out.insert(header::CONTENT_TYPE, value);
    }
    let range = headers
        .get(header::RANGE)
        .and_then(|v| v.to_str().ok())
        .filter(|_| validators.range_allowed(headers))
        .map_or(Range::Ignore, |value| parse_range(value, len));
    let (status, start, count) = match range {
        Range::Satisfiable(first, last) => {
            if let Ok(value) = HeaderValue::from_str(&format!("bytes {first}-{last}/{len}")) {
                out.insert(header::CONTENT_RANGE, value);
            }
            (StatusCode::PARTIAL_CONTENT, first, last - first + 1)
        }
        Range::Unsatisfiable => {
            if let Ok(value) = HeaderValue::from_str(&format!("bytes */{len}")) {
                out.insert(header::CONTENT_RANGE, value);
            }
            *response.status_mut() = StatusCode::RANGE_NOT_SATISFIABLE;
            return response;
        }
        Range::Ignore => (StatusCode::OK, 0, len),
    };
    *response.status_mut() = status;
    response
        .headers_mut()
        .insert(header::CONTENT_LENGTH, HeaderValue::from(count));
    if head {
        return response;
    }
    match open_at(path, start).await {
        Ok(file) => {
            *response.body_mut() = FileBody::new(file, count).boxed_unsync();
            response
        }
        Err(err) => {
            tracing::debug!(error = %err, "couldn't read a served file");
            plain(StatusCode::INTERNAL_SERVER_ERROR, "Couldn't read the file")
        }
    }
}

async fn open_at(path: &Path, start: u64) -> io::Result<tokio::fs::File> {
    let mut file = tokio::fs::File::open(path).await?;
    if start > 0 {
        file.seek(io::SeekFrom::Start(start)).await?;
    }
    Ok(file)
}

/// Streams `remaining` bytes of a file.
struct FileBody {
    file: tokio::fs::File,
    remaining: u64,
    buf: Box<[u8]>,
}

impl FileBody {
    fn new(file: tokio::fs::File, remaining: u64) -> Self {
        Self {
            file,
            remaining,
            buf: vec![0; CHUNK].into_boxed_slice(),
        }
    }
}

impl Body for FileBody {
    type Data = Bytes;
    type Error = BoxError;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, BoxError>>> {
        let this = &mut *self;
        if this.remaining == 0 {
            return Poll::Ready(None);
        }
        let want = usize::try_from(this.remaining)
            .unwrap_or(usize::MAX)
            .min(CHUNK);
        let mut read_buf = ReadBuf::new(&mut this.buf[..want]);
        match Pin::new(&mut this.file).poll_read(cx, &mut read_buf) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(err)) => Poll::Ready(Some(Err(err.into()))),
            Poll::Ready(Ok(())) => {
                let filled = read_buf.filled();
                if filled.is_empty() {
                    // The file shrank while being served.
                    this.remaining = 0;
                    return Poll::Ready(Some(Err("the file ended early".into())));
                }
                this.remaining -= filled.len() as u64;
                Poll::Ready(Some(Ok(Frame::data(Bytes::copy_from_slice(filled)))))
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::with_exact(self.remaining)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(parse_range("bytes=0-9", 100), Range::Satisfiable(0, 9));
        assert_eq!(parse_range("bytes=90-", 100), Range::Satisfiable(90, 99));
        assert_eq!(parse_range("bytes=-10", 100), Range::Satisfiable(90, 99));
        assert_eq!(parse_range("bytes=-500", 100), Range::Satisfiable(0, 99));
        assert_eq!(parse_range("bytes=50-500", 100), Range::Satisfiable(50, 99));
        assert_eq!(parse_range("bytes=100-", 100), Range::Unsatisfiable);
        assert_eq!(parse_range("bytes=-0", 100), Range::Unsatisfiable);
        assert_eq!(parse_range("bytes=0-1,5-6", 100), Range::Ignore);
        assert_eq!(parse_range("items=0-1", 100), Range::Ignore);
        assert_eq!(parse_range("bytes=9-3", 100), Range::Ignore);
        assert_eq!(parse_range("bytes=x-3", 100), Range::Ignore);
        assert_eq!(parse_range("bytes=-", 100), Range::Ignore);
        assert_eq!(parse_range("bytes=0-", 0), Range::Unsatisfiable);
    }

    #[test]
    fn segments_refuse_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let folder = Folder::new(FolderConfig::new(dir.path())).unwrap();
        assert_eq!(
            folder.segments("/a/./b/"),
            Some(vec!["a".into(), "b".into()])
        );
        assert_eq!(folder.segments("/a/../b"), None);
        assert_eq!(folder.segments("/a/%2e%2e/b"), None);
        assert_eq!(folder.segments("/a%2f..%2fb"), None);
        assert_eq!(folder.segments("/.env"), None);
        assert_eq!(folder.segments("/.git/config"), None);
        assert!(folder.segments("/.well-known/x").is_some());
        assert_eq!(folder.segments("/a\\..\\b"), None);
        assert_eq!(folder.segments("/%00"), None);
        assert_eq!(folder.segments("/%ff"), None);
        assert_eq!(folder.segments("/C:/x"), None);
    }

    #[test]
    fn missing_root_is_an_error() {
        assert!(Folder::new(FolderConfig::new("/definitely/not/here")).is_err());
    }

    #[test]
    fn sizes() {
        assert_eq!(human_size(10), "10 B");
        assert_eq!(human_size(2_048), "2.0 KB");
        assert_eq!(human_size(3 * 1_048_576), "3.0 MB");
    }
}
