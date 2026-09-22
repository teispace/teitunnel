# Teitunnel — Rewrite Plan

Status: proposal · 2026-09-22 · replaces the prototype on `main` (to be archived as tag `legacy-prototype`)

---

## 0. The one-paragraph pitch

Teitunnel lets anyone put a local app on their own domain in under a minute, and keeps it that way.
You say *"xyz.com → localhost:3000, yx.com → localhost:5000"*. Teitunnel works out the tunnel, config, DNS records, process, health checks and cleanup, shows you exactly what it will change before it changes it, and fixes or explains anything that drifts afterwards. Beginners never see the word "ingress". Experts get every `cloudflared` knob, plus the equivalent CLI command for every action.

---

## 1. What went wrong in the prototype (so we don't repeat it)

| Problem | Consequence | Rule for the rewrite |
|---|---|---|
| API tokens returned to the frontend and kept in Zustand | Secrets live in the webview heap | **Secrets never cross IPC.** Rust resolves credentials from the active account id. |
| Two auth systems (token + cert.pem) mixed into one store | Tangled branching everywhere | One `Account` abstraction, several `CredentialSource`s behind it. |
| Two tunnel kinds (API vs cert/local) with parallel commands | Duplicated flows and UI | One `Tunnel` model with a `ConfigSource` field (Remote / LocalFile). |
| Tunnel-centric UI | Users think in *hostnames*, not tunnels | **Route-centric UI.** Tunnels are an implementation detail with an Advanced view. |
| 700–800-line view files, 3 shared components | Unmaintainable, inconsistent | Feature folders plus a real design system, with a hard ~250-line guideline per file. |
| Hand-written IPC types in `tauri.ts` | TS/Rust types drift silently | Generated bindings (`tauri-specta`), with a CI check that fails on drift. |
| Imperative "call API, then call DNS, then hope" | Half-applied changes, orphaned records | **Plan → Apply engine** with previews, ownership tracking and rollback. |
| URL/status scraped from log text | Fragile across cloudflared versions | Use cloudflared's local `/quicktunnel`, `/ready` and `/metrics` endpoints and `--output json` logs. |
| Tunnel token passed as a CLI argument | Visible in `ps` | Pass via `TUNNEL_TOKEN` env or `--token-file` (0600). |
| General "run any command" terminal | Security hole, little value | Removed. Replaced by a structured log viewer and "Copy as cloudflared command". |
| Web-app look (hover hand cursors, glow, gradients) | Feels like a website in a box | Native conventions per OS (section 7). |

---

## 2. Product model: the nouns users see

```
Account ──< Zone (domain) ──< Route >── Tunnel ──< Connector (a running cloudflared)
                               │
                               └── Origin (local service: port / process / docker container / unix socket)
QuickShare (no account): Origin → random *.trycloudflare.com
```

- **Route** is the main object: `https://app.xyz.com/api/* → http://localhost:3000`. Everything in the UI revolves around routes.
- **Tunnel**: by default one per machine, auto-created and named after it (e.g. "Krishna's MacBook Pro"). Power users can create more, for example to isolate projects or add a replica on a server.
- **Multi-domain on one machine** is native. A single tunnel and a single cloudflared process serve any number of hostnames across any number of zones in the account. `xyz.com → :3000` and `yx.com → :5000` are two routes on the same tunnel, with two CNAMEs in two zones.
- **Origin types**: HTTP(S), TCP, SSH, RDP, SMB, unix socket, `hello_world`, `http_status:NNN`. Per-route origin options cover everything cloudflared supports: `noTLSVerify`, `originServerName`, `httpHostHeader`, timeouts, `disableChunkedEncoding`, `http2Origin`, `proxyType`, `access` and so on.

---

## 3. Architecture

### 3.1 Core idea: declarative reconciliation

Every mutation follows one pipeline:

```
 Desired state            Observed state
 (what the user wants)    (Cloudflare API + local processes + DNS + local ports)
          \                 /
           ▼               ▼
            Planner (pure fn, heavily tested)
                   │
                   ▼
            Plan  = [Step]    ← shown to user as a readable preview
                   │             "Create CNAME app.xyz.com → <uuid>.cfargotunnel.com"
                   ▼
            Executor  → applies steps in order, records each in the Activity log
                   │     on failure: runs compensating steps (rollback)
                   ▼
            Verifier  → probes the public URL end to end, reports result
```

The same engine powers **every** "smart" feature:
- **Add, edit or remove a route**: diff the desired route set against reality.
- **Drift detection**: someone edited the tunnel in the dashboard, so the observed state no longer matches the desired state. Teitunnel shows the diff and offers "Keep theirs" or "Restore mine".
- **Cleanup ("no mess")**: DNS records Teitunnel owns that no route needs anymore become "delete" steps.
- **Doctor**: checks are observers over the same state, and fixes are plans.
- **Import**: observed resources (existing tunnels, config.yml, DNS) become desired state.

### 3.2 Ownership tracking (safe cleanup, zero orphans)

- Every DNS record Teitunnel creates gets the comment `teitunnel:route=<route-id>` (DNS comments work on all plans).
- Local SQLite stores `route ↔ dns_record_id ↔ tunnel_id`.
- **Rule:** Teitunnel only auto-deletes records it owns. Foreign records are only *reported*, e.g. "points at a tunnel that no longer exists". Deleting them needs explicit confirmation.
- Because the ownership marker lives on Cloudflare, a fresh install on a new machine can rebuild its state.

### 3.3 Process runtime

- **Supervisor**: one Tokio actor per connector. Explicit state machine:
  `Stopped → Starting → Connecting → Healthy ⇄ Degraded → Stopping → Stopped`, plus `Crashed(backoff)`.
- Each connector gets a **stable metrics port** from the DB (range 20300+, avoiding cloudflared's default 20241–20245).
  - `/ready` gives health (ready connection count), polled every 2 s.
  - `/quicktunnel` gives the QuickShare hostname, with no log parsing.
  - `/metrics` gives telemetry (Prometheus text, parsed in Rust).
  - `/diag/*` feeds the diagnostics bundle.
- Logs come from `--output json` (cloudflared ≥ 2025.6.1) and are parsed into typed `LogEvent`s, with a text parser as fallback for older binaries.
- Restart policy: exponential backoff with jitter (1s → 60s cap), and "crash loop" detection that raises a Doctor issue.
- Shutdown: SIGTERM, then SIGKILL after 5 s (on Windows, `CTRL_BREAK`, then terminate).
- **Two run modes per tunnel**:
  1. **Session**: supervised by the app. Tunnels keep running while the app is in the tray and stop when the app quits.
  2. **Always-on**: installed as a native user service (macOS LaunchAgent plist, systemd `--user` unit, Windows scheduled task/service). It survives app quit and reboot. The app *observes* it through the fixed metrics port and never needs to own the process.
  This avoids writing our own daemon while still being production grade.
- **Adoption**: on startup, scan processes (`sysinfo`) for cloudflared instances Teitunnel didn't start. Probe their metrics ports and offer "Import / Adopt / Ignore".

### 3.4 Auth & accounts

Priority order in onboarding:

1. **Sign in with Cloudflare (OAuth 2.0 + PKCE)**. Cloudflare opened self-managed OAuth to all customers in June 2026, and public clients use PKCE. Needs a public OAuth client registered under a verified teispace domain. *To verify:* whether loopback redirects (`http://127.0.0.1:<random>/callback`) are allowed; the fallback is the `teitunnel://` deep link.
2. **API token**: a "Create token" button opens a **pre-filled template URL** (`dash.cloudflare.com/profile/api-tokens?permissionGroupKeys=…&name=Teitunnel`) with exactly the needed scopes. Paste, verify, done. The app also shows which features each missing permission disables.
3. **Import `cert.pem`** from `cloudflared tunnel login`. This is scoped to one zone, so it's treated as a limited account and the UI says so.

- Multiple accounts are supported, each with its own credential in the OS keychain under the key `teitunnel/<account-id>`.
- Permission introspection: the token-verify endpoint plus probe calls produce a capability map (`can_edit_tunnels`, `can_edit_dns(zone)`, `can_edit_access`…). The UI disables what can't work, with a one-line reason.

### 3.5 Discovery (the "it already knows" feeling)

- **Local services**: listening TCP ports along with the owning process (`listeners` crate). Examples: "node · vite · :5173", "postgres · :5432". Known dev servers get labels (Next, Vite, Rails, Django, Laravel, Astro…).
- **Docker**: containers with published ports (`bollard`). Route suggestions use container names.
- **Existing cloudflared setup**: `~/.cloudflared/`, `/etc/cloudflared/` (config.yml, credentials JSON, cert.pem), running processes, installed services. Everything can be imported, and there's an optional guided migration from a locally-managed to a remotely-managed tunnel.
- **Zones**: which domains are active on Cloudflare, which are pending nameserver changes, and existing records that would conflict with a new hostname.

### 3.6 Doctor (issue detection, each with a Fix or an explanation)

| Check | Fix |
|---|---|
| cloudflared missing / outdated / unsupported | Install or update the managed binary (SHA256 verified) |
| Origin not listening (`localhost:3000` refused) | Show which process last used it; "Start watching" |
| Origin returns 5xx / TLS error to self-signed origin | Suggest `noTLSVerify` / `originServerName` |
| Hostname DNS missing / points to other tunnel / not proxied | Plan to create or repair the CNAME |
| Conflicting A/AAAA/CNAME on the hostname | Show the record, offer replace (explicit confirm) |
| Zone pending (nameservers not switched) | Show the required NS values |
| Tunnel has 0 connections / degraded / crash loop | Show the last error lines, restart |
| QUIC/UDP 7844 blocked | Switch the connector to `--protocol http2` |
| Clock skew | Explain |
| Drift: remote config ≠ desired | Show diff, keep theirs or restore |
| Orphaned CNAMEs to `*.cfargotunnel.com` | Cleanup plan (owned: auto; foreign: confirm) |
| Stale tunnels (created by Teitunnel, no routes, idle N days) | Delete plan |
| Duplicate connectors for one tunnel on the same host | Stop extras |
| Token lacks a scope a feature needs | Deep link to regenerate the token |
| End-to-end probe of public URL fails | Staged result: DNS, edge, tunnel, origin; shows the stage that failed |

"Export diagnostics" produces a single redacted zip (app logs, `cloudflared tunnel diag`, doctor report) for bug reports.

### 3.7 Frontend ↔ backend contract

- **Commands** are generated with `tauri-specta`, which produces `bindings.ts` with every command, argument and return type. CI fails if the bindings are out of date.
- **Errors**: one serializable `AppError { code, message, hint?, fix?: PlanRef, field? }`. Cloudflare API error codes map to human messages (e.g. `81053` becomes "A record with this name already exists").
- **Server state** lives in TanStack Query. The backend emits `changed { entity, id }` events on a single bus, and the frontend invalidates the matching query keys. The UI stays live without hand-written sync code.
- **High-frequency streams** (logs, metrics) use Tauri `Channel`s per subscription, batched every 100 ms, and are unsubscribed on unmount. There is no global `emit` firehose.
- **UI state only** lives in Zustand (selection, palette, panes). No Cloudflare data sits in Zustand.
- **Validation**: shape rules (hostname syntax, port range) live in one `zod` module for instant feedback. Semantic rules (zone ownership, conflicts, permissions) come from a Rust `validate_*` command, debounced. Rust re-validates on apply. It is the authority.

---

## 4. Repository layout

A Cargo + pnpm workspace. Core logic is Tauri-free, so it's testable in isolation and reusable for a future CLI.

```
teitunnel/
├── Cargo.toml                    # [workspace]
├── pnpm-workspace.yaml
├── crates/
│   ├── cf-api/                   # Typed Cloudflare API v4 client. No Tauri.
│   │   └── src/{client,auth,accounts,zones,dns,tunnels,tunnel_config,access,management,error}.rs
│   ├── cloudflared/              # Everything about the binary. No Tauri.
│   │   └── src/{locate,install,version,command,config_yaml,credentials,log_parse,metrics_parse,endpoints}.rs
│   └── core/                     # Domain + engine. No Tauri.
│       └── src/
│           ├── domain/           # Account, Zone, Tunnel, Route, Origin, Ownership, ids
│           ├── plan/             # desired.rs observed.rs diff.rs plan.rs executor.rs rollback.rs verify.rs
│           ├── runtime/          # supervisor.rs connector.rs health.rs streams.rs
│           │   └── service/      # launchd.rs systemd.rs windows.rs
│           ├── discovery/        # ports.rs processes.rs docker.rs cloudflared_import.rs
│           ├── doctor/           # registry.rs + checks/*.rs (one file per check)
│           ├── store/            # sqlite (rusqlite, bundled) + migrations/
│           ├── secrets/          # keyring-backed SecretStore trait (+ in-memory for tests)
│           ├── events.rs         # typed broadcast bus
│           └── error.rs
├── apps/
│   └── desktop/
│       ├── src-tauri/            # Thin shell: wiring only, no business logic
│       │   └── src/
│       │       ├── main.rs  lib.rs  state.rs
│       │       ├── ipc/          # commands/*.rs (1 file per feature), events.rs, bindings export
│       │       └── shell/        # tray.rs menu.rs window.rs deep_link.rs updater.rs notifications.rs
│       └── src/
│           ├── app/              # providers, router, query client, theme, shortcuts, platform detection
│           ├── routes/           # TanStack Router file routes (thin: compose features)
│           ├── features/
│           │   ├── onboarding/   ├── routes/      ├── quick-share/  ├── tunnels/
│           │   ├── domains/      ├── doctor/      ├── activity/     ├── services/
│           │   ├── accounts/     └── settings/
│           │   #   each: components/ hooks/ queries.ts schemas.ts index.ts
│           ├── components/
│           │   ├── ui/           # shadcn primitives, restyled to our tokens
│           │   └── patterns/     # Inspector, SplitView, ListRow, EmptyState, StatusDot,
│           │                     # PlanPreview, CopyField, KeyValue, HostnameInput, OriginPicker
│           ├── lib/ipc/          # bindings.ts (generated), client.ts, query-keys.ts, events.ts
│           ├── lib/              # format.ts validation.ts platform.ts
│           └── styles/           # tokens.css (primitives → semantic), platform.css, globals.css
├── fixtures/                     # recorded CF API responses, cloudflared outputs, metrics samples
├── tools/fake-cloudflared/       # tiny test binary emulating cloudflared (endpoints + json logs)
├── docs/                         # ARCHITECTURE, SECURITY, CONTRIBUTING, design/
└── .github/workflows/            # ci.yml, release.yml, bindings-check, e2e
```

Later: `apps/cli` (a `teitunnel` CLI over `core`) and `apps/site` (docs and landing page).

---

## 5. Tech choices (and why)

| Area | Choice | Why |
|---|---|---|
| Shell | Tauri v2 | Small, native webview, Rust core |
| Tauri plugins | single-instance, deep-link, updater, autostart, notification, window-state, opener, os, log, global-shortcut, positioner | Native behaviors from official plugins |
| IPC typing | tauri-specta | Single source of types |
| HTTP | reqwest + rustls | No OpenSSL pain cross-platform |
| Storage | rusqlite (bundled) + migrations | Metadata, ownership, activity, metric rollups |
| Secrets | keyring v3 | OS keychain / Credential Manager / Secret Service |
| Discovery | listeners, sysinfo, bollard | Ports+PIDs, processes, Docker |
| Logging (app) | tracing + rolling file | Diagnostics bundle |
| Router | TanStack Router | Type-safe routes, search params for filters |
| Server state | TanStack Query | Cache, invalidation, optimistic updates |
| UI state | Zustand | Tiny, UI-only |
| Components | shadcn/ui on Radix, fully re-tokened | Accessible primitives we own |
| Forms | react-hook-form + zod | Fast, typed |
| Charts | **uPlot** (not Recharts) | ~45KB, canvas, handles 1 Hz realtime without jank |
| Lists | TanStack Virtual | 100k-line log viewer |
| Palette | cmdk | ⌘K everything |
| Lint/format | Biome (TS), rustfmt + clippy `-D warnings`, cargo-deny | Fast, strict |
| Tests | cargo-nextest, insta (snapshots), proptest, wiremock; Vitest + Testing Library + `mockIPC`; Playwright (UI against mocked IPC, visual regression); WebdriverIO + tauri-driver on Linux/Windows | See §9 |
| Release | release-please, GitHub Actions matrix, macOS notarization, Windows signing (SignPath OSS / Azure Trusted Signing), Tauri updater (signed) | Market-ready distribution |
| Channels | GitHub Releases, Homebrew cask, winget, Scoop, AUR, Flatpak/AppImage/deb/rpm | Reach |

Dropped from the prototype: xterm.js (terminal removed), Recharts, `qrcode.react` (kept only if we don't render QR in Rust — decide in M1).

---

## 6. Screens & flows

**Information architecture (sidebar):**
Overview · Routes · Quick Share · Domains · Tunnels (Advanced) · Activity · Doctor · Settings.
Account switcher at the top of the sidebar. A global status pill is always visible: "3 routes healthy" / "1 issue".

**Layout pattern:** Sidebar | List | Inspector (like Mail/Finder/Xcode). Select a row, and details, logs, metrics and actions open in the inspector. No modals for primary work. Modals are only for destructive confirms and plan previews.

**Key flows:**
1. **First run (≤ 60 s to a public URL):** detect or install cloudflared → "Share something now" (QuickShare, no account) *or* "Connect Cloudflare" (OAuth / token / import) → pick a detected local service → choose hostname (subdomain + domain dropdown, live conflict check) → **Plan preview** → Apply → live progress checklist → verified URL with Copy / Open / QR.
2. **Add route (⌘N):** Origin picker (detected services first, or type `:3000`, a URL, a socket) → hostname → optional path, Access protection, and advanced origin options folded away → preview → apply.
3. **Remove route:** preview shows "remove ingress rule, delete CNAME (owned), tunnel now empty → stop / keep / delete?"
4. **QuickShare:** one field (port or detected service) → URL in ~3 s → copy/QR, live request counter, auto-stop timer option.
5. **Doctor:** grouped issues, "Fix all safe issues" (only fixes on owned resources run automatically), per-issue explanation.
6. **Activity:** every applied step with timestamp, actor, before/after, and "Copy as cloudflared/API command".
7. **Menu bar / tray:** routes with health dots and toggles, QuickShare start, open app, pause all.

**Beyond v1 (designed for, not built first):** Access protection toggle ("Require login: emails / domain") via Access apps + policies · private networks and WARP routes (CIDR, virtual networks) · remote connector logs via the Management API (`cloudflared tail`) for tunnels on other machines · replicas · config export (YAML / Terraform) · CLI · i18n.

---

## 7. Native design system

**Principles:** quiet, dense, fast, predictable. It should look like it shipped with the OS. Reference bar: macOS System Settings, Xcode, Tower, Proxyman, TablePlus, Tailscale, Linear's desktop app.

**Hard rules (what makes UI "look AI-generated", and what we do instead):**
- No gradients, glows, glassy cards on cards, emoji, "✨", oversized rounded hero sections, or decorative icons in every heading.
- No pointer (hand) cursor on buttons. Native apps use the arrow; the hand is only for real links.
- Text is not selectable by default, except values (URLs, IDs, logs), which are selectable and copyable.
- Base font 13px (macOS) / 14px (Windows), system fonts only: `-apple-system`/SF Pro, Segoe UI Variable, Cantarell/Inter fallback on Linux. SF Mono / Cascadia Mono for values.
- One accent color: follows the **OS accent** where possible. Cloudflare orange appears only in the brand mark.
- Semantic status colors only (healthy, degraded, down, idle), always paired with a shape or label so they don't depend on color.
- Borders 1px hairlines, radius 6–8px, shadows only on popovers and menus.
- Motion 120–180 ms ease-out, only for state changes. Respects *Reduce motion* and *Reduce transparency*.
- Density: 28px list rows, 32px toolbar controls, 8px spacing grid.
- Keyboard first: ⌘K palette, ⌘N new route, ⌘R refresh, ⌘1–8 sections, ⌘, settings, arrow-key list navigation, ⌫ to remove with confirm.
- Uses the **native menu bar** (File/Edit/View/Window/Help via the Tauri Menu API), native context menus (right-click on rows), native notifications and native file dialogs. No HTML imitations of these.

**Per-OS shell:**
- **macOS:** `titleBarStyle: Overlay`, hidden title, inset traffic lights, sidebar with `NSVisualEffectView` vibrancy (`windowEffects: ["sidebar"]`), content pane opaque, menu bar extra with a template (monochrome) icon.
- **Windows 11:** Mica backdrop, Segoe UI Variable, custom caption area with snap-layout support, tray icon.
- **Linux:** native GTK decorations, solid surfaces (no fake blur), tray via libappindicator where available.

**Tokens:** `primitives` (neutral scale, accent, status) → `semantic` (`--surface-window`, `--surface-sidebar`, `--surface-raised`, `--text-primary/secondary/tertiary`, `--border-subtle`, `--focus-ring`, `--status-*`) → component tokens. Light and dark are both first class, following the system setting with an override.

**Deliverable before coding screens:** a design-system page (Storybook/Ladle) with every primitive and pattern in light/dark × macOS/Windows, reviewed for look and feel first.

---

## 8. Performance budgets (enforced in CI where measurable)

| Metric | Budget |
|---|---|
| Cold start → interactive | < 500 ms (macOS M1), no splash |
| Idle memory (app, excl. cloudflared) | < 120 MB |
| Initial JS (gzip) | < 250 KB; routes lazy-loaded |
| Log viewer | 2,000 lines/s sustained at 60 fps, 100k lines retained (ring buffer in Rust) |
| Metrics | 1 Hz per connector in memory (1 h); 1-min rollups persisted 7 days |
| IPC | No command > 50 ms on the UI path. Anything slower is async with progress events. |
| Installer size | < 15 MB (cloudflared downloaded on demand, not bundled) |

---

## 9. Quality gates

- **Rust unit:** planner/diff (snapshot tests with `insta` for every plan shape; `proptest` for "apply(plan(desired, observed)) == desired"), config YAML round-trip, log/metrics parsers against fixture corpora from several cloudflared versions.
- **Rust integration:** `cf-api` against `wiremock` with recorded real responses, including error codes and pagination. Supervisor against `tools/fake-cloudflared` covering crash, slow start, degraded, SIGTERM ignored, and so on.
- **Frontend:** Vitest + Testing Library with `mockIPC`. Playwright runs the UI against mocked IPC for flows and visual regression (light/dark).
- **E2E:** WebdriverIO + tauri-driver (Linux/Windows CI), plus a nightly smoke test against a real Cloudflare test account (QuickShare + one route lifecycle + cleanup verification: zero leftover records).
- **CI on every PR:** fmt, clippy, Biome, typecheck, tests, bindings drift, cargo-deny, bundle-size check, build on macOS/Windows/Linux.
- **Security:** CSP locked down, Tauri capabilities least-privilege per window, no shell plugin, tokens only in the keychain and never logged (redaction layer in `tracing`), `TUNNEL_TOKEN` via env/token-file, SHA256 verification of downloaded binaries, signed updates.

---

## 10. Milestones

Each milestone ends with something shippable and demoable.

| # | Name | Scope | Exit criteria |
|---|---|---|---|
| M0 | Foundations | Archive prototype (tag), new workspace layout, CI, specta pipeline, design tokens + primitives, native window shell (all 3 OS), menu bar, tray stub | Empty app looks native on macOS/Windows/Linux; CI green |
| M1 | Binary + QuickShare | cloudflared locate/install/update (checksummed), supervisor v1, `/quicktunnel`, JSON logs, QuickShare UI + QR + tray | **v0.1 public release.** Share localhost in 1 click |
| M2 | Accounts & Domains | OAuth (if client approved) + token template + cert import, multi-account, capability map, zones view | Connect in < 30 s; permissions explained |
| M3 | Routes engine | Planner/executor/rollback, ownership comments, auto-tunnel per machine, multi-domain routes, plan preview, E2E verify | **v0.3.** `xyz.com→:3000` + `yx.com→:5000` from zero in < 60 s; delete leaves zero records |
| M4 | Discovery + Doctor | Ports/processes/Docker discovery, import existing cloudflared setups, all §3.6 checks with fixes, cleanup | Every check has a fix or explanation; imports round-trip |
| M5 | Observability + Always-on | Metrics (uPlot), log viewer, activity log, LaunchAgent/systemd/Windows service mode, notifications | Tunnels survive app quit/reboot; 1 h of metrics without jank |
| M6 | Distribution | Signing, notarization, updater, brew/winget/scoop/AUR/flatpak, docs site, CONTRIBUTING | **v1.0.** Installs from brew/winget; auto-updates |
| v1.x | Advanced | Access protection, private networks/WARP routes, remote connector logs, replicas, YAML/Terraform export, CLI, i18n | — |

---

## 11. Decisions needed from the maintainer

1. **Platform priority**: build macOS-first with Windows/Linux kept green in CI, or all three equal from M0? *(Recommendation: macOS-first polish, all three must build and pass CI.)*
2. **OAuth**: register a public Cloudflare OAuth client under a verified teispace domain? *(Recommendation: yes. It's the biggest UX win. The token flow ships first as a fallback.)*
3. **Minimum cloudflared version**: require ≥ 2025.6.1 for JSON logs, and ship the text-parser fallback only if needed? *(Recommendation: require it, since the managed binary makes this painless.)*
4. **Always-on service mode in v1**: yes (M5) or defer? *(Recommendation: yes. It's what makes this production grade for servers and home labs.)*
5. **Repo reset**: archive current `main` as tag `legacy-prototype` and start the new layout on a `rewrite` branch, then merge. Nothing gets deleted without your go-ahead.
