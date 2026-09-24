//! Passive observers for streamed protocols: server-sent events and WebSocket frames.
//!
//! Both are incremental state machines fed with whatever bytes pass through; they never
//! hold more than one bounded preview, and they never alter or delay the stream. A
//! WebSocket observer that meets bytes it can't parse stops observing (the bytes still
//! flow).

use crate::{Direction, MessageKind, MessagePreview, StreamStats};

/// Limits for previews.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PreviewLimits {
    /// Messages previewed per exchange.
    pub(crate) count: usize,
    /// Bytes kept per preview.
    pub(crate) bytes: usize,
}

/// A finished message seen by an observer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Message {
    pub(crate) kind: MessageKind,
    pub(crate) size: u64,
    pub(crate) head: Vec<u8>,
    pub(crate) compressed: bool,
}

impl Message {
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
        let truncated = (self.head.len() as u64) < self.size;
        let preview = match self.kind {
            MessageKind::Binary => crate::util::hex_encode(&self.head),
            MessageKind::Close if self.head.len() >= 2 => {
                let code = u16::from_be_bytes([self.head[0], self.head[1]]);
                format!("{code} {}", String::from_utf8_lossy(&self.head[2..]))
            }
            _ => String::from_utf8_lossy(&self.head).into_owned(),
        };
        stats.previews.push(MessagePreview {
            at_us,
            direction,
            kind: self.kind,
            size: self.size,
            preview,
            truncated,
            compressed: self.compressed,
        });
        true
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
                        let head_trimmed = head.strip_suffix(b"\n").unwrap_or(&head).to_vec();
                        out.push(Message {
                            kind: MessageKind::Event,
                            size: self.event_size,
                            head: head_trimmed,
                            compressed: false,
                        });
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

/// Observes one direction of a WebSocket connection (RFC 6455 framing).
#[derive(Debug)]
pub(crate) struct WsObserver {
    preview_bytes: usize,
    broken: bool,
    // Frame header being read.
    header: [u8; 14],
    have: usize,
    need: usize,
    // Current frame.
    opcode: u8,
    fin: bool,
    payload_left: u64,
    mask: Option<[u8; 4]>,
    mask_pos: usize,
    // Current control frame payload (at most 125 bytes).
    control: Vec<u8>,
    control_size: u64,
    // Current data message (may span frames).
    msg_kind: Option<MessageKind>,
    msg_size: u64,
    msg_head: Vec<u8>,
    msg_compressed: bool,
}

impl WsObserver {
    pub(crate) fn new(preview_bytes: usize) -> Self {
        Self {
            preview_bytes,
            broken: false,
            header: [0; 14],
            have: 0,
            need: 2,
            opcode: 0,
            fin: false,
            payload_left: 0,
            mask: None,
            mask_pos: 0,
            control: Vec::new(),
            control_size: 0,
            msg_kind: None,
            msg_size: 0,
            msg_head: Vec::new(),
            msg_compressed: false,
        }
    }

    /// Whether the observer gave up on malformed data.
    pub(crate) fn is_broken(&self) -> bool {
        self.broken
    }

    /// Feeds bytes; finished messages are appended to `out`.
    pub(crate) fn feed(&mut self, mut data: &[u8], out: &mut Vec<Message>) {
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

    fn header_ready(&mut self, out: &mut Vec<Message>) {
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
        let rsv1 = b0 & 0x40 != 0;
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
        self.payload_left = length;
        self.have = 0;
        self.need = 0;
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
                self.msg_compressed = rsv1;
            }
            0x8..=0xa => {
                if length > 125 || !self.fin {
                    self.broken = true;
                    return;
                }
                self.control.clear();
                self.control_size = length;
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

    fn payload(&mut self, chunk: &[u8]) {
        let is_control = self.opcode >= 0x8;
        let room = if is_control {
            125usize.saturating_sub(self.control.len())
        } else {
            self.msg_size += chunk.len() as u64;
            self.preview_bytes.saturating_sub(self.msg_head.len())
        };
        let keep = room.min(chunk.len());
        let target = if is_control {
            &mut self.control
        } else {
            &mut self.msg_head
        };
        for (i, &b) in chunk[..keep].iter().enumerate() {
            let unmasked = match self.mask {
                Some(mask) => b ^ mask[(self.mask_pos + i) % 4],
                None => b,
            };
            target.push(unmasked);
        }
        self.mask_pos = (self.mask_pos + chunk.len()) % 4;
    }

    fn frame_done(&mut self, out: &mut Vec<Message>) {
        self.need = 2;
        self.have = 0;
        match self.opcode {
            0x8..=0xa => {
                let kind = match self.opcode {
                    0x8 => MessageKind::Close,
                    0x9 => MessageKind::Ping,
                    _ => MessageKind::Pong,
                };
                let mut head = std::mem::take(&mut self.control);
                head.truncate(self.preview_bytes.max(2));
                out.push(Message {
                    kind,
                    size: self.control_size,
                    head,
                    compressed: false,
                });
            }
            _ => {
                if self.fin
                    && let Some(kind) = self.msg_kind.take()
                {
                    out.push(Message {
                        kind,
                        size: self.msg_size,
                        head: std::mem::take(&mut self.msg_head),
                        compressed: self.msg_compressed,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    fn frame(fin: bool, opcode: u8, payload: &[u8], mask: Option<[u8; 4]>) -> Vec<u8> {
        let mut out = vec![(u8::from(fin) << 7) | opcode];
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
        let mut observer = WsObserver::new(1024);
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
        let kinds: Vec<MessageKind> = out.iter().map(|m| m.kind).collect();
        assert_eq!(
            kinds,
            vec![
                MessageKind::Ping,
                MessageKind::Text,
                MessageKind::Binary,
                MessageKind::Close
            ]
        );
        assert_eq!(out[1].head, b"Hello");
        assert_eq!(out[1].size, 5);

        let mut stats = StreamStats::default();
        let limits = PreviewLimits {
            count: 10,
            bytes: 1024,
        };
        for message in out {
            message.record(&mut stats, Direction::ClientToServer, 1, limits);
        }
        assert_eq!(stats.client.count, 4);
        assert_eq!(stats.previews[2].preview, "dead");
        assert_eq!(stats.previews[3].preview, "1000 bye");
    }

    #[test]
    fn ws_large_frames_and_bounded_preview() {
        let mut observer = WsObserver::new(16);
        let mut out = Vec::new();
        let big = vec![b'a'; 70_000];
        observer.feed(&frame(true, 0x2, &big, Some([1, 1, 1, 1])), &mut out);
        let medium = vec![b'b'; 300];
        observer.feed(&frame(true, 0x1, &medium, None), &mut out);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].size, 70_000);
        assert_eq!(out[0].head.len(), 16);
        assert_eq!(out[1].size, 300);
    }

    #[test]
    fn ws_garbage_breaks_observer() {
        let mut observer = WsObserver::new(16);
        let mut out = Vec::new();
        observer.feed(&[0x83, 0x00], &mut out); // reserved opcode 3
        assert!(observer.is_broken());
        let mut observer = WsObserver::new(16);
        observer.feed(&[0x80, 0x01, b'x'], &mut out); // continuation without a message
        assert!(observer.is_broken());
    }

    #[test]
    fn preview_limit_counts_but_stops_previewing() {
        let mut stats = StreamStats::default();
        let limits = PreviewLimits { count: 1, bytes: 4 };
        for _ in 0..3 {
            Message {
                kind: MessageKind::Event,
                size: 10,
                head: b"data".to_vec(),
                compressed: false,
            }
            .record(&mut stats, Direction::ServerToClient, 0, limits);
        }
        assert_eq!(stats.server.count, 3);
        assert_eq!(stats.server.bytes, 30);
        assert_eq!(stats.previews.len(), 1);
        assert!(stats.previews[0].truncated);
    }

    proptest! {
        #[test]
        fn observers_never_panic(data in proptest::collection::vec(any::<u8>(), 0..4096), split in 1usize..64) {
            let mut ws = WsObserver::new(32);
            let mut sse = SseObserver::new(32);
            let mut out = Vec::new();
            for chunk in data.chunks(split) {
                ws.feed(chunk, &mut out);
                sse.feed(chunk, &mut out);
            }
        }

        #[test]
        fn ws_round_trips_any_split(payload in proptest::collection::vec(any::<u8>(), 0..3000), split in 1usize..200) {
            let bytes = frame(true, 0x2, &payload, Some([7, 3, 1, 9]));
            let mut observer = WsObserver::new(4096);
            let mut out = Vec::new();
            for chunk in bytes.chunks(split) {
                observer.feed(chunk, &mut out);
            }
            prop_assert_eq!(out.len(), 1);
            prop_assert_eq!(&out[0].head, &payload);
        }
    }
}
