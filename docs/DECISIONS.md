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

### D-048 · 2026-09-23 · Doctor notifications: one monitor fed by every run; ignores live in settings
**Decision:** `core::doctor_monitor::DoctorMonitor` (pure, like `health`) records every Doctor run, whether the window asked (`doctor_run`) or the background loop did, and returns at most one coalesced notice per run for errors that are new since the previous run. It never notifies for ignored issues, connector outages (`health` covers those) or `account.unreachable` (usually a network blip). The background loop checks every minute and runs only when nothing ran in the last 5 minutes, so while the window is visible its own 5-minute polling is the only run and Cloudflare isn't queried twice; when the window is hidden (TanStack pauses polling) the loop takes over. Ignored issue ids moved from the window's local storage to `settings.ignoredIssues` (changed atomically with `doctor_set_ignored`); the window moves any old local ignores there once. Settings ▸ Notifications ▸ Problems turns the notices off.
**Why:** Problems matter most when the window is closed, and the backend can't honour ignores it can't see. A shared monitor keeps "new since last time" consistent no matter who ran the Doctor.

### D-049 · 2026-09-23 · A cloudflared update moves running connectors over without a gap
**Decision:** The managed binary keeps one path (swapped by rename), so launch agents never point at a missing file, but running connectors keep the old version until restarted. After an install that changes the version or path, the app restarts this Mac's connectors one account at a time (`MachineTunnels::restart_on_current_binary`). An Always-on connector is bridged: a temporary app connector starts and must connect, then the agent is reinstalled on a fresh port and must be ready, and only then does the bridge stop. If the reinstall fails the bridge keeps serving until the next launch reinstalls the agent. A Session connector restarts in place (a few seconds, under the 20 s before "down" notifies). Failures notify and leave whatever was serving running.
**Why:** Updating cloudflared shouldn't take routes down, and a connector that silently stays on an old version defeats the update. Replicas of the same tunnel can run side by side, which makes the bridge safe.

### D-050 · 2026-09-23 · Connector logs: bounded files, tail reads, per-route filtering in the backend
**Decision:** Always-on connector logs (a launchd `StandardOutPath` the agent appends to) are cut back once they pass 8 MiB by copying to `<name>.1` and truncating in place (renaming would leave launchd writing to the renamed file), checked on every sample. Reads take only the tail, backwards in 64 KiB blocks, never more than 2 MiB. `routes_logs` filters a route's lines in the backend by the matched rule's index and service from the ingress Teitunnel applied (`connector_logs::RouteFilter`); level and text filters stay in the window, where they respond as you type over at most 1,000 lines.
**Why:** An unrotated file grows for as long as the agent runs, and reading it whole on every refresh scaled with its age. Filtering by route needs the applied ingress, which only the backend has; level/text filtering in the window costs nothing at this size.

### D-051 · 2026-09-23 · One neutral service definition; exit criteria are measured, not asserted
**Decision:** `cloudflared::service::ServiceSpec` (label, program, args, log file; tokens only from a file) replaces the launchd-specific `LaunchAgent`; `launchd`, `systemd` and `task_scheduler` each render it and build their own typed commands, and `core::service::{Launchd, Systemd, TaskScheduler}` implement `ServiceManager` over it. Only launchd is selected at runtime until M7/M8. Performance exit criteria get a dev-only bench (`src/dev/stress.tsx`) and a script (`pnpm --filter @teitunnel/desktop perf [seconds]`) that drives it in WebKit and reports frame percentiles and DOM size.
**Result (2026-09-23, WebKit, 30 s):** log at ~2,000 lines/s plus a 3,600-point chart at 20 Hz together: 60 fps, p50 17.0 ms, p99 19.0 ms, max 20.0 ms, no frame over 50 ms; DOM constant at 186 nodes.
**Why:** The platforms differ only in how the same thing is written down and started, so the spec shouldn't be launchd's. Claims like "60 fps at 2,000 lines/s" should be re-checkable by anyone, any time.

### D-052 · 2026-09-23 · Accessibility: native colours by default, WCAG AA under Increase Contrast, audited in CI-able script
**Decision:** The default appearance keeps macOS's label and accent colours, whose secondary/tertiary text and white-on-accent sit below 4.5:1 by Apple's design. With Increase Contrast on, the app meets WCAG 2.2 AA contrast everywhere: stronger text tiers, an `--accent-fill` token (accent mixed 78% with black) for every fill that carries white text, and Apple's accessible status colours, nudged to keep 4.5:1 on grey control surfaces. `pnpm --filter @teitunnel/desktop a11y` runs axe-core on every screen in WebKit in light/dark × default/increased contrast: all rules except colour contrast in the default appearance, all rules under Increase Contrast. Result on 2026-09-23: no violations. A manual VoiceOver pass still needs an unlocked screen.
**Why:** Matching the native look is a product goal, and macOS provides Increase Contrast precisely for people who need more contrast. Honouring it fully, and checking it automatically, serves them without making the default look non-native.

### D-053 · 2026-09-23 · Docs site: Astro Starlight in `apps/site`, deployed to GitHub Pages when the maintainer opts in
**Decision:** The user docs are an Astro Starlight site in `apps/site` (Astro 7.3, Starlight 0.42; `passthroughImageService`, so no `sharp` or native install script), served at `https://teispace.github.io/teitunnel/`. Sections: Getting started, Concepts, Guides, Reference (the Doctor catalogue mirrors `core::doctor` check ids). Screenshots are WebP made with `shoot` from the dev mocks, which use neutral names only. `.github/workflows/docs.yml` builds it on every change; deploying needs Pages enabled and the repository variable `DEPLOY_DOCS=true`, both maintainer settings. The Help menu's documentation link stays on the README until the site is live.
**Why:** Starlight gives search, navigation, dark mode and accessible markup with no custom code, and lives in the same workspace and lockfile as the app. Keeping deployment opt-in respects that Pages and repository variables are the maintainer's call.

### D-054 · 2026-09-23 · Windows and Linux: services selected at runtime, no console windows, clear credential-store errors
**Decision:** The app picks the service manager per platform at startup: launchd on macOS, `systemd --user` on Linux when a user session exists (`XDG_RUNTIME_DIR`), Task Scheduler on Windows. `ServiceManager::captures_output` tells the machine whether the manager saves the connector's output (launchd, systemd: the app bounds and tails `<tunnel>.log`) or not (Task Scheduler: the connector runs with `--log-directory <logs>/<tunnel>` and rotates its own `cloudflared.log`, 1 MB × 5, per cloudflared's `logger/configuration.go`). Every helper program starts with `CREATE_NO_WINDOW` on Windows. A missing credential store (Linux without a Secret Service) is `SecretError::Unavailable`, whose message says what to install. Code that needs a real Windows or Linux desktop to verify is listed as such in the M7/M8 plans.
**Why:** The platform differences are small and belong in the adapters, not in the machine or the UI; CI can compile and test all of it, and what it can't is recorded rather than assumed to work.

### D-055 · 2026-09-23 · Export renders from the observed state; Terraform adopts via import blocks
**Decision:** `core::export::render` turns what the engine observed (this Mac's tunnel, its ingress, and the proxied CNAMEs pointing at it) into `config.yml`, Docker Compose or Terraform. Terraform targets the Cloudflare provider v5 (`cloudflare_zero_trust_tunnel_cloudflared`, `…_config`, `cloudflare_dns_record`, verified against the provider docs at v5.25.0) and emits `import` blocks (`<account>/<tunnel>`, `<zone>/<record>`), with `originRequest` keys converted to the provider's snake_case and each record's comment, TTL and proxied flag kept, so a first `terraform plan` shows no changes. Strings go through JSON quoting (valid YAML and HCL) and HCL template sequences are escaped. No export contains a secret; the Compose file reads the run token from `${TUNNEL_TOKEN}`.
**Why:** Adopting existing resources is what makes an export useful rather than a template to recreate from; rendering from observed state means it reflects what's really in Cloudflare, including edits made elsewhere.

### D-056 · 2026-09-23 · A CLI over the same core, sharing the app's data, never running connectors
**Decision:** `apps/cli` builds `teitunnel-cli` (clap 4.6) over `teitunnel-core`. It opens the app's data folder (`dirs::data_dir()/com.teispace.teitunnel`, or `TEITUNNEL_DATA_DIR`) and keychain, and changes routes through the same `Change → intent → preview → apply → verify` path as the UI, printing the plan and asking (`--yes` to skip, `--replace` for records Teitunnel didn't create). It never starts or stops connectors: `ProbedConnectors` reports each tunnel's state from its remembered metrics port and refuses start/stop, so a new route is served by the app or an Always-on service. The binary isn't called `teitunnel` because on case-insensitive disks it would be the same file as the app's `Teitunnel` in the shared target directory.
**Why:** One engine means the CLI can't diverge from the app's safety rules. A short-lived process owning connectors would take routes down when it exits.

### D-057 · 2026-09-23 · Logins through Access apps Teitunnel owns, up before a route and down after it
**Decision:** A route's "Require a login" is a `self_hosted` Access application per route (domain = hostname plus the path when it's a plain prefix; regex paths are refused), named `Teitunnel · <domain>`, with one inline allow policy of `email`/`email_domain` rules, hidden from the App Launcher. Ownership is a local index (migration 7), so Teitunnel updates or deletes only applications it created; a foreign application on the domain is left alone (the plan errors unless it already allows exactly the requested people). The planner creates or updates the application before config and DNS go live, and deletes it after the route is gone (before the irreversible tunnel delete); every step is undoable (a deleted application is recreated from its observed definition). One-time PIN is added only when the account has no login method; a missing Zero Trust organization is a plan error. Access is observed only when a change needs it (`AccessNeed`), and a token without Access permissions still manages plain routes (owned-app reads degrade on 401/403). Verify treats a redirect to the Access login as "works, protected". On an edit, no rule removes Teitunnel's login; on an add, it leaves any login alone (Doctor fixes re-add routes that way).
Teitunnel's own logins left behind by an outside route removal are a Doctor issue (`access.orphan`, fixed by a planned `RemoveLogin`). The token template doesn't ask for Access permissions (least privilege; most routes don't use logins); asking for a login with a token that lacks them fails with an error naming the two permissions.
**Why:** Protection must never lag behind exposure, and nothing Teitunnel didn't create is changed. Reading Access only when needed keeps tokens without that permission working.

### D-058 · 2026-09-23 · Connectors per machine and remote logs; no one-click replicas
**Decision:** The Tunnels view groups a tunnel's edge connections by connector (the tunnel list's `client_id`, no extra API call) and marks this Mac's using the `connectorId` from its connector's `/ready`. The Doctor warns (`tunnel.other_connectors`) when another machine runs this Mac's tunnel, unless a local twin explains it. Any connector's logs can be followed through Cloudflare's management relay: `cf_api::LogStream` speaks the `start_streaming`/`logs` protocol over `tokio-tungstenite` (rustls, OS roots); `core::remote_logs` keeps one session per connector with a 2,000-line ring, streams level info and above (request logs at debug would flood a busy tunnel), reconnects after transient drops with a fresh token, ends with a reason on the session limit or missing permission, and stops 30 s after the UI last read it. The management token never leaves Rust and is redacted from error texts. "Run the same tunnel on several machines" is not offered as a button.
**Why:** Teitunnel's routes point at this Mac's localhost, so a replica would receive a share of requests for services it doesn't run: intermittent 502s that look random. Seeing every machine and its logs solves the real problem (a forgotten connector somewhere) without adding that footgun; running elsewhere on purpose is what Export is for.

### D-059 · 2026-09-23 · `teitunnel-cli share`: a connector scoped to the command
**Decision:** `share` is the CLI's one exception to D-056: it runs a Quick Share through the same `QuickShares` as the app, for exactly the command's lifetime (Ctrl-C, SIGTERM, SIGHUP or `--for` stop it). Its connectors are recorded in a per-process registry (`<data>/run-cli/<pid>-<start time>/`); each run first reaps the registries of CLIs that are gone (owner pid and start time no longer match), so a SIGKILLed CLI can't leave a public URL behind. Metrics ports are allocated from a per-process offset (`PortAllocator::spread`) so the app and CLIs starting at once don't hand cloudflared the same port. The URL is the only stdout line; progress goes to stderr. It works without the app set up (in-memory history then). `doctor` reuses `doctor::run` (now generic over `Connectors`) with the probed connectors and applies only `doctor::safe_change` fixes.
**Why:** A share started in a terminal belongs to that terminal; nothing about it outlives the command. Reaping by owner identity is what makes that true even when the terminal is killed.


### D-060 · 2026-09-23 · Private networks: ranges on this Mac's tunnel in the default virtual network; client commands for non-web routes
**Decision:** "Share a private network" routes a CIDR range (a bare address is a `/32`/`/128`; host bits are cleared; ranges broader than /8 or /16 and loopback, link-local and multicast are refused) to this Mac's tunnel through the engine (`Intent::AddNetwork`/`RemoveNetwork`, steps `CreateNetworkRoute`/`DeleteNetworkRoute` with undo), creating the tunnel if needed; the connector is started after. Routes go in the account's default virtual network only; other virtual networks are read (so their routes don't count as conflicts) but not offered. A route belongs to this Mac when it points at this Mac's tunnel, whoever created it (comment `Added by Teitunnel` only labels it); removing a network or the tunnel deletes exactly those, never another tunnel's. The same range on another tunnel is a plan error; overlapping ranges and public ranges are warnings, and a public range needs confirmation (`--replace` in the CLI). Private networks are read only when a change or view needs them (`ObserveNeed::networks`, `Want::IfAllowed` for the overview, tunnel removal and the Doctor, so tokens without the permission keep working). The Doctor reads the WARP device settings only when this Mac shares a network and warns when the Gateway proxy is off (`network.proxy_off`) or the default profile's Split Tunnels exclude the range (Cloudflare's default excludes RFC 1918) or don't include it (`network.excluded`, `network.not_included`); it can't fix them (they're account-wide Zero Trust settings). SSH, RDP, SMB and TCP routes show the `cloudflared access` command visitors run (plus an `~/.ssh/config` entry for SSH) instead of a URL, and aren't checked with an HTTPS probe after applying.
**Why:** One virtual network covers the home-lab and small-team case without asking users to understand namespaces. Split Tunnels are the most common reason a correctly routed network "doesn't work", so surfacing them where the network is listed saves the debugging. A browser probe of an SSH route would always report a failure.

### D-061 · 2026-09-23 · i18n: an in-house typed catalog, the system language, English text from Rust for now
**Decision:** The UI's text lives in JSON catalogs (now `locales/<language>.json` at the repository root, see D-062); English is the source, and `MessageKey` is derived from it, so an unknown key is a type error. `t(key, vars)` (`src/lib/i18n.ts`, no dependency) fills `{name}` placeholders, formats numbers for the language and picks plural forms with `Intl.PluralRules` (`_one`/`_other`, plus `_zero`/`_two`/`_few`/`_many` where a language has them). The language follows the system's (`navigator.languages`, which on macOS includes the per-app language), falling back per message to English; catalogs load before the first render, so module-level label maps hold keys, not text. Lists use `Intl.ListFormat`. A test rejects translations with keys English doesn't have or with different placeholders; `pnpm --filter @teitunnel/desktop i18n:missing <language>` lists what's left to translate. Text produced in Rust was English in this phase; D-062 moved it to the same catalogs.
**Why:** A few hundred strings don't need a framework; a typed 100-line module keeps the bundle budget and catches typos at compile time. JSON is what translation platforms (Weblate, Crowdin) read. Following the system language is what Mac apps do.

### D-062 · 2026-09-23 · Text from Rust as message keys, checked at compile time
**Decision:** Rust never sends the user a sentence. User-facing text is a `teitunnel_core::text::Text { key, args }`, serialized over IPC as `{ key, args }` and translated by the UI with the same `t()` as its own messages (`translate()`; `IpcError` translates its message and hint once, and keeps `key` for logic such as recognising a cancelled sign-in). There is one catalog per language, `locales/<language>.json` at the repository root: the UI's messages at the top level, the Rust core's under `core`. The core's `build.rs` generates a typed constructor per `core` message (`msg::doctor::dns_missing::title(hostname)`; plurals take `count: u64` first), so a misspelled key or missing argument doesn't compile; it also embeds every catalog. Errors implement `UserText` (`text()`), and their `Display` renders the catalog in English, so logs and bug reports stay English and the English isn't written twice. Covered: every core error type (and cf-api/cloudflared errors via the core), `AppError`, plan steps and summaries, apply failures and leftovers, the activity record (new optional `summary`, `error`, `leftovers`, `connector_error`; `Text` also deserializes from a plain string, so stored history loads without a migration), Doctor issues (title, detail, evidence, fix labels, and a display `label` separate from the stable `subject`), verify results, Quick Share and remote-log states, import problems, the OAuth callback page (HTML-escaped), and the native menu bar (including the predefined macOS items, via `*_with_text`), tray and notifications, rendered in Rust in the system language (`sys-locale`; plural rules from `intl_pluralrules`, the Fluent project's CLDR data). Technical values (DNS record contents, services, log lines, Cloudflare's own error messages) pass through as `core.raw` arguments, untranslated. Route health in the tray is an enum (`RouteHealth`), not compared text. Two things stay English on purpose: `teitunnel-cli` (a developer tool whose output scripts parse; its `--json` renders messages to English strings), and markers stored in Cloudflare (the `Added by Teitunnel` route comment identifies ownership).
**Why:** Translating on the side that knows the language keeps Rust free of UI state, and compile-time keys make the catalog impossible to drift from the code. One file per language is what a translator (or Weblate) wants.

### D-063 · 2026-09-23 · Windows and Linux: native title bars, solid surfaces, commands in the window, platform wording
**Decision:** On Windows and Linux the main window is opaque with the system's own title bar and caption buttons (snap layouts on Windows come with it): `tauri.windows.conf.json` and `tauri.linux.conf.json` replace the macOS window (transparent, overlay title bar, vibrancy). Surfaces are solid, from `styles/platform-other.css`: Windows 11's Mica-like neutrals, Segoe UI Variable and its default accent; GNOME's Adwaita tones and Cantarell. The navigation selection is a neutral fill (Windows adds its accent pill), and selections don't grey out when the window is inactive, since both are macOS conventions. There's no menu bar there: `menu::build` runs only on macOS, the webview handles shortcuts (Ctrl instead of ⌘, matched on the physical key; `app/shortcuts.ts`), and the command palette carries the Help links and Quit (`app_open_help`, `app_request_quit`, which asks first like ⌘Q). The tray opens the app on left click on Windows (menu on right click); closing the window quits when the tray icon is hidden, since nothing else would bring it back. Messages that name macOS things (this Mac, Finder, keychain, System Settings, the menu bar, ⌘, Homebrew) have `key@windows` and `key@linux` wordings (this PC / this computer, File Explorer, Credential Manager / keyring, notification area / system tray, Ctrl, winget / the package manager), picked by both the UI and the core; a test fails when a new message has macOS words without them. Real Mica is left out for now: it needs a transparent window, which shows the desktop through it on Windows 10, and it can't be verified without a Windows machine. The token file keeps the per-user profile's ACL (only the user, SYSTEM and Administrators can read `AppData`); an explicit ACL would need Win32 FFI (`unsafe`), which the workspace forbids.
**Why:** Each platform's own chrome and wording is what makes the app feel native there, and falling back to macOS materials on a platform without them produces a see-through window.

### D-064 · 2026-09-23 · A missing Access permission is fixed in place, not reported
**Decision:** When a route needs a login and the account's credential can't manage Access, the route sheet shows `AccessFix` instead of an error: ticking **Require a login** with `accessEdit = no`, or a plan refused with `core.error.observe.accessPermission`, both lead there. For API tokens it walks through editing the existing token in the dashboard (`TokenPage::Edit` opens the token list; Cloudflare keeps the token's value when its permissions change, so nothing is pasted) and re-checks whenever the window regains focus or on **Check Again**. When a re-check finds the permission, the form continues, and a refused review runs again. Only explicit re-checks report readiness, so a refusal that persists can't loop. Credentials that can't gain the permission (cloudflared's cert.pem, OAuth until its Access scopes are registered), or users who prefer a new token, get **Create Token** (`TokenPage::CreateWithLogins`: the usual template plus `access` and `access_acct` edit, keys from Cloudflare's documented table) and a paste field that replaces the account's credential through `accounts_add_token`. The default connect template stayed without Access here (least privilege, D-057); D-065 changed that. `accessEdit` now needs both Access permissions: the apps write probe and a read of the login methods (`/access/identity_providers`; its update endpoint is PUT-only, so a PATCH probe would read 405 as allowed).
**Why:** A permission error with nowhere to go makes users disconnect and start over. Editing the token they already have, and noticing when they come back, turns it into one trip to the browser, without asking every user for a broader token up front.

### D-065 · 2026-09-23 · The token template asks for Access up front
**Decision:** The pre-filled "Create API token" link includes `access` and `access_acct` (edit) next to Tunnel, DNS, Zone and Account Settings, so a new token can require logins without a second trip to the dashboard. Supersedes the least-privilege default in D-057/D-064. The fix-in-place card (D-064) stays for older or hand-made tokens and for cert.pem/OAuth accounts. Ownership rules don't change: Teitunnel edits only Access applications it created.
**Why:** The maintainer chose ease over the narrower grant: logins are a first-class route option, and a permission error on first use is the worse experience. The risk (a leaked token could change other Access apps) is the same class as DNS · Edit on every zone, which the token already has.

### D-066 · 2026-09-23 · No dead ends: every core error is fixed in place, linked, or explained
**Decision:** `PermissionFix` (generalised from D-064's Access card) lists what a feature needs that the credential lacks (`PermissionNeed`: tunnels, zones, DNS per domain or on any domain, Access), in the dashboard's own names, with why, and offers "edit the existing token" (nothing to paste) or "paste a new one". It re-checks on window focus and calls back so the feature resumes. It appears in the route sheet (a gap found by the capability check disables Review; a refusal, whether Access or Cloudflare's generic permission error, shows the change's needs), the Doctor's `auth.missing_scope`, private networks, connector logs, and under each account's and domain's permission list (whose per-row hints it replaces). `ZeroTrustFix` opens Zero Trust onboarding and retries on return. Every `core.error.*` message is classified in `lib/error-help.ts` as `fix`, `link` (a button to the Cloudflare page that resolves it: add a domain, Access applications), `input` or `retry`; a test fails when a new error isn't classified.
**Why:** A user who hits a wall has to know where to go and has to be brought back to where they were; making the classification exhaustive keeps new errors from regressing to plain text.

### D-067 · 2026-09-23 · Several tunnels per machine, one planner per tunnel
**Decision:** A machine can have more tunnels besides its default (machine) tunnel (M10-02, Cloudify parity: environments). Local state is one row per tunnel (`local_tunnels`, migration 8, `is_default`; the old per-account row becomes the default), and everything keyed by account that was really about the tunnel is now keyed by tunnel id (applied version and ingress, metrics port, run mode). The engine stays single-tunnel: `Context.tunnel` picks which of this Mac's tunnels a change is about (`None`: the default; an unknown id is `ObserveError::UnknownTunnel`), so planning, rollback and the staleness guard are unchanged. A new tunnel is `Intent::CreateTunnel { name }` (plan → apply, name 1–64 characters and unique in the account, `TunnelNameTaken` otherwise); a created tunnel is recorded as the default, an additional one, or a replacement of the row it recreates (`Slot`). A hostname is routed once per machine: the snapshot carries the routes of the other local tunnels (`elsewhere`, from the local store), and the planner refuses a duplicate (`RoutedElsewhere`). The overview merges every local tunnel (routes carry `tunnel_id`; health comes from the carrying tunnel's connector); verify and route logs find the carrying tunnel themselves; drift is checked on every tunnel; connectors resume, update, quit-to-Always-on and sign-out per tunnel; the Doctor gathers and diagnoses each tunnel separately, deduplicates account-wide findings, and fixes on the issue's tunnel (`Issue.tunnel_id`); logins on another tunnel's routes aren't orphans. Private networks stay on the default tunnel. CLI: `tunnels`, `tunnel create|delete`, `--tunnel` on changes and export.
**Why:** Keeping the planner per tunnel makes the feature additive: every guarantee (plan → apply, ownership, undo) holds per tunnel without a multi-tunnel planner, and one-hostname-per-machine keeps DNS unambiguous.
