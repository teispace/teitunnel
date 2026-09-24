//! Lens: Teitunnel's local inspecting reverse proxy.
//!
//! Lens sits between cloudflared and the user's origin (visitor → Cloudflare edge →
//! cloudflared → **Lens on `127.0.0.1`** → origin) and records every exchange while
//! streaming bodies through untouched. It's a pure library: the desktop app, the CLI
//! and the server embed it; it knows nothing about Tauri or Teitunnel's store.
//!
//! - **Taps** ([`TapConfig`]): one inspected share, route or folder, forwarding to an
//!   [`Upstream`] (HTTP(S) origin, or a static folder). A [`Lens`] runs many taps on
//!   loopback listeners, one per tap or several behind host routing ([`Routing`]).
//! - **Capture** ([`Exchange`]): timings, headers, bodies up to a cap, WebSocket/SSE
//!   summaries, errors. Stored in a [`CaptureStore`] (in-memory ring by default),
//!   queried with [`Query`], streamed live with [`Lens::subscribe`], awaited with
//!   [`Lens::wait_for`].
//! - **Redaction** ([`Redaction`]): every read path masks secrets unless told not to.
//! - **Replay** ([`Lens::replay`]), **exports** ([`export`]), **webhook signatures**
//!   ([`webhook`]), **stubs** ([`StubRule`]), **gates** ([`Gates`]), **HTML injection**
//!   ([`Injection`], [`ReservedHandler`]), **paused page** ([`PausedPage`]), **header
//!   rules** ([`HeaderRules`]) and **metrics** ([`MetricsSnapshot`]).
//!
//! See the crate README for the architecture, guarantees and measured overhead.

#![forbid(unsafe_code)]

mod body;
mod capture;
mod config;
mod error;
mod events;
pub mod export;
mod forward;
mod gate;
mod ids;
mod inject;
mod keepalive;
mod lens;
mod listener;
mod metrics;
mod pages;
mod pattern;
mod recorder;
mod redact;
mod replay;
mod rules;
mod secret;
mod service;
mod sim;
mod store;
mod stream;
mod stub;
mod tap;
mod tunnel;
mod upstream;
mod util;
pub mod webhook;

pub use body::{BoxError, LensBody, empty, full};
pub use capture::{
    BodyRecord, BodyView, ClientInfo, ContentKind, DecodeError, Direction, ErrorKind, Exchange,
    ExchangeError, ExchangeKind, ExchangeState, ExchangeView, FrameOpcode, FrameRecord,
    GateOutcome, HeaderView, MessageCounts, MessageKind, MessagePreview, Redaction, RequestRecord,
    RequestView, Responder, ResponseRecord, ResponseView, StreamStats, Timings, content_kind,
    decode::MAX_DECODED_BYTES, decode_body,
};
pub use config::{
    CaptureConfig, DEFAULT_MAX_BODY_BYTES, FolderConfig, ForwardedHeaders, HostHeader, NameFilter,
    OriginConfig, OriginUrl, PausedPage, TapConfig, Upstream,
};
pub use error::{LensError, Result};
pub use events::{Change, LensEvent};
pub use gate::{AgentPreset, BasicAuth, BearerToken, Gates, LOGIN_PATH, PasswordGate, SecretLink};
pub use ids::{ExchangeId, ListenerId, TapId};
pub use inject::{HandlerFuture, Injection, RESERVED_PREFIX, ReservedHandler, ReservedRequest};
pub use keepalive::DEFAULT_SSE_KEEPALIVE;
pub use lens::{Lens, LensOptions, TapHandle, WaitOptions};
pub use listener::{
    AcceptFuture, Accepted, Acceptor, Io, Limits, ListenOptions, ListenerInfo, PlainAcceptor,
    Routing,
};
pub use metrics::{LatencySummary, MetricsSnapshot, StatusCounts};
pub use pattern::PathPattern;
pub use redact::{
    MASK, is_sensitive_header, is_sensitive_key, mask_header, mask_json, mask_query, mask_text,
};
pub use replay::{MAX_REPLAYS, ReplayOptions, ReplayTarget, RequestEdits, Resign};
pub use rules::{HeaderOp, HeaderRules};
pub use secret::Secret;
pub use sim::{
    FaultAction, FaultRecord, FaultRule, Latency, NetworkConfig, RandomSource, SplitMix,
};
pub use store::{CaptureStore, DEFAULT_CAPACITY, Filter, MAX_PAGE, MemoryStore, Page, Query};
pub use stub::{StubMode, StubRule};
pub use tap::TapInfo;
