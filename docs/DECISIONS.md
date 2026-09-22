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
