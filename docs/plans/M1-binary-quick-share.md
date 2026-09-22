# M1: cloudflared management + Quick Share

**Goal:** someone with no Cloudflare account installs Teitunnel, clicks once, and gets a public URL for localhost with a QR code. The runtime foundations (binary manager, supervisor, endpoints, logs) must be production grade, because every later milestone runs on them.
**Release:** **v0.1.0** (macOS, developer preview).
**Exit criteria:**
- First run on a clean Mac: detect or install cloudflared (verified), and share `localhost:3000` in ≤ 5 s to a working `trycloudflare.com` URL.
- Several concurrent Quick Shares work. Stop is clean: no orphan processes after quit, crash or force-quit of the app (verified).
- Supervisor integration tests cover crash, restart backoff, crash-loop, slow start, and SIGTERM-ignored → SIGKILL.

---

### M1-01 · `cloudflared::locate` + version
- [ ] Search order: managed (`<app_data>/bin/cloudflared`), then `$PATH`, `/opt/homebrew/bin`, `/usr/local/bin`, `/usr/bin` (Windows: `Program Files`, `%LOCALAPPDATA%`).
- [ ] `Version` parse from `cloudflared --version` (`cloudflared version 2026.9.1 (built …)`), with `MIN_SUPPORTED = 2025.6.1`.
- [ ] `BinaryStatus { path, source: Managed|System, version, supported, latest? }`.
- [ ] Tests: version parsing fixtures (old formats, dev builds), search precedence with a temp dir.

### M1-02 · `cloudflared::install` (managed binary)
- [ ] Resolve the latest release via the GitHub API (`/repos/cloudflare/cloudflared/releases/latest`), with an ETag cache and no auth. Rate limit handling: fall back to the `releases/latest/download/<asset>` redirect for the version.
- [ ] Asset selection per OS/arch (`darwin-arm64.tgz`, `darwin-amd64.tgz`, `linux-*`, `windows-amd64.exe`).
- [ ] Streamed download with progress events (bytes/total), cancellable.
- [ ] Verify SHA256 against the checksum line in the release body (parser + fixtures).
- [ ] macOS: extract the tgz, then `codesign --verify --strict` and check the TeamIdentifier equals Cloudflare's (look it up from a real binary with `codesign -dv`, record it in research/cloudflare.md, and hard-code it).
- [ ] Atomic install: write to `bin/.staging`, `rename` into place, keep `bin/cloudflared.prev` for rollback. Mode 0755.
- [ ] Update check (daily, on launch; setting to disable). The update is applied only while no managed connector is running, or after asking.
- [ ] Tests: wiremock GitHub + a fixture tarball with a known hash; hash mismatch aborts and leaves the current binary untouched.

### M1-03 · `cloudflared::command` builders
- [ ] `QuickTunnelCmd { origin, metrics_port }` and `RunCmd { token_source: Env|File(path), metrics_port, protocol, edge_ip_version, log_dir?, loglevel }`. They produce `(program, args, env)`.
- [ ] Always adds `--no-autoupdate --output json --metrics 127.0.0.1:<port>`. The token is **never** placed in args (compile-time: `Secret` isn't `Into<OsString>`).
- [ ] `to_display_command()` for "Copy as command", with secrets rendered as `$TUNNEL_TOKEN`.
- [ ] Tests: snapshot of args/env per builder; a test asserting no secret appears in the args.

### M1-04 · `cloudflared::log_parse`
- [ ] Parse JSON lines into `LogEvent { ts, level, message, fields: Map, connection_index?, location?, error? }`. Tolerate unknown fields. Treat non-JSON lines (panics, early startup) as `level=raw`.
- [ ] Classify known events: `registered connection` (colo, connIndex), `unregistered`, `retrying`, `quick tunnel url`, `origin error` (so Doctor can use them later).
- [ ] Fixture corpus captured from real cloudflared 2026.9.x runs (quick tunnel, run with token, origin down, UDP blocked). Store them in `fixtures/cloudflared/`.

### M1-05 · `cloudflared::endpoints`
- [ ] Client for `127.0.0.1:<port>`: `ready() -> Ready { status, ready_connections, connector_id }`, `quicktunnel() -> Option<Hostname>`, `metrics() -> MetricsSnapshot` (Prometheus text parser: counters/gauges/histograms needed per research doc), `healthcheck()`.
- [ ] Short timeouts (500 ms) and no retries inside; the caller decides.
- [ ] Tests: fixture Prometheus payloads; parser property test (never panics on arbitrary input).

### M1-06 · `tools/fake-cloudflared`
- [ ] A binary that mimics the CLI surface we use: parses `tunnel … run`/`--url`, serves `/ready`, `/quicktunnel`, `/metrics`, and emits JSON logs.
- [ ] Behaviour scripted via env (`FAKE_CFD_SCENARIO=healthy|slow_start|crash_after:5s|degraded|ignore_sigterm|no_url`).
- [ ] Also acts as a tiny origin for E2E if needed.

### M1-07 · `core::runtime` supervisor v1
- [ ] `Supervisor` actor owning `Connector` actors. The state machine is exactly as in ARCHITECTURE §5.1, and transitions are emitted as events.
- [ ] Metrics port allocator (`20300..20399`, bind-probe, persisted per tunnel from M3; ephemeral for Quick Share).
- [ ] Spawn via the `Spawner` port: own process group, `kill_on_drop`, stdout/stderr → line reader → `log_parse` → ring buffer (100k) + broadcast.
- [ ] Health loop: `/ready` every 2 s. Restart policy with exponential backoff + jitter; crash-loop detection (> 5 in 2 min).
- [ ] Stop: SIGTERM → 5 s → SIGKILL (Unix via `nix`/`rustix`; Windows: `CTRL_BREAK_EVENT` then `TerminateProcess`).
- [ ] **Orphan safety:** write a pidfile registry (`<app_data>/run/*.json` with pid + start time + our marker). On launch, reap orphans whose start time matches (the app was force-quit). Never kill PIDs we didn't start.
- [ ] App exit hook: stop all Session connectors concurrently within the deadline.
- [ ] Tests (with fake-cloudflared): every scenario above, plus exit-hook timing and orphan reaping.

### M1-08 · Minimal local-service discovery
- [ ] `core::discovery::ports`: listening TCP sockets on loopback/any, with pid → process name (`listeners` + `sysinfo`). Label known dev servers (vite, next, node, python, rails, php, docker-proxy…).
- [ ] Command `services_list` (snapshot). No background polling except while the picker is open.

### M1-09 · Quick Share feature
- [ ] Core: `quick_share_start(origin) -> QuickShareId`, `quick_share_stop(id)`, `quick_share_list()`. The URL comes from polling `/quicktunnel` (20 s timeout, surfaced as a typed error). Optional `stop_after`.
- [ ] Request counter from `cloudflared_tunnel_total_requests` (scraped every 2 s while visible).
- [ ] QR code as SVG, generated in Rust (`qrcode` crate) and returned as a string (no JS lib).
- [ ] History in SQLite (`quick_shares` table migration).
- [ ] UI (`features/quick-share`): origin field with detected services, one primary "Share" button, and a live card with URL (CopyField), Open, QR popover, status, request count, elapsed time, auto-stop menu, Stop. Log drawer (basic list; the full LogViewer comes in M5). Empty state teaches in one sentence.
- [ ] Motion: the URL arrives with `spring.bouncy` success tick; the card uses layout animation on add/remove.
- [ ] Tray: running shares listed with Copy URL / Stop, plus "Share port…" opening the app.

### M1-10 · Onboarding: binary step
- [ ] First-run window content: "Teitunnel uses cloudflared, Cloudflare's connector." It shows detection results, then **Use installed version** / **Install managed version** (progress, verification steps visible) and a "Why?" disclosure.
- [ ] If the system binary is too old, offer managed install. Never modify the user's Homebrew install.
- [ ] Settings → cloudflared pane: source, version, path, check for updates, reinstall, reveal in Finder.

### M1-11 · Notifications
- [ ] `tauri-plugin-notification`: "Quick Share is live" (only when the window isn't focused), and "Quick Share stopped unexpectedly".

### M1-12 · Tests & E2E
- [ ] Vitest flow tests for Quick Share with `mockIPC`.
- [ ] `@wdio/tauri-service` E2E on macOS CI: launch the app with `TEITUNNEL_CLOUDFLARED=fake-cloudflared`, start a share, assert the URL shows, stop, and assert no process is left.
- [ ] Nightly workflow: real cloudflared, real Quick Share against a local HTTP server; fetch the public URL and expect 200.

### M1-13 · Release v0.1.0
- [ ] Decide signing (see STATUS open questions). If there's no Developer ID yet, ship an unsigned DMG labelled "developer preview" with clear Gatekeeper instructions.
- [ ] `release.yml`: on tag `v*`, build macOS arm64 + x64 (or universal), create a GitHub Release with notes.
- [ ] README: screenshots (light/dark), install instructions.
