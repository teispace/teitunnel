//! The captured exchange: what Lens records about one request and its response.
//!
//! [`Exchange`] holds raw data (headers and bodies as received) so it can be replayed
//! exactly. Anything shown to people or agents goes through [`Exchange::view`] (or an
//! [`crate::export`] function), which takes a [`Redaction`] choice.

use std::{collections::VecDeque, net::IpAddr, time::Duration};

use bytes::Bytes;
use http::{HeaderMap, Method, StatusCode, Uri, Version};
use serde::{Deserialize, Serialize};

use crate::{ExchangeId, TapId};

pub(crate) mod decode;
mod view;

pub use decode::{ContentKind, DecodeError, content_kind, decode_body};
pub use view::{BodyView, ExchangeView, HeaderView, Redaction, RequestView, ResponseView};

/// What kind of traffic an exchange carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ExchangeKind {
    /// A plain request and response.
    Http,
    /// An upgraded WebSocket connection.
    WebSocket,
    /// A server-sent event stream (`text/event-stream`).
    Sse,
    /// Another protocol upgrade (e.g. `h2c`, a custom `Upgrade:` token).
    Upgrade,
}

/// Where an exchange is in its life.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ExchangeState {
    /// Received; waiting for the upstream's response head.
    Pending,
    /// The response head was sent; the body (or the upgraded stream) is flowing.
    Streaming,
    /// Finished normally.
    Complete,
    /// Ended with an error ([`Exchange::error`]).
    Failed,
}

/// Who produced the response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Responder {
    /// The origin server.
    Upstream,
    /// The static folder server.
    Folder,
    /// A stub rule (`rule` is its index in the tap's stub list, `fallback` when it
    /// answered because the upstream was unreachable).
    Stub {
        /// Index of the rule in [`crate::TapConfig::stubs`].
        #[cfg_attr(feature = "specta", specta(type = u32))]
        rule: usize,
        /// Whether the rule answered only because the upstream was unreachable.
        fallback: bool,
    },
    /// A gate refused or challenged the request.
    Gate {
        /// Why.
        reason: GateOutcome,
    },
    /// The tap is paused and served its paused page.
    Paused,
    /// A fault rule answered (a status or a simulated timeout).
    Fault {
        /// Index of the rule in [`crate::TapConfig::faults`].
        #[cfg_attr(feature = "specta", specta(type = u32))]
        rule: usize,
    },
    /// Lens itself (reserved `/__teitunnel/` paths, CORS preflight, error pages).
    Lens,
}

/// Why a gate stopped a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum GateOutcome {
    /// The client IP isn't on the allow list.
    IpNotAllowed,
    /// The client IP is on the deny list.
    IpDenied,
    /// The user agent is blocked.
    UserAgentBlocked,
    /// The password page was shown.
    PasswordRequired,
    /// A wrong password was submitted.
    PasswordWrong,
    /// Too many wrong passwords from this IP.
    RateLimited,
    /// The visitor signed in; Lens set the session cookie.
    SignedIn,
    /// A secret link was used; Lens set the cookie and redirected without the key.
    LinkAccepted,
    /// A secret link is required and none (or a wrong one) was given.
    LinkRequired,
    /// HTTP basic credentials are missing or wrong.
    BasicAuthRequired,
    /// A bearer token is missing or wrong.
    BearerRequired,
}

/// Per-phase timings, relative to when Lens received the request head.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Timings {
    /// A connection to the upstream was ready (new or reused), in microseconds.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub upstream_connected_us: Option<u64>,
    /// The response head arrived (time to first byte), in microseconds.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub first_byte_us: Option<u64>,
    /// The request body finished arriving, in microseconds.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub request_done_us: Option<u64>,
    /// The exchange finished (response body or upgraded stream ended), in microseconds.
    #[cfg_attr(feature = "specta", specta(type = Option<u32>))]
    pub complete_us: Option<u64>,
}

/// Who sent the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ClientInfo {
    /// The visitor's IP: `CF-Connecting-IP` when present and valid, else the peer.
    pub ip: IpAddr,
    /// The TCP peer (usually cloudflared on loopback).
    pub peer: std::net::SocketAddr,
    /// Cloudflare's `CF-Ray` id, when present.
    pub cf_ray: Option<String>,
    /// Cloudflare's `CF-IPCountry`, when present.
    pub country: Option<String>,
}

/// A captured body: the first bytes up to the tap's cap, and the true size.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BodyRecord {
    /// Captured bytes, as sent on the wire (still compressed if `Content-Encoding` says so).
    pub data: Bytes,
    /// Total bytes seen on the wire.
    pub size: u64,
    /// Whether `data` is shorter than `size` (the cap was reached).
    pub truncated: bool,
    /// Whether the body has finished (false while streaming, or if it was cut off).
    pub complete: bool,
}

impl BodyRecord {
    /// An empty, complete body.
    pub fn empty() -> Self {
        Self {
            complete: true,
            ..Self::default()
        }
    }

    /// A complete body held in full.
    pub fn full(data: Bytes) -> Self {
        Self {
            size: data.len() as u64,
            data,
            truncated: false,
            complete: true,
        }
    }
}

/// The request as received.
#[derive(Debug, Clone, PartialEq)]
pub struct RequestRecord {
    /// Method.
    pub method: Method,
    /// Path and query as received (origin form).
    pub uri: Uri,
    /// `https` when cloudflared says so (`X-Forwarded-Proto`), else the listener's scheme.
    pub scheme: String,
    /// Host the visitor asked for (`Host` header or `:authority`), without changes.
    pub host: String,
    /// HTTP version between cloudflared and Lens.
    pub version: Version,
    /// Headers as received.
    pub headers: HeaderMap,
    /// Body.
    pub body: BodyRecord,
}

impl RequestRecord {
    /// The request path without the query.
    pub fn path(&self) -> &str {
        self.uri.path()
    }

    /// The raw query string, if any.
    pub fn query(&self) -> Option<&str> {
        self.uri.query()
    }

    /// The full URL the visitor used: `scheme://host/path?query`.
    pub fn url(&self) -> String {
        let path_and_query = self
            .uri
            .path_and_query()
            .map_or("/", http::uri::PathAndQuery::as_str);
        format!("{}://{}{}", self.scheme, self.host, path_and_query)
    }
}

/// The response as sent to the client (before Lens's HTML injection, if any).
#[derive(Debug, Clone, PartialEq)]
pub struct ResponseRecord {
    /// Status code.
    pub status: StatusCode,
    /// HTTP version from the upstream.
    pub version: Version,
    /// Headers.
    pub headers: HeaderMap,
    /// Body.
    pub body: BodyRecord,
}

/// Why an exchange failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum ErrorKind {
    /// Nothing listens on the upstream port.
    ConnectionRefused,
    /// Connecting or waiting for the response took too long.
    Timeout,
    /// The upstream closed or reset the connection mid-exchange.
    ConnectionReset,
    /// The upstream's hostname didn't resolve.
    Dns,
    /// The TLS handshake with the upstream failed.
    Tls,
    /// The upstream sent something that isn't valid HTTP.
    Protocol,
    /// The client went away before the exchange finished.
    ClientAborted,
    /// A static file couldn't be read.
    Io,
    /// Anything else.
    Other,
}

impl ErrorKind {
    /// Whether the upstream was unreachable before any response (stub fallback applies).
    pub fn is_unreachable(self) -> bool {
        matches!(
            self,
            Self::ConnectionRefused | Self::Timeout | Self::Dns | Self::Tls
        )
    }
}

/// An error recorded on an exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ExchangeError {
    /// Category.
    pub kind: ErrorKind,
    /// Technical detail (English, for logs and developers).
    pub message: String,
}

/// Which way a stream message went.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum Direction {
    /// From the visitor to the origin.
    ClientToServer,
    /// From the origin to the visitor.
    ServerToClient,
}

/// Kind of a stream message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum MessageKind {
    /// WebSocket text message.
    Text,
    /// WebSocket binary message.
    Binary,
    /// WebSocket ping.
    Ping,
    /// WebSocket pong.
    Pong,
    /// WebSocket close.
    Close,
    /// A server-sent event.
    Event,
}

/// The beginning of one stream message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MessagePreview {
    /// When it finished, relative to the request (microseconds).
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub at_us: u64,
    /// Direction.
    pub direction: Direction,
    /// Kind.
    pub kind: MessageKind,
    /// Full size in bytes.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub size: u64,
    /// The first bytes (text is lossy UTF-8).
    pub preview: String,
    /// Whether `preview` is shorter than the message.
    pub truncated: bool,
    /// WebSocket `permessage-deflate`: the message was compressed on the wire.
    pub compressed: bool,
    /// For compressed messages: whether `preview` shows the inflated text (otherwise
    /// the preview is unavailable and empty).
    pub inflated: bool,
}

/// A WebSocket frame opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub enum FrameOpcode {
    /// Continues a fragmented message.
    Continuation,
    /// Text.
    Text,
    /// Binary.
    Binary,
    /// Close.
    Close,
    /// Ping.
    Ping,
    /// Pong.
    Pong,
}

/// One WebSocket frame, observed without altering it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct FrameRecord {
    /// When the frame finished, relative to the request (microseconds).
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub at_us: u64,
    /// Direction.
    pub direction: Direction,
    /// Opcode.
    pub opcode: FrameOpcode,
    /// Final fragment of its message.
    pub fin: bool,
    /// Masked on the wire (client frames are).
    pub masked: bool,
    /// `RSV1` set: compressed with `permessage-deflate`.
    pub compressed: bool,
    /// Payload size in bytes (compressed size for compressed frames).
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub size: u64,
    /// The first payload bytes, unmasked: UTF-8 text for text data, hex otherwise.
    /// `None` when unavailable (compressed frames; see the message previews).
    pub preview: Option<String>,
    /// Whether `preview` is shorter than the payload.
    pub truncated: bool,
    /// Close frames: the status code.
    pub close_code: Option<u16>,
    /// Close frames: the reason.
    pub close_reason: Option<String>,
}

/// Counters for one direction of a stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MessageCounts {
    /// Messages.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub count: u64,
    /// Payload bytes.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub bytes: u64,
}

/// Summary of a WebSocket or SSE stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct StreamStats {
    /// Messages from the visitor.
    pub client: MessageCounts,
    /// Messages from the origin.
    pub server: MessageCounts,
    /// The first messages, up to the tap's preview limit.
    pub previews: Vec<MessagePreview>,
    /// WebSocket frames: the most recent ones, up to the tap's frame limit.
    pub frames: VecDeque<FrameRecord>,
    /// Older frames dropped from `frames` to respect the limit.
    #[cfg_attr(feature = "specta", specta(type = u32))]
    pub frames_dropped: u64,
    /// Whether the stream has ended.
    pub closed: bool,
}

/// One captured request/response exchange.
#[derive(Debug, Clone, PartialEq)]
pub struct Exchange {
    /// Unique id.
    pub id: ExchangeId,
    /// Sequence number within the tap, from 1.
    pub seq: u64,
    /// The tap that captured it.
    pub tap: TapId,
    /// HTTP, WebSocket, SSE or another upgrade.
    pub kind: ExchangeKind,
    /// Life-cycle state.
    pub state: ExchangeState,
    /// When the request head arrived (Unix milliseconds).
    pub started_at_ms: u64,
    /// Phase timings.
    pub timings: Timings,
    /// Who sent it.
    pub client: ClientInfo,
    /// The request.
    pub request: RequestRecord,
    /// The response, once its head is known.
    pub response: Option<ResponseRecord>,
    /// Who answered.
    pub responder: Responder,
    /// What went wrong, if anything.
    pub error: Option<ExchangeError>,
    /// WebSocket/SSE summary.
    pub stream: Option<StreamStats>,
    /// The exchange this one replays.
    pub replay_of: Option<ExchangeId>,
    /// The fault rule applied to it, if any.
    pub fault: Option<crate::FaultRecord>,
}

impl Exchange {
    /// Response status, when known.
    pub fn status(&self) -> Option<StatusCode> {
        self.response.as_ref().map(|response| response.status)
    }

    /// Total duration so far (complete time, else first byte), when known.
    pub fn duration(&self) -> Option<Duration> {
        self.timings
            .complete_us
            .or(self.timings.first_byte_us)
            .map(Duration::from_micros)
    }

    /// Whether the exchange has finished (complete or failed).
    pub fn is_finished(&self) -> bool {
        matches!(self.state, ExchangeState::Complete | ExchangeState::Failed)
    }

    /// A display- and export-ready copy with secrets handled per `redaction`.
    pub fn view(&self, redaction: &Redaction) -> ExchangeView {
        view::build(self, redaction)
    }
}
