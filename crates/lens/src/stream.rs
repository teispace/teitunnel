//! Passive observers for streamed protocols: server-sent events and WebSocket frames.
//!
//! Both are incremental state machines fed with copies of the bytes that pass through;
//! they never alter or delay the stream and hold only bounded previews. A WebSocket
//! observer that meets bytes it can't parse stops observing (the bytes still flow).
//! `permessage-deflate` messages are inflated for previews with one bounded inflater
//! per direction (which also follows context takeover); if inflating fails, previews
//! of compressed messages become unavailable.

use flate2::{Decompress, FlushDecompress};
use http::HeaderMap;

use crate::{Direction, FrameOpcode, FrameRecord, MessageKind, MessagePreview, StreamStats};

/// Inflated bytes allowed per message before giving up (a defence against bombs).
const MAX_INFLATED_PER_MESSAGE: u64 = 16 * 1024 * 1024;

/// Limits for previews.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PreviewLimits {
    /// Messages previewed per exchange.
    pub(crate) count: usize,
    /// Bytes kept per message preview.
    pub(crate) bytes: usize,
    /// WebSocket frames kept per exchange (the most recent).
    pub(crate) frames: usize,
    /// Bytes kept per frame preview.
    pub(crate) frame_bytes: usize,
}

/// A finished message seen by an observer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) kind: MessageKind,
    pub(crate) size: u64,
    pub(crate) head: Vec<u8>,
    pub(crate) compressed: bool,
    pub(crate) inflated: bool,
}

impl Message {
    fn plain(kind: MessageKind, size: u64, head: Vec<u8>) -> Self {
        Self {
            kind,
            size,
            head,
            compressed: false,
            inflated: false,
        }
    }

    /// Adds this message to `stats`; returns whether a preview was added.
    pub(crate) fn record(
        self,
        stats: &mut StreamStats,
        direction: Direction,
        at_us: u64,
        limits: PreviewLimits,
    ) -> bool {
        let counts = match direction {
            Direction::ClientToServer => &mut stats.client,
            Direction::ServerToClient => &mut stats.server,
        };
        counts.count += 1;
        counts.bytes += self.size;
        if stats.previews.len() >= limits.count {
            return false;
        }
        let available = !self.compressed || self.inflated;
        // Inflated previews may be longer than the compressed size.
        let truncated = if self.compressed {
            !available || self.head.len() >= limits.bytes
        } else {
            (self.head.len() as u64) < self.size
        };
        let preview = if !available {
            String::new()
        } else {
            match self.kind {
                MessageKind::Binary => crate::util::hex_encode(&self.head),
                MessageKind::Close if self.head.len() >= 2 => {
                    let code = u16::from_be_bytes([self.head[0], self.head[1]]);
                    format!("{code} {}", String::from_utf8_lossy(&self.head[2..]))
                }
                _ => String::from_utf8_lossy(&self.head).into_owned(),
            }
        };
        stats.previews.push(MessagePreview {
            at_us,
            direction,
            kind: self.kind,
            size: self.size,
            preview,
            truncated,
            compressed: self.compressed,
            inflated: self.inflated,
        });
        true
    }
}

/// A finished frame seen by a WebSocket observer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Frame {
    pub(crate) opcode: FrameOpcode,
    pub(crate) fin: bool,
    pub(crate) masked: bool,
    pub(crate) compressed: bool,
    pub(crate) size: u64,
    /// Unmasked first bytes (empty for compressed frames).
    pub(crate) head: Vec<u8>,
    /// Whether the payload is text (text messages, close reasons).
    pub(crate) text: bool,
}

impl Frame {
    /// Adds this frame to `stats`, dropping the oldest beyond the limit.
    pub(crate) fn record(
        self,
        stats: &mut StreamStats,
        direction: Direction,
        at_us: u64,
        limits: PreviewLimits,
    ) {
        let (close_code, close_reason) = match self.opcode {
            FrameOpcode::Close if self.head.len() >= 2 => (
                Some(u16::from_be_bytes([self.head[0], self.head[1]])),
                Some(String::from_utf8_lossy(&self.head[2..]).into_owned()),
            ),
            _ => (None, None),
        };
        let preview = if self.compressed {
            None
        } else if self.opcode == FrameOpcode::Close {
            close_reason.clone()
        } else if self.text {
            Some(String::from_utf8_lossy(&self.head).into_owned())
        } else {
            Some(crate::util::hex_encode(&self.head))
        };
        let truncated = !self.compressed && (self.head.len() as u64) < self.size;
        if limits.frames == 0 {
            stats.frames_dropped += 1;
            return;
        }
        while stats.frames.len() >= limits.frames {
            stats.frames.pop_front();
            stats.frames_dropped += 1;
        }
        stats.frames.push_back(FrameRecord {
            at_us,
            direction,
            opcode: self.opcode,
            fin: self.fin,
            masked: self.masked,
            compressed: self.compressed,
            size: self.size,
            preview,
            truncated,
            close_code,
            close_reason,
        });
    }
}

/// What a WebSocket observer reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Observed {
    Frame(Frame),
    Message(Message),
}

impl Observed {
    /// Records into `stats`; returns whether a message preview was added.
    pub(crate) fn record(
        self,
        stats: &mut StreamStats,
        direction: Direction,
        at_us: u64,
        limits: PreviewLimits,
    ) -> bool {
        match self {
            Self::Frame(frame) => {
                frame.record(stats, direction, at_us, limits);
                false
            }
            Self::Message(message) => message.record(stats, direction, at_us, limits),
        }
    }
}

/// Counts server-sent events (blocks separated by a blank line; comment-only blocks
/// don't count, per the HTML spec's dispatch rules).
#[derive(Debug)]
pub(crate) struct SseObserver {
    preview_bytes: usize,
    pending_cr: bool,
    line_len: usize,
    line_is_comment: bool,
    event_size: u64,
    event_has_field: bool,
    preview: Vec<u8>,
}

impl SseObserver {
    pub(crate) fn new(preview_bytes: usize) -> Self {
        Self {
            preview_bytes,
            pending_cr: false,
            line_len: 0,
            line_is_comment: false,
            event_size: 0,
            event_has_field: false,
            preview: Vec::new(),
        }
    }

    /// Feeds bytes; finished events are appended to `out`.
    pub(crate) fn feed(&mut self, data: &[u8], out: &mut Vec<Message>) {
        for &b in data {
            if self.pending_cr {
                self.pending_cr = false;
                if b == b'\n' {
                    continue;
                }
            }
            if b == b'\r' || b == b'\n' {
                self.pending_cr = b == b'\r';
                if self.line_len == 0 {
                    if self.event_has_field {
                        let head = std::mem::take(&mut self.preview);
                        let head = head.strip_suffix(b"\n").unwrap_or(&head).to_vec();
                        out.push(Message::plain(MessageKind::Event, self.event_size, head));
                    }
                    self.event_size = 0;
                    self.event_has_field = false;
                    self.preview.clear();
                } else {
                    if !self.line_is_comment && self.preview.len() < self.preview_bytes {
                        self.preview.push(b'\n');
                    }
                    self.line_len = 0;
                }
                continue;
            }
            if self.line_len == 0 {
                self.line_is_comment = b == b':';
                if !self.line_is_comment {
                    self.event_has_field = true;
                }
            }
            self.line_len += 1;
            if !self.line_is_comment {
                self.event_size += 1;
                if self.preview.len() < self.preview_bytes {
                    self.preview.push(b);
                }
            }
        }
    }
}

/// `permessage-deflate` as negotiated in the `101` response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Deflate {
    /// The client resets its compressor after each message.
    pub(crate) client_no_context_takeover: bool,
    /// The server resets its compressor after each message.
    pub(crate) server_no_context_takeover: bool,
}

impl Deflate {
    /// Reads `Sec-WebSocket-Extensions` from the switching response.
    pub(crate) fn negotiated(headers: &HeaderMap) -> Option<Self> {
        let extension = headers
            .get_all("sec-websocket-extensions")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(','))
            .find(|ext| {
                ext.trim()
                    .to_ascii_lowercase()
                    .starts_with("permessage-deflate")
            })?;
        let params: Vec<String> = extension
            .split(';')
            .map(|p| p.trim().to_ascii_lowercase())
            .collect();
        Some(Self {
            client_no_context_takeover: params.iter().any(|p| p == "client_no_context_takeover"),
            server_no_context_takeover: params.iter().any(|p| p == "server_no_context_takeover"),
        })
    }

    /// Whether the sender in `direction` resets after each message.
    pub(crate) fn resets(self, direction: Direction) -> bool {
        match direction {
            Direction::ClientToServer => self.client_no_context_takeover,
            Direction::ServerToClient => self.server_no_context_takeover,
        }
    }
}

/// Inflates one direction's compressed messages for previews, with bounded output.
struct Inflater {
    decompress: Decompress,
    reset_each_message: bool,
    broken: bool,
    produced: u64,
}

impl std::fmt::Debug for Inflater {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Inflater")
            .field("broken", &self.broken)
            .finish_non_exhaustive()
    }
}

impl Inflater {
    fn new(reset_each_message: bool) -> Self {
        Self {
            decompress: Decompress::new(false),
            reset_each_message,
            broken: false,
            produced: 0,
        }
    }

    /// Inflates `input`, appending up to `keep` total bytes to `out` (the rest is
    /// discarded but still decoded, to keep the shared window right).
    fn feed(&mut self, mut input: &[u8], out: &mut Vec<u8>, keep: usize) {
        let mut buf = [0u8; 8 * 1024];
        while !self.broken {
            let (before_in, before_out) = (self.decompress.total_in(), self.decompress.total_out());
            if self
                .decompress
                .decompress(input, &mut buf, FlushDecompress::Sync)
                .is_err()
            {
                self.broken = true;
                return;
            }
            let consumed = usize::try_from(self.decompress.total_in() - before_in).unwrap_or(0);
            let produced = usize::try_from(self.decompress.total_out() - before_out).unwrap_or(0);
            let room = keep.saturating_sub(out.len());
            out.extend_from_slice(&buf[..produced.min(room)]);
            self.produced += produced as u64;
            if self.produced > MAX_INFLATED_PER_MESSAGE {
                self.broken = true;
                return;
            }
            input = &input[consumed.min(input.len())..];
            let more_output = produced == buf.len();
            if (input.is_empty() && !more_output) || (consumed == 0 && produced == 0) {
                return;
            }
        }
    }

    fn end_message(&mut self, out: &mut Vec<u8>, keep: usize) {
        // RFC 7692 §7.2.2: the sender removed this empty-block tail.
        self.feed(&[0x00, 0x00, 0xff, 0xff], out, keep);
        self.produced = 0;
        if self.reset_each_message {
            self.decompress.reset(false);
        }
    }
}

/// Observes one direction of a WebSocket connection (RFC 6455 framing).
#[derive(Debug)]
pub(crate) struct WsObserver {
    preview_bytes: usize,
    frame_bytes: usize,
    broken: bool,
    // Frame header being read.
    header: [u8; 14],
    have: usize,
    need: usize,
    // Current frame.
    opcode: u8,
    fin: bool,
    rsv1: bool,
    payload_len: u64,
    payload_left: u64,
    mask: Option<[u8; 4]>,
    mask_pos: usize,
    frame_head: Vec<u8>,
    // Current control frame payload (at most 125 bytes).
    control: Vec<u8>,
    // Current data message (may span frames).
    msg_kind: Option<MessageKind>,
    msg_size: u64,
    msg_head: Vec<u8>,
    msg_compressed: bool,
    inflater: Option<Inflater>,
}

impl WsObserver {
    /// An observer; `deflate` is `Some(reset_each_message)` when `permessage-deflate`
    /// was negotiated for this direction's sender.
    pub(crate) fn new(limits: PreviewLimits, deflate: Option<bool>) -> Self {
        Self {
            preview_bytes: limits.bytes,
            frame_bytes: limits.frame_bytes,
            broken: false,
            header: [0; 14],
            have: 0,
            need: 2,
            opcode: 0,
            fin: false,
            rsv1: false,
            payload_len: 0,
            payload_left: 0,
            mask: None,
            mask_pos: 0,
            frame_head: Vec::new(),
            control: Vec::new(),
            msg_kind: None,
            msg_size: 0,
            msg_head: Vec::new(),
            msg_compressed: false,
            inflater: deflate.map(Inflater::new),
        }
    }

    /// Whether the observer gave up on malformed data.
    pub(crate) fn is_broken(&self) -> bool {
        self.broken
    }

    /// Feeds bytes; finished frames and messages are appended to `out`.
    pub(crate) fn feed(&mut self, mut data: &[u8], out: &mut Vec<Observed>) {
        while !data.is_empty() && !self.broken {
            if self.need > 0 {
                let take = self.need.min(data.len());
                self.header[self.have..self.have + take].copy_from_slice(&data[..take]);
                self.have += take;
                self.need -= take;
                data = &data[take..];
                if self.need == 0 {
                    self.header_ready(out);
                }
                continue;
            }
            let take = usize::try_from(self.payload_left)
                .unwrap_or(usize::MAX)
                .min(data.len());
            self.payload(&data[..take]);
            self.payload_left -= take as u64;
            data = &data[take..];
            if self.payload_left == 0 {
                self.frame_done(out);
            }
        }
    }

    fn header_ready(&mut self, out: &mut Vec<Observed>) {
        let b0 = self.header[0];
        let b1 = self.header[1];
        let len7 = b1 & 0x7f;
        let masked = b1 & 0x80 != 0;
        let extra = match len7 {
            126 => 2,
            127 => 8,
            _ => 0,
        } + if masked { 4 } else { 0 };
        let full = 2 + extra;
        if self.have < full {
            // Base header read; now read the extended length and mask.
            self.need = full - self.have;
            return;
        }
        self.fin = b0 & 0x80 != 0;
        self.rsv1 = b0 & 0x40 != 0;
        self.opcode = b0 & 0x0f;
        let (length, mask_at) = match len7 {
            126 => (
                u64::from(u16::from_be_bytes([self.header[2], self.header[3]])),
                4,
            ),
            127 => {
                let mut bytes = [0u8; 8];
                bytes.copy_from_slice(&self.header[2..10]);
                (u64::from_be_bytes(bytes), 10)
            }
            n => (u64::from(n), 2),
        };
        if length > (1 << 62) {
            self.broken = true;
            return;
        }
        self.mask = masked.then(|| {
            [
                self.header[mask_at],
                self.header[mask_at + 1],
                self.header[mask_at + 2],
                self.header[mask_at + 3],
            ]
        });
        self.mask_pos = 0;
        self.payload_len = length;
        self.payload_left = length;
        self.have = 0;
        self.need = 0;
        self.frame_head.clear();
        match self.opcode {
            0x0 => {
                if self.msg_kind.is_none() {
                    self.broken = true;
                    return;
                }
            }
            0x1 | 0x2 => {
                self.msg_kind = Some(if self.opcode == 0x1 {
                    MessageKind::Text
                } else {
                    MessageKind::Binary
                });
                self.msg_size = 0;
                self.msg_head.clear();
                self.msg_compressed = self.rsv1;
            }
            0x8..=0xa => {
                if length > 125 || !self.fin {
                    self.broken = true;
                    return;
                }
                self.control.clear();
            }
            _ => {
                self.broken = true;
                return;
            }
        }
        if length == 0 {
            self.frame_done(out);
        }
    }

    fn unmask(&self, chunk: &[u8]) -> Vec<u8> {
        match self.mask {
            Some(mask) => chunk
                .iter()
                .enumerate()
                .map(|(i, b)| b ^ mask[(self.mask_pos + i) % 4])
                .collect(),
            None => chunk.to_vec(),
        }
    }

    fn payload(&mut self, chunk: &[u8]) {
        let is_control = self.opcode >= 0x8;
        let compressed = !is_control && self.msg_compressed;
        if is_control {
            let keep = 125usize.saturating_sub(self.control.len()).min(chunk.len());
            let bytes = self.unmask(&chunk[..keep]);
            self.control.extend_from_slice(&bytes);
            self.frame_head.extend_from_slice(&bytes);
        } else if compressed {
            self.msg_size += chunk.len() as u64;
            if self.inflater.as_ref().is_some_and(|i| !i.broken) {
                let bytes = self.unmask(chunk);
                if let Some(inflater) = self.inflater.as_mut() {
                    inflater.feed(&bytes, &mut self.msg_head, self.preview_bytes);
                }
            }
        } else {
            self.msg_size += chunk.len() as u64;
            let wanted = self
                .frame_bytes
                .saturating_sub(self.frame_head.len())
                .max(self.preview_bytes.saturating_sub(self.msg_head.len()))
                .min(chunk.len());
            let bytes = self.unmask(&chunk[..wanted]);
            let frame_room = self.frame_bytes.saturating_sub(self.frame_head.len());
            self.frame_head
                .extend_from_slice(&bytes[..frame_room.min(bytes.len())]);
            let msg_room = self.preview_bytes.saturating_sub(self.msg_head.len());
            self.msg_head
                .extend_from_slice(&bytes[..msg_room.min(bytes.len())]);
        }
        self.mask_pos = (self.mask_pos + chunk.len()) % 4;
    }

    fn frame_done(&mut self, out: &mut Vec<Observed>) {
        self.need = 2;
        self.have = 0;
        let opcode = match self.opcode {
            0x0 => FrameOpcode::Continuation,
            0x1 => FrameOpcode::Text,
            0x2 => FrameOpcode::Binary,
            0x8 => FrameOpcode::Close,
            0x9 => FrameOpcode::Ping,
            _ => FrameOpcode::Pong,
        };
        let is_control = self.opcode >= 0x8;
        let compressed = !is_control && self.msg_compressed;
        let mut head = std::mem::take(&mut self.frame_head);
        head.truncate(self.frame_bytes.max(2));
        out.push(Observed::Frame(Frame {
            opcode,
            fin: self.fin,
            masked: self.mask.is_some(),
            compressed,
            size: self.payload_len,
            head: if compressed { Vec::new() } else { head },
            text: self.msg_kind == Some(MessageKind::Text) && !is_control
                || opcode == FrameOpcode::Close,
        }));
        if is_control {
            let kind = match opcode {
                FrameOpcode::Close => MessageKind::Close,
                FrameOpcode::Ping => MessageKind::Ping,
                _ => MessageKind::Pong,
            };
            let mut head = std::mem::take(&mut self.control);
            head.truncate(self.preview_bytes.max(2));
            out.push(Observed::Message(Message::plain(
                kind,
                self.payload_len,
                head,
            )));
            return;
        }
        if self.fin
            && let Some(kind) = self.msg_kind.take()
        {
            let mut inflated = false;
            if self.msg_compressed
                && let Some(inflater) = self.inflater.as_mut()
            {
                inflater.end_message(&mut self.msg_head, self.preview_bytes);
                inflated = !inflater.broken;
            }
            out.push(Observed::Message(Message {
                kind,
                size: self.msg_size,
                head: std::mem::take(&mut self.msg_head),
                compressed: self.msg_compressed,
                inflated,
            }));
            self.msg_compressed = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use proptest::prelude::*;

    use super::*;

    const LIMITS: PreviewLimits = PreviewLimits {
        count: 20,
        bytes: 1024,
        frames: 500,
        frame_bytes: 4096,
    };

    fn frame(fin: bool, opcode: u8, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
        frame_rsv(fin, false, opcode, payload, mask)
    }

    fn frame_rsv(
        fin: bool,
        rsv1: bool,
        opcode: u8,
        payload: &[u8],
        mask: Option<[u8; 4]>,
    ) -> Vec<u8> {
        let mut out = vec![(u8::from(fin) << 7) | (u8::from(rsv1) << 6) | opcode];
        let mask_bit = if mask.is_some() { 0x80 } else { 0 };
        match payload.len() {
            n if n < 126 => out.push(mask_bit | u8::try_from(n).unwrap()),
            n if n < 65_536 => {
                out.push(mask_bit | 126);
                out.extend_from_slice(&u16::try_from(n).unwrap().to_be_bytes());
            }
            n => {
                out.push(mask_bit | 127);
                out.extend_from_slice(&(n as u64).to_be_bytes());
            }
        }
        match mask {
            Some(key) => {
                out.extend_from_slice(&key);
                out.extend(payload.iter().enumerate().map(|(i, b)| b ^ key[i % 4]));
            }
            None => out.extend_from_slice(payload),
        }
        out
    }

    fn messages(observed: &[Observed]) -> Vec<&Message> {
        observed
            .iter()
            .filter_map(|o| match o {
                Observed::Message(m) => Some(m),
                Observed::Frame(_) => None,
            })
            .collect()
    }

    fn frames(observed: &[Observed]) -> Vec<&Frame> {
        observed
            .iter()
            .filter_map(|o| match o {
                Observed::Frame(f) => Some(f),
                Observed::Message(_) => None,
            })
            .collect()
    }

    #[test]
    fn sse_counts_events_across_chunks() {
        let mut observer = SseObserver::new(64);
        let mut out = Vec::new();
        let stream =
            b"event: tick\ndata: 1\n\n: keep-alive\n\ndata: two\r\n\r\ndata: 3\r\rdata: partial";
        for chunk in stream.chunks(3) {
            observer.feed(chunk, &mut out);
        }
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].head, b"event: tick\ndata: 1");
        assert_eq!(out[1].head, b"data: two");
        assert_eq!(out[2].head, b"data: 3");
        assert_eq!(out[0].size, "event: tickdata: 1".len() as u64);
    }

    #[test]
    fn sse_preview_is_bounded() {
        let mut observer = SseObserver::new(8);
        let mut out = Vec::new();
        observer.feed(
            format!("data: {}\n\n", "x".repeat(10_000)).as_bytes(),
            &mut out,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].head.len(), 8);
        assert_eq!(out[0].size, 10_006);
    }

    #[test]
    fn ws_masked_text_fragmented_and_control() {
        let mut observer = WsObserver::new(LIMITS, None);
        let mut out = Vec::new();
        let mut bytes = frame(false, 0x1, b"Hel", Some([1, 2, 3, 4]));
        bytes.extend(frame(true, 0x9, b"ping", Some([9, 9, 9, 9])));
        bytes.extend(frame(true, 0x0, b"lo", Some([5, 6, 7, 8])));
        bytes.extend(frame(true, 0x2, &[0xde, 0xad], None));
        let mut close = 1000u16.to_be_bytes().to_vec();
        close.extend_from_slice(b"bye");
        bytes.extend(frame(true, 0x8, &close, None));
        for chunk in bytes.chunks(1) {
            observer.feed(chunk, &mut out);
        }
        assert!(!observer.is_broken());
        let kinds: Vec<MessageKind> = messages(&out).iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                MessageKind::Ping,
                MessageKind::Text,
                MessageKind::Binary,
                MessageKind::Close
            ]
        );
        assert_eq!(messages(&out)[1].head, b"Hello");

        let mut stats = StreamStats::default();
        for observed in out {
            observed.record(&mut stats, Direction::ClientToServer, 1, LIMITS);
        }
        assert_eq!(stats.client.count, 4);
        assert_eq!(stats.previews[2].preview, "dead");
        assert_eq!(stats.previews[3].preview, "1000 bye");
        // Frames: every frame, with opcode, fin, mask and previews.
        let f: Vec<&FrameRecord> = stats.frames.iter().collect();
        assert_eq!(f.len(), 5);
        assert_eq!(
            (f[0].opcode, f[0].fin, f[0].masked),
            (FrameOpcode::Text, false, true)
        );
        assert_eq!(f[0].preview.as_deref(), Some("Hel"));
        assert_eq!(f[1].opcode, FrameOpcode::Ping);
        assert_eq!(
            (f[2].opcode, f[2].preview.as_deref()),
            (FrameOpcode::Continuation, Some("lo"))
        );
        assert_eq!(f[3].preview.as_deref(), Some("dead"));
        assert_eq!(
            (f[4].close_code, f[4].close_reason.as_deref()),
            (Some(1000), Some("bye"))
        );
    }

    #[test]
    fn ws_large_frames_and_bounded_previews() {
        let limits = PreviewLimits {
            bytes: 16,
            frame_bytes: 32,
            ..LIMITS
        };
        let mut observer = WsObserver::new(limits, None);
        let mut out = Vec::new();
        let big = vec![b'a'; 70_000];
        observer.feed(&frame(true, 0x2, &big, Some([1, 1, 1, 1])), &mut out);
        let medium = vec![b'b'; 300];
        observer.feed(&frame(true, 0x1, &medium, None), &mut out);
        let m = messages(&out);
        assert_eq!(m.len(), 2);
        assert_eq!((m[0].size, m[0].head.len()), (70_000, 16));
        assert_eq!(m[1].size, 300);
        let f = frames(&out);
        assert_eq!((f[0].size, f[0].head.len()), (70_000, 32));
    }

    #[test]
    fn frame_list_keeps_the_most_recent() {
        let limits = PreviewLimits {
            frames: 3,
            ..LIMITS
        };
        let mut observer = WsObserver::new(limits, None);
        let mut out = Vec::new();
        for i in 0..10u8 {
            observer.feed(&frame(true, 0x2, &[i], None), &mut out);
        }
        let mut stats = StreamStats::default();
        for observed in out {
            observed.record(&mut stats, Direction::ServerToClient, 0, limits);
        }
        let kept: Vec<String> = stats
            .frames
            .iter()
            .filter_map(|f| f.preview.clone())
            .collect();
        assert_eq!(kept, vec!["07", "08", "09"]);
        assert_eq!(stats.frames_dropped, 7);
    }

    fn deflate_message(compressor: &mut flate2::Compress, text: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len() + 64);
        compressor
            .compress_vec(text, &mut out, flate2::FlushCompress::Sync)
            .unwrap();
        // RFC 7692: drop the trailing empty block.
        assert!(out.ends_with(&[0x00, 0x00, 0xff, 0xff]));
        out.truncate(out.len() - 4);
        out
    }

    #[test]
    fn permessage_deflate_with_context_takeover() {
        let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
        let mut observer = WsObserver::new(LIMITS, Some(false));
        let mut out = Vec::new();
        for text in [&b"hello hello hello"[..], b"hello again, hello"] {
            let payload = deflate_message(&mut compressor, text);
            // Split the compressed message over two frames.
            let (a, b) = payload.split_at(payload.len() / 2);
            observer.feed(
                &frame_rsv(false, true, 0x1, a, Some([3, 1, 4, 1])),
                &mut out,
            );
            observer.feed(&frame(true, 0x0, b, Some([5, 9, 2, 6])), &mut out);
        }
        let m = messages(&out);
        assert_eq!(m.len(), 2);
        assert!(m[0].compressed && m[0].inflated);
        assert_eq!(m[0].head, b"hello hello hello");
        assert_eq!(
            m[1].head, b"hello again, hello",
            "the shared window is followed"
        );
        let f = frames(&out);
        assert!(f[0].compressed && f[0].head.is_empty());
    }

    #[test]
    fn permessage_deflate_without_negotiation_is_unavailable() {
        let mut compressor = flate2::Compress::new(flate2::Compression::default(), false);
        let payload = deflate_message(&mut compressor, b"secret-ish text");
        let mut observer = WsObserver::new(LIMITS, None);
        let mut out = Vec::new();
        observer.feed(&frame_rsv(true, true, 0x1, &payload, None), &mut out);
        let mut stats = StreamStats::default();
        for observed in out {
            observed.record(&mut stats, Direction::ServerToClient, 0, LIMITS);
        }
        let preview = &stats.previews[0];
        assert!(preview.compressed && !preview.inflated && preview.preview.is_empty());
        assert_eq!(stats.frames[0].preview, None);
    }

    #[test]
    fn corrupt_deflate_marks_previews_unavailable() {
        let mut observer = WsObserver::new(LIMITS, Some(true));
        let mut out = Vec::new();
        observer.feed(
            &frame_rsv(true, true, 0x1, &[0xff, 0xff, 0xff, 0xff], None),
            &mut out,
        );
        let m = messages(&out);
        assert!(m[0].compressed && !m[0].inflated);
    }

    #[test]
    fn negotiated_extension_parsing() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "sec-websocket-extensions",
            "permessage-deflate; client_no_context_takeover; server_max_window_bits=10"
                .parse()
                .unwrap(),
        );
        let deflate = Deflate::negotiated(&headers).unwrap();
        assert!(deflate.resets(Direction::ClientToServer));
        assert!(!deflate.resets(Direction::ServerToClient));
        assert!(Deflate::negotiated(&HeaderMap::new()).is_none());
    }

    #[test]
    fn ws_garbage_breaks_observer() {
        let mut observer = WsObserver::new(LIMITS, None);
        let mut out = Vec::new();
        observer.feed(&[0x83, 0x00], &mut out); // reserved opcode 3
        assert!(observer.is_broken());
        let mut observer = WsObserver::new(LIMITS, None);
        observer.feed(&[0x80, 0x01, b'x'], &mut out); // continuation without a message
        assert!(observer.is_broken());
    }

    #[test]
    fn preview_limit_counts_but_stops_previewing() {
        let mut stats = StreamStats::default();
        let limits = PreviewLimits {
            count: 1,
            bytes: 4,
            ..LIMITS
        };
        for _ in 0..3 {
            Message::plain(MessageKind::Event, 10, b"data".to_vec()).record(
                &mut stats,
                Direction::ServerToClient,
                0,
                limits,
            );
        }
        assert_eq!(stats.server.count, 3);
        assert_eq!(stats.server.bytes, 30);
        assert_eq!(stats.previews.len(), 1);
        assert!(stats.previews[0].truncated);
    }

    #[test]
    fn inflate_bomb_is_bounded() {
        let zeros = vec![0u8; 20 * 1024 * 1024];
        let mut encoder =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::best());
        encoder.write_all(&zeros).unwrap();
        let payload = encoder.finish().unwrap();
        let mut observer = WsObserver::new(LIMITS, Some(true));
        let mut out = Vec::new();
        observer.feed(&frame_rsv(true, true, 0x2, &payload, None), &mut out);
        let m = messages(&out);
        assert!(
            m[0].compressed && !m[0].inflated,
            "stopped after 16 MiB of output"
        );
        assert!(m[0].head.len() <= LIMITS.bytes);
    }

    proptest! {
        #[test]
        fn observers_never_panic(data in proptest::collection::vec(any::<u8>(), 0..4096), split in 1usize..64) {
            let mut ws = WsObserver::new(LIMITS, Some(false));
            let mut sse = SseObserver::new(32);
            let mut observed = Vec::new();
            let mut events = Vec::new();
            for chunk in data.chunks(split) {
                ws.feed(chunk, &mut observed);
                sse.feed(chunk, &mut events);
            }
            let mut stats = StreamStats::default();
            for o in observed {
                o.record(&mut stats, Direction::ClientToServer, 0, LIMITS);
            }
        }

        #[test]
        fn ws_round_trips_any_split(payload in proptest::collection::vec(any::<u8>(), 0..3000), split in 1usize..200) {
            let bytes = frame(true, 0x2, &payload, Some([7, 3, 1, 9]));
            let mut observer = WsObserver::new(PreviewLimits { bytes: 4096, ..LIMITS }, None);
            let mut out = Vec::new();
            for chunk in bytes.chunks(split) {
                observer.feed(chunk, &mut out);
            }
            let m = messages(&out);
            prop_assert_eq!(m.len(), 1);
            prop_assert_eq!(&m[0].head, &payload);
        }
    }
}
