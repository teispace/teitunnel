# Decision Log

Append-only. Never delete a decision. If one changes, add a new entry that supersedes the old one and mark the old one **Superseded by D-NNN**.

Format: `D-NNN · date · title`: decision, why, alternatives considered.

---

### D-001 · 2026-09-22 · Rewrite from scratch
**Decision:** Archive the prototype (tag `legacy-prototype`, branch `legacy/prototype`) and rebuild on a new layout.
**Why:** Its tangled auth, parallel tunnel models, tokens in the webview, and oversized views made fixing it more expensive than rewriting it.
**Alternatives:** An incremental refactor was rejected because nearly every file needed replacing.

### D-002 · 2026-09-22 · macOS first
**Decision:** macOS UX polish comes first through v1.0. Linux and Windows must compile and pass CI from M0, and are polished in M7/M8.
**Why:** It's the maintainer's platform, where "native" is most visible, and focus ships faster.

### D-003 · 2026-09-22 · Route-centric product model
**Decision:** The primary noun is **Route** (hostname → local service). Tunnels are auto-provisioned (one per machine by default) and shown in an Advanced view.
**Why:** Users think in hostnames. Multi-domain on one machine is simply N routes on one tunnel.

### D-004 · 2026-09-22 · Plan → apply engine for every mutation
**Decision:** All mutations are Intents, planned against an observed snapshot, previewed, executed with activity logging and rollback, then verified.
**Why:** One code path powers previews, drift handling, cleanup, Doctor fixes and imports, and prevents half-applied changes.

### D-005 · 2026-09-22 · Cloudflare is the source of truth
**Decision:** Tunnel config and DNS are read from Cloudflare. SQLite stores only ownership, run modes, ports, applied versions, activity and preferences.
**Why:** It's safe with multiple machines and dashboard edits. Drift is detected through the config `version`.

### D-006 · 2026-09-22 · Ownership via DNS record comments
**Decision:** Records we create carry `teitunnel:route=<id>`. Only owned records are auto-deleted.
**Why:** It gives zero-mess cleanup without risking user records, and ownership can be rebuilt on a new machine. Comments work on all plans; tags don't.

### D-007 · 2026-09-22 · Minimum cloudflared 2025.6.1
**Decision:** Require cloudflared ≥ 2025.6.1 (`--output json`; `--token-file` needs ≥ 2025.4.0). The managed binary tracks the latest release. There's no text-log parser.
**Why:** Structured logs are reliable, and the managed binary makes the requirement painless.

### D-008 · 2026-09-22 · Status from local endpoints, not log scraping
**Decision:** Use cloudflared's metrics server: `/ready` (health), `/quicktunnel` (Quick Share URL), `/metrics` (telemetry), `/diag` (diagnostics).
**Why:** It's stable and structured, and survives log format changes.

### D-009 · 2026-09-22 · Always-on via OS service manager, no custom daemon
**Decision:** Always-on connectors are launchd agents (then systemd --user and Task Scheduler). The app observes them through fixed metrics ports.
**Why:** Production-grade persistence without writing and securing our own daemon.

### D-010 · 2026-09-22 · Auth: OAuth (PKCE, loopback) first, API token second, cert.pem import third
**Decision:** Register a public Cloudflare OAuth client under the verified teispace.com domain. Use loopback redirects on fixed registered ports, since custom schemes aren't allowed. Ship the token flow first as a fallback.
**Why:** Cloudflare opened self-managed OAuth to all customers in June 2026, and it's the best onboarding UX.

### D-011 · 2026-09-22 · Secrets never cross IPC; run tokens never in argv
**Decision:** See [SECURITY_MODEL.md](SECURITY_MODEL.md).

### D-012 · 2026-09-22 · Workspace layout
**Decision:** A Cargo + pnpm workspace: `crates/{cf-api,cloudflared,core}`, `apps/desktop/{src,src-tauri}`, `tools/fake-cloudflared`. Core logic is Tauri-free.
**Why:** Testability, clear boundaries, and room for a future CLI.

### D-013 · 2026-09-22 · Frontend stack
**Decision:** React 19, TypeScript (strict), Vite, Tailwind v4, shadcn/Radix (re-tokened), TanStack Router + Query + Virtual, Zustand (UI state only), react-hook-form + zod, cmdk, sonner, uPlot, lucide, motion (springs; see D-022). Biome for lint/format.
**Why:** Typed routing and server-state caching remove hand-written sync code. uPlot is roughly 10× lighter than Recharts and suited to realtime use.
**Dropped:** Recharts, xterm.js (terminal removed), qrcode.react (QR is rendered as SVG in Rust).

### D-014 · 2026-09-22 · Typed IPC via tauri-specta
**Decision:** Generate `bindings.ts` from Rust. CI fails on drift.
**Note:** tauri-specta 2.x is still RC (2.0.0-rc.25 as of 2026-09). Pin the exact version. If it becomes a blocker, the fallback is `ts-rs` plus hand-written command wrappers.

### D-015 · 2026-09-22 · Tauri 2 stable, not 3 alpha
**Decision:** Build on Tauri 2.11.x. Re-evaluate once Tauri 3 is stable.

### D-016 · 2026-09-22 · In-app dev gallery instead of Storybook
**Decision:** Primitives and patterns are showcased at a dev-only route inside the real app window.
**Why:** Native fidelity (WKWebView rendering, vibrancy, system accent) can't be judged in a browser Storybook.

### D-017 · 2026-09-22 · E2E testing via WebdriverIO `@wdio/tauri-service`
**Decision:** Use the embedded WebDriver approach, which supports macOS. Playwright is used for component/visual tests against mocked IPC in the WebKit engine.

### D-018 · 2026-09-22 · Bundle identifier `com.teispace.teitunnel`
**Decision:** Use it as the app identifier, keychain service name, launchd label prefix and app data folder name.
**Why:** It matches the teispace.com domain, which is also used for OAuth domain verification.

### D-019 · 2026-09-22 · No telemetry
**Decision:** No analytics, crash upload or phone-home other than update checks, which can be disabled.

### D-021 · 2026-09-22 · Materials: NSVisualEffectView + CSS materials, no NSGlassEffectView (for now)
**Decision:** The sidebar uses native vibrancy via `windowEffects`. Floating toolbar controls use restrained CSS materials. The private `NSGlassEffectView` is not used.
**Why:** It crashes packaged Tauri apps on macOS 27 (tauri-apps/window-vibrancy#229). macOS 27 moved sidebars back to edge-to-edge, which NSVisualEffectView renders natively.
**Revisit:** each milestone, via the issue.

### D-022 · 2026-09-22 · Motion: SwiftUI-style springs
**Decision:** Spring tokens (duration + bounce) are shared by CSS `linear()` easings (simple transitions) and `motion/react` (layout, presence, gestures). Interruptible by default.
**Why:** This matches Apple's motion model. CSS keeps simple transitions free of JS cost.

### D-020 · 2026-09-22 · Remove the embedded terminal
**Decision:** No arbitrary command runner. It's replaced by a structured log viewer and "Copy as command" on every plan step and activity entry.
**Why:** It was a security risk with low value.

### D-023 · 2026-09-22 · Minimum macOS 14, `light-dark()` tokens, accent from AppKit
**Decision:** `minimumSystemVersion` is 14.0 and the web build targets Safari 17. Colour tokens use CSS `light-dark()` and `color-scheme`, so each token is defined once and the theme override only pins `color-scheme`. The accent colour is read from `NSColor.controlAccentColor` (command `app_accent_color`) and applied as `--accent` on start and whenever the window regains focus.
**Why:** Verified on macOS 27: WKWebView resolves CSS `AccentColor` to a fixed blue `(52,120,246)` even when the system accent is green, while System Settings follows it. Reading AppKit gives values identical to System Settings, updated live. `light-dark()` removes a whole duplicated dark block.

### D-024 · 2026-09-22 · Dark sidebar tint over native vibrancy
**Decision:** The sidebar keeps the native `sidebar` NSVisualEffectView, with a dark-mode-only tint `rgb(0 0 0 / 0.42)` (`--surface-sidebar`).
**Why:** Measured on macOS 27: Finder and System Settings sidebars render ~`(38,40,41)` in dark mode, while the NSVisualEffectView `sidebar` material renders ~`(69,70,70)`. The tint lands at `(40,41,41)` and keeps some translucency. Light mode already matches (231 vs 236).

### D-025 · 2026-09-22 · macOS 27 sidebar and title-bar metrics
**Decision:** Sidebar rows are 32 px with 18 px accent-tinted icons, 10 px side inset and 8 px selection radius. Traffic lights use `trafficLightPosition {x: 19, y: 28}`, which puts the buttons at exactly the same pixels as System Settings (close button 19–32.5 pt × 20–31.5 pt). Selection happens on mouse-down, as in NSOutlineView.
**Why:** Measured from System Settings and Finder screenshots on macOS 27. This supersedes the 28 px row / 6 px radius / (18,18) values in the first DESIGN draft.

### D-026 · 2026-09-22 · Dependabot instead of Renovate; no `tauri-plugin-os`
**Decision:** Weekly grouped Dependabot updates (Cargo, npm, Actions), with Tauri majors and specta RCs ignored. The `os` plugin is dropped because `app_info` already reports platform and arch.
**Why:** Dependabot needs no third-party app installation (a maintainer-only action). Fewer plugins means a smaller attack surface.

### D-027 · 2026-09-22 · Respect pnpm's minimum release age
**Decision:** Keep pnpm 12's default supply-chain policy (packages must be at least a day old). Pin the previous release when the latest is too new (e.g. motion 13.4.0, jsdom 30.1.0).
**Why:** It's a cheap defence against compromised fresh releases, consistent with SECURITY_MODEL.

### D-028 · 2026-09-22 · Settings is a separate native window
**Decision:** ⌘, opens a dedicated Settings window (620×460, not resizable, overlay title bar) that loads the `/settings` route, instead of a page inside the main window.
**Why:** That's the macOS convention (every native app's Settings is its own window). It also keeps preferences reachable while the main window is hidden.

### D-029 · 2026-09-22 · Closing the window never stops tunnels; "Show in menu bar" controls the icon
**Decision:** Closing the main window hides it; the app keeps running (Dock icon and menu bar item reopen it) and ⌘Q quits. The setting is `showInMenuBar` (default on), which only toggles the menu bar icon.
**Why:** Matches Mail/Messages behaviour and avoids surprising tunnel shutdowns. The quit confirmation for running Session connectors lands with connectors in M1.

### D-030 · 2026-09-22 · WebKit (Playwright) screenshots when the native window can't be captured
**Decision:** `pnpm --filter @teitunnel/desktop shoot <dir> [routes…]` renders routes in Playwright's WebKit (light and dark) with the measured sidebar material painted in. It's used for visual review in CI-like conditions and when the screen is locked during unattended sessions. Native captures remain the reference for vibrancy, traffic lights and accent.
**Why:** macOS doesn't composite windows while the screen is locked, so `screencapture` returns blank content. Headless WebKit also doesn't render `backdrop-filter`, so materials must stay readable without blur.

### D-031 · 2026-09-22 · Two CSS materials: glass (controls) and panel (popovers, menus, palette, toasts)
**Decision:** `material-glass` (62% tint) is only for small controls floating over content. Panels use `material-panel` (94% tint, 30 px blur). Both become opaque under Reduce transparency.
**Why:** At 62% the text underneath competed with list items in the command palette. Near-opaque panels match macOS menus and stay legible even where blur isn't rendered.

### D-032 · 2026-09-23 · No `Spawner` port; tests use the fake binary
**Decision:** The supervisor spawns processes directly with `tokio::process`. Tests point it at `tools/fake-cloudflared` (a real binary), and the supervisor integration tests live in that package so Cargo always builds the fake first (`CARGO_BIN_EXE_fake-cloudflared`).
**Why:** A real child process exercises signals, process groups, pipes and HTTP polling, which a mocked spawner wouldn't. One fewer abstraction.

### D-033 · 2026-09-23 · Merging to `main` needs the maintainer; milestones stack
**Decision:** Unattended sessions don't merge PRs (the environment's permission policy blocks merge-without-review, rightly). When a milestone is done, its PR is marked ready and the next milestone continues on a branch stacked on it, with a draft PR targeting the previous milestone branch. After the maintainer merges, the next PR is retargeted to `main`.

### D-034 · 2026-09-23 · A Quick Share is "live" only when a connection is registered
**Decision:** Show the URL as ready only when `/quicktunnel` has a hostname **and** `/ready` reports ≥ 1 connection.
**Why:** Measured with cloudflared 2026.9.1: `/quicktunnel` returns the hostname about 2 s before the first edge connection registers (`readyConnections: 0`); opening the URL in that window fails.

### D-035 · 2026-09-23 · E2E harness is compiled in only with `--features e2e`
**Decision:** The embedded WebDriver (`tauri-plugin-wdio-webdriver`), `tauri-plugin-wdio`, its JS bridge, its capability and `withGlobalTauri` are enabled only in E2E builds (`pnpm e2e:build`: cargo feature `e2e`, a `--config` overlay, `VITE_E2E=1`). Release builds contain none of it (checked: no wdio code in `dist/`). E2E builds refuse to start unless `TEITUNNEL_CLOUDFLARED` points at a binary, and `TEITUNNEL_DATA_DIR` isolates their data.
**Why:** An embedded WebDriver can drive the entire UI. The first E2E attempt, before the guard, ran the real cloudflared and opened a real Quick Share; it was stopped immediately.

### D-036 · 2026-09-23 · Exit animations are skipped while the page is hidden
**Decision:** List exit animations only run when `document.visibilityState` is `visible`.
**Why:** WebKit runs no animation frames for a hidden window, so a share stopped from the menu bar would linger in the list until the window reappeared and then animate out. Verified by the E2E run (0 frames while hidden).

### D-037 · 2026-09-23 · Quick Shares go "Live" 6 s after the hostname appears, without DNS queries
**Decision:** After `/quicktunnel` reports a hostname and a connection is registered, wait until 6 s have passed since the hostname first appeared before marking the share live (the URL is shown meanwhile, Open disabled). Teitunnel never queries DNS for the new name itself.
**Why:** Measured: new names aren't resolvable for ~2–3 s, and NXDOMAIN is cached for up to 30 minutes (SOA minimum 1800). A DoH readiness check was tried and rejected, because querying 1.1.1.1 too early caches the NXDOMAIN there, breaking the URL for everyone using that resolver. The nightly real test confirms the approach (public fetch succeeds on the first try).

### D-038 · 2026-09-23 · Permission probing by PATCHing a nil id
**Decision:** Capability checks never write. Reads use one-item lists; write permission is inferred from `PATCH …/<all-zero id>`: 404 means authorized (no such object), 403 / code 10000 means not authorized.
**Why:** API tokens can't read their own permissions without an extra "API Tokens Read" grant. A write probe on a nil id is side-effect free by construction; tests assert no POST/DELETE is ever sent.

### D-039 · 2026-09-23 · The active account is UI state
**Decision:** Which account Domains/Routes show is remembered in the persisted UI store, not in the backend; the engine works with all accounts.
**Why:** ARCHITECTURE §6 (all accounts active in the engine). Keeps commands stateless (`domains_list(accountId)`).

### D-040 · 2026-09-23 · Route checks bypass DNS entirely
**Decision:** The verifier confirms the DNS record through the Cloudflare API, then sends its HTTPS probe directly to a Cloudflare edge address (resolved from `api.cloudflare.com`) with the route's hostname as SNI/Host. It never resolves the route's hostname.
**Why:** Same failure mode as D-037: resolving a brand-new name too early caches NXDOMAIN in the local and upstream resolvers for up to the zone's SOA minimum, breaking the URL the user is about to open. Cloudflare's edge serves any proxied hostname on any of its anycast addresses ("Addressing Agility"), so the probe exercises the real edge → tunnel → origin path without touching DNS.


### D-041 · 2026-09-23 · The machine tunnel is never adopted by name
**Decision:** A new machine tunnel is named after the host name; if the account already has a tunnel with that name, the new one becomes "name 2", "name 3"… Teitunnel only reuses the tunnel recorded in `tunnels_local`.
**Why:** Two Macs can share a host name. Adopting by name would make both run connectors for one tunnel, and Cloudflare would load-balance one route across two machines. Import of foreign tunnels is an explicit M4 flow.

### D-042 · 2026-09-23 · Route ids are derived from hostname and path
**Decision:** A route's id (written into its DNS comment, `teitunnel:route=<id>`) is the first 6 bytes of SHA-256(hostname ∖0 path), hex.
**Why:** Preview and apply are separate IPC calls that re-plan; a derived id makes both plans identical without storing state in between, and the ownership comment stays recoverable from the route itself.

### D-043 · 2026-09-23 · E2E runs against a fake Cloudflare with in-memory secrets
**Decision:** `tools/fake-cloudflare` implements the API endpoints the engine uses and answers route probes as the edge. Builds with the `e2e` feature read `TEITUNNEL_API_BASE` / `TEITUNNEL_EDGE` and use `MemoryStore` for secrets; release builds can't do either.
**Why:** End-to-end tests must never touch the maintainer's Cloudflare account or login keychain, and must be deterministic in CI. The real API is exercised by the nightly job once a test token exists.

### D-044 · 2026-09-23 · Importing an existing setup moves its routes onto this Mac's tunnel
**Decision:** Routes found in `config.yml` files are imported as routes of this Mac's remotely-managed tunnel (`Intent::ImportRoutes`), one combined plan. DNS records that point at the old tunnel aren't Teitunnel's, so repointing them needs confirmation. The files and the old tunnel are left untouched. Per-route `originRequest` settings block a route's import for now; global ones are reported. YAML is read with `serde-saphyr` (maintained, deserialize-only; `serde_yaml` is deprecated and `serde_yml` has a RustSec unsoundness advisory).
**Why:** Keeps one model (routes on one machine tunnel, plan → apply, ownership) instead of a second locally-managed mode with YAML rewriting, and leaves the user a working fallback until they delete the old setup themselves.

### D-045 · 2026-09-23 · Always-on connectors are launchd agents the app installs, never supervises
**Decision:** Always-on runs `cloudflared tunnel run --token-file` as a per-user launch agent (`com.teispace.teitunnel.connector.<id>`, KeepAlive, RunAtLoad) with its JSON log redirected to `<app_data>/logs/connectors/<id>.log`. The app writes the plist and runs `/bin/launchctl bootstrap|bootout|print gui/<uid>` through typed builders; health comes from the connector's `/ready`. Switching modes starts the new connector on a fresh metrics port and stops the old one only after the new one has an edge connection. E2E builds substitute `ProcessServices` (child processes) so tests never install real agents.
**Why:** launchd gives restart-on-crash and start-at-login without a privileged helper; keeping the app an observer means quitting it can't take routes down. A fresh port avoids clashing with the running connector during the switch.


### D-046 · 2026-09-23 · Live traffic is polled with a cursor; sampling speeds up while polled
**Decision:** The UI reads live traffic with `tunnels_traffic(tunnel, since)` once a second and appends only the samples newer than `since`. Each read renews a 5 s lease that samples that connector at 1 s; otherwise it is sampled every 10 s. The last 3,600 samples stay in memory per tunnel; finished minutes are persisted to `metrics_rollup` (7-day retention, pruned on write, upserted so restarts mid-minute add up) and served bucketed (5 min for a day, 30 min for a week) by `tunnels_traffic_history`. Series cross IPC as columns (`TrafficSeries`), the shape uPlot draws. Charts are uPlot on canvas (`TimeSeriesChart`) with colours resolved from the tokens at paint time; the SVG `Sparkline` stays for tiny inline use.
**Why:** A pull with a cursor needs no subscription bookkeeping: nothing leaks when a view unmounts, the webview reloads or the window hides (TanStack pauses polling), and a missed poll loses nothing. The lease keeps the scrape rate tied to someone actually looking. Columns roughly halve the JSON and need no reshaping before drawing.

### D-047 · 2026-09-23 · Activity entries carry a structured record next to their text
**Decision:** Each applied plan stores an `ActivityRecord` (JSON in `activity.record`, migration 6): its kind, every hostname involved, each step's `StepView` with its final `StepState` (taken from the same progress events the UI sees), and a before/after list (`Delta`) derived purely from the plan (ingress `previous` vs new, keyed by hostname + path; DNS records' previous values). The plain `detail` lines stay for search and for entries written before the record existed; a record a newer version wrote that this one can't read falls back to them. Steps a rollback reverses implicitly (a new tunnel's config goes with the tunnel) are reported `Undone` once the rollback succeeds. The Activity view filters by kind or problems and by domain (the account's zones, matched as apex or subdomain; a tunnel filter is moot with one machine tunnel per account), shows failed changes as "Attempted Changes" in neutral colours, and offers "Check" only for hostnames that are routed now.
**Why:** Diffs, per-step commands and filters need structure; re-deriving it from sentences would be fragile. Recording from the executor's own events means the log can't disagree with what the user watched happen.
