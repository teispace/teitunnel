# teitunnel-lens

Lens is Teitunnel's local inspecting reverse proxy (M12-02, see
[docs/plans/M12-platform.md](../../docs/plans/M12-platform.md)). It sits between cloudflared
and the user's origin and records every exchange while bodies stream through untouched:

```
visitor → Cloudflare edge → cloudflared → Lens (127.0.0.1:random) → origin or folder
```

It is a pure library (no Tauri, no `teitunnel-core`); the app, the CLI and `teitunnel serve`
embed it, and `core` decides which share or route gets a tap.

## Using it

```rust
use lens::{Filter, Lens, LensOptions, Redaction, TapConfig, Upstream, WaitOptions, export};

let lens = Lens::new(LensOptions::default())?;
let tap = lens
    .start_tap(TapConfig::new(Upstream::origin("http://localhost:3000")?))
    .await?;
// Point cloudflared (`--url` or the ingress rule) at http://{tap.addr}.

let hook = lens
    .wait_for(
        &Filter { methods: vec!["POST".into()], path: Some("/webhooks/".into()), ..Filter::default() },
        &WaitOptions::default(),
    )
    .await?;
println!("{}", export::markdown(&hook, &Redaction::masked()));
```

| Area | API |
|---|---|
| Runtime | `Lens::new`, `start_tap`, `add_tap` + `listen(ListenOptions)`, `update_tap`, `remove_tap`, `set_routing`, `close_listener`, `shutdown` |
| Upstreams | `Upstream::Origin(OriginConfig)` (http/https, `verify_tls`, `server_name`, `http2`, timeouts, pool), `Upstream::Folder(FolderConfig)` (index, listing, SPA fallback, dotfiles) |
| Listeners | loopback only unless `allow_non_loopback`; `Routing::Tap` or `Routing::Hosts` (exact and `*.` wildcards, fallback); pluggable `Acceptor` (plain TCP now, rustls later); `Limits` |
| Capture | `Exchange` (timings, client, request/response heads and capped bodies, kind, stream stats, error, responder, `replay_of`), `CaptureStore` trait + `MemoryStore` ring, `Query`/`Filter`/`Page`, `subscribe()` → `LensEvent`, `wait_for` |
| Reading | `Exchange::view(&Redaction)` → serializable `ExchangeView`; `decode_body`, `content_kind`; `mask_*` helpers |
| Replay | `Lens::replay(id, ReplayOptions { edits, times, target, resign, timeout })` |
| Export | `export::{curl, httpie, fetch, raw_http, har, har_string, json_string, markdown, export}` |
| Webhooks | `webhook::{detect, verify, verify_exchange, resign}` for Stripe, GitHub, Slack, Shopify, Standard Webhooks (Svix, Clerk, Resend…), Twilio, Linear, Discord |
| Per-tap features | `Gates` (password page, secret link, basic auth, bearer tokens, IP allow/deny, user-agent presets, bypass paths), `StubRule` (always / when unreachable), `HeaderRules` (+ CORS helper), `Injection` + `ReservedHandler` (`/__teitunnel/…`), `PausedPage`, `sse_keepalive` |
| Simulation | `NetworkConfig` (`Latency::{THREE_G, FOUR_G, SATELLITE}` or custom, up/down bytes per second), `FaultRule` + `FaultAction::{Status, Reset, Delay, Timeout}` on a share of matching requests; `LensOptions::random` for deterministic tests |
| Metrics | `Lens::metrics(tap)` → counts by status class, errors, blocked, stubbed, bytes, active connections/requests/streams, latency p50/p95/p99 |

## Guarantees

- **Streaming.** Request and response bodies are forwarded frame by frame with hyper;
  the capture copies at most `max_body_bytes` (1 MiB default) per body. SSE, chunked
  responses, long polls, uploads and downloads of hundreds of MB pass without memory
  growth, with backpressure (tests prove the origin receives an upload before it ends,
  an SSE event arrives before the stream ends, and a stalled reader stalls the origin).
  Upgrades (WebSocket and any `Upgrade:`, including from `POST`) are tunnelled byte for
  byte. WebSocket frames are parsed from a copy in both directions: each frame's
  direction, time, opcode, fin, mask, size, preview (UTF-8 text or hex, 4 KiB) and close
  code/reason; the last 500 frames are kept (older ones counted). `permessage-deflate`
  messages are inflated for previews by one bounded inflater per direction (following
  context takeover, capped at 16 MiB of output per message); if inflating isn't
  possible the preview is marked unavailable. h2c from cloudflared
  and HTTP/2 to the origin (`OriginConfig::http2`) carry gRPC with trailers.
- **Headers.** Hop-by-hop headers (RFC 9110 §7.6.1, including those named in
  `Connection`) are dropped; `TE: trailers` survives; no `Via`. `X-Forwarded-For/-Proto/
  -Host` are kept from cloudflared and added only when missing (`ForwardedHeaders`).
  `Host` is preserved by default (`HostHeader::Upstream` or `Custom` to override).
- **Secrets.** Every read path takes a `Redaction`; the default masks credential and
  signature headers (keeping the auth scheme, cookie names and `Set-Cookie` attributes),
  secret-named query/form/JSON values, and known token formats (JWT, Stripe, GitHub,
  Slack, AWS, Google, OpenAI/Anthropic keys, PEM private keys). Text search runs on the
  masked view, so a hidden secret can't be found by guessing. Login form bodies are never
  captured. `Secret`, `PasswordGate`, `SecretLink`, `BasicAuth`, `BearerToken`, `WebhookSecret` and the
  session key never print in `Debug`.
- **Gates.** Argon2id password hashes; sessions are `HttpOnly; SameSite=Lax; Secure`
  (`__Host-` prefixed) cookies signed with HMAC-SHA256 under a random per-instance key,
  bound to the tap and a fingerprint of the credentials (changing them signs everyone
  out), checked in constant time; 10 failed passwords per IP per 10 minutes, Argon2 work
  bounded by a semaphore; open redirects refused; pages escape everything and ship a
  strict CSP. `CF-Connecting-IP` is used for IP rules only with
  `trust_cf_connecting_ip`. Bearer tokens are compared in constant time against every configured
  token. Lens's session cookie and validated basic or bearer credentials are removed
  before forwarding.
- **SSE keep-alive.** Cloudflare ends a response that stays silent for 100 s (524 on
  Free/Pro). For `text/event-stream` responses Lens writes `: keep-alive` after 25 s of
  downstream silence (configurable, `None` to turn off), only between events: if the
  origin is mid-event, it waits. Heartbeats aren't part of the capture.
- **Simulation.** Latency is added before a request is handled; bandwidth limits are
  token buckets shared per tap and direction, pacing streamed bodies in 16 KiB chunks
  without buffering. Fault rules match method and path, fire for a percentage drawn from
  the injectable `RandomSource`, and mark the exchange (`Exchange::fault`); a reset
  closes the connection without a response.
- **Limits.** Head size 64 KiB, 128 headers, 16 KiB request target, 30 s header-read
  timeout (also bounds idle keep-alive connections), 4,096 connections per listener,
  256 h2 streams per connection; decompression for display capped at 8 MiB.
- **Folders.** Paths are decoded once and split; `..`, `\`, NUL, `:` and dot segments
  (except `.well-known`) are refused before touching the disk; the canonical path must
  stay inside the canonical root, so symlinks leaving it are refused.
- No `unsafe`, no `unwrap` outside tests; parsers, masking and exports are property-tested.

## Overhead

Measured with `cargo bench -p teitunnel-lens --bench overhead` (release, Apple M-series,
loopback, one keep-alive connection, 20,000 sequential small `GET`s, capture on):

| | p50 | p95 | p99 |
|---|---|---|---|
| direct | 72 µs | 537 µs | 1.5 ms |
| via Lens | 256 µs | 1.2 ms | 3.1 ms |
| added | 183 µs | 626 µs | 1.6 ms |

256 MiB download: 1,018 MiB/s direct, 814 MiB/s through Lens.

These were taken while other builds kept the machine's load average near 70, which
inflates the tails for both paths (direct p99 moved between 1.5 and 12 ms across runs);
the p50 difference (one extra loopback hop through hyper, plus capture) is the stable
figure. Re-measure on an idle machine before quoting the p99 budget. Upstream
connections are pooled and returned as soon as a response body ends (a test asserts one
origin connection serves sequential requests).

## Deferred and limits

- TLS termination (local HTTPS domains): the `Acceptor` trait is the hook; a rustls
  acceptor with SNI → `Accepted::server_name` comes with M12-07.
- Persistence: implement `CaptureStore` in core (e.g. wrap `MemoryStore` and write to
  SQLite on a blocking thread); Lens has no SQLite.
- HTML injection strips `Accept-Encoding` on page navigations and injects while streaming
  (holding back only bytes after the last `</body>`); origins that compress anyway are
  decompressed and injected only up to `max_html_bytes`. Pages with a strict CSP may
  refuse the injected script.
- WebSocket `permessage-deflate` messages are counted, but previews show compressed bytes
  (`MessagePreview::compressed`). HTTP/2 extended CONNECT (WebSockets over h2) isn't
  proxied; cloudflared uses HTTP/1.1 upgrades.
- Network simulation doesn't apply to upgraded (WebSocket) tunnels after the switch,
  and latency is one delay per request (not per packet).
- Replays run sequentially and skip gates, stubs, faults and simulation; Twilio and Discord signatures can't
  be recomputed (the public URL and Discord's private key aren't known).
