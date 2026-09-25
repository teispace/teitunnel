# M12: See every request, let agents drive, measure everything

**Goal:** Teitunnel becomes the most capable way to put local work on the internet: everything ngrok, LocalCan, Pinggy and FlareDeck do, on the user's own Cloudflare account, free, native, with no caps. Evidence and sources: [research/competitors-2026.md](../research/competitors-2026.md).
**Principles:** simple first (one click, sensible defaults, advanced behind disclosure); native (DESIGN.md); local-first and private (captured traffic never leaves the machine unless exported); every Cloudflare change through plan → apply; agents get the same power as people, behind the same previews and an approval the person controls.
**Status:** Decided 2026-09-24 (answers at the end). In progress.

## Order of work

0. **Ship 0.2.0 before 2026-10-05** (Cloudflare API change, D-094/D-095). No new features before it.
1. **M12-01 Dev servers just work** (small, removes the top failure).
2. **M12-02 Inspector** (the biggest gap; the foundation for replay, webhooks, protection, analytics and agent tools).
3. **M12-03 Agents (MCP)** on top of the inspector and the existing engine.
4. **M12-04 Protection** (password, secret link, IP rules) using the inspector's proxy.
5. **M12-05 Analytics, uptime and alerts.**
6. **M12-06 Sharing power-ups** (pause/resume, stable and branch names, folders).
7. **M12-07 Everywhere** (tray, deep links, Raycast, VS Code, browser).
8. **M12-08 Cloudflare depth** (private hostnames, webhook bypass paths, scoped tokens, batch routes).
9. **M12-09 Robustness from others' bug trackers.**
10. **M12-10 Reach** (listings, comparisons, launch).
11. **M12-11 CI/CD and teams** (GitHub Action with PR previews, name reservations).
12. **M12-12 More** (webhook inbox, project file, exposure check, idle stop, OpenAPI from traffic, move to a new computer).

Each item ships behind tests (unit, E2E against fake-cloudflare/fake-cloudflared, UI tests), docs and a DECISIONS entry, like every milestone so far.

## Architecture: the local inspector proxy ("Lens")

Traffic today: visitor → Cloudflare edge → cloudflared → origin (`localhost:3000`). With inspection on:

visitor → edge → cloudflared → **Lens (127.0.0.1:random, in the Teitunnel process)** → origin

- Rust (`hyper` + `tokio`), new crate `crates/lens`: streaming reverse proxy that never buffers (SSE, chunked, WebSocket upgrades and gRPC pass through untouched, answering cloudflared #199/#883), records each exchange: timing (proxy received, origin first byte, done), method, URL, status, headers, bodies up to a cap (default 1 MB each, truncated beyond, never held in full for downloads), WebSocket/SSE message summaries.
- Storage: a ring per share/route in memory (default 1,000 exchanges), optionally persisted to SQLite for 24 h so a restart keeps recent history; clear on demand. Captures never leave the machine except by explicit export/copy.
- Secrets: `Authorization`, `Cookie`, `Set-Cookie`, `Proxy-Authorization`, webhook signature headers and known token patterns are masked in the UI by default (click to reveal) and redacted in exports unless the user unticks it.
- **Quick Shares:** cloudflared's `--url` points at Lens instead of the origin; on by default (decision Q1).
- **Routes (remote-managed ingress in Cloudflare):** "Inspect this route" is a temporary change through plan → apply that points the rule's service at Lens and remembers the original (like temporary shares, D-068): reverted when turned off, when the app quits, and at launch after a crash. Always-on connectors keep their direct origin unless inspection is on, and then only while the app runs (decision Q2).
- **Without Lens** (any connector, including Always-on and other machines): a lightweight live request feed from cloudflared's own log stream (method, path, status, duration from `--loglevel debug`/management logs): no bodies, zero setup.
- CLI: `teitunnel share --inspect` and `teitunnel inspect <route>` run Lens in the CLI process; `teitunnel traffic ls|get|watch|replay|export` read it (LocalCan parity).

## M12-01 · Dev servers just work
- [x] When a share or route goes live, the verifier fetches it once; a Vite "Blocked request. This host is not allowed", webpack "Invalid Host header", Next.js/Angular host checks or Rails `HostAuthorization` answer is recognised.
- [x] Fix in place: "Your dev server rejects this address. [Send Host: localhost]" (sets `httpHostHeader` for that share/route, re-checks) or "[Show the one-line config]" with the exact `allowedHosts` line for the detected framework and version.
- [x] Discovery marks known dev servers; new shares of them get the host header by default when safe (not for Next.js server actions or OAuth callbacks, where it breaks origin checks), with an explanation.
- [x] SSE on a Quick Share (Cloudflare doesn't carry it): detect `text/event-stream` and suggest "Share on my domain" (MCP servers on SSE transport, streaming UIs).
- [x] Explain 413 (100 MB body limit on Free/Pro), 429 (Quick Share 200 in-flight), 502/1033 inline with the fix.

## M12-02 · Inspector
- [x] `crates/lens`: streaming proxy, capture ring, masking, timing; property tests for header/body handling; fuzzed request parsing; benchmarks (added latency under 1 ms p99 locally, no buffering for streams). (D-100; the <1 ms p99 target still needs a bench on an idle machine.)
- [x] Quick Share and route integration (above), CLI integration. (D-110)
- [x] UI, per share/route and one "All traffic" view: live list (method, path, status, duration, size, time), filters (status class, method, path/host text, duration), full-text search in bodies, detail pane (request/response headers, body: pretty JSON, form, multipart parts, image preview, hex for binary), timing bar. (D-111; the 60 fps at 10,000 rows budget is by design, not yet measured by `perf`.)
- [x] Replay to the origin; edit and replay (method, path, headers, body); replay N times; compare two exchanges (diff). (backend and CLI; UI with the inspector screens.)
- [x] Export: cURL, HTTPie, fetch, raw HTTP, HAR, JSON, Markdown (for issues and agents); redacted by default. (backend and CLI.)
- [x] Webhooks: recognise Stripe, GitHub, Slack, Shopify, Clerk, Twilio, Linear, Discord and standard-webhooks signatures; verify with a secret kept in the keychain; show "signature valid / invalid / expired timestamp"; replay keeps or recomputes the signature (user's choice). (backend and CLI.)
- [x] Mock/stub: answer a path with a saved response when the origin is down (keeps webhook senders happy while you restart).
- [x] Breakpoints: hold matching requests or answers, edit, answer from here, drop, continue (D-130; 60 s limit, 50 at once, bodies up to 1 MB of known-size text).
- [ ] Performance budget: 60 fps list with 10,000 exchanges (virtualised, like the log viewer, D-051).
- [x] Network simulation per tap: latency and jitter presets (3G, 4G, satellite), bandwidth limits; fault injection per path (a share of 500/504/429 answers, dropped connections, slow first byte). HTTP-level: connection resets and timeouts stand in for packet loss.
- [x] Stream keep-alive: during idle periods Lens writes SSE comment lines into `text/event-stream` responses so Cloudflare's 100-second idle timeout (524 on Free/Pro) never cuts a long AI tool call.
- [ ] WebSocket frame viewer: frames in both directions with direction, time, size, text/binary preview (bounded).

## M12-03 · Agents (MCP) and AI
- [x] `teitunnel mcp`: MCP server over stdio (Claude Code, Cursor, VS Code, Codex, Windsurf, Zed), and Streamable HTTP from `teitunnel serve` with API keys for remote agents.
- [x] Tools: `share_port`, `stop_share`, `list_shares`, `list_routes`, `plan_change` (returns the same plan people review), `apply_plan` (by fingerprint), `verify_route`, `doctor`, `fix_issue`, `logs_tail`, `traffic_list`, `traffic_get`, `traffic_replay`, `wait_for_request` (block until a matching request arrives: webhook testing without polling), `traffic_stats`, `export_config`.
- [x] Resources (routes, shares, domains, issues) and prompts ("debug this failing webhook", "put my dev server online on my domain with a login").
- [x] Safety: three modes per client (read-only, ask, full). "Ask" shows a native approval in the app (or the terminal) with the plan before any Cloudflare change; secrets never exposed to agents (masked bodies unless allowed); every agent action in Activity, marked with the client's name; rate limits. (Done in `crates/mcp`: modes, elicitation or a confirmed second call, redaction, Activity `actor`, rate limits. The app's native approval: `teitunnel mcp` asks through the control connection (`agent.approve`) while the app runs, else elicitation; Settings lists connected agents.)
- [x] One-click "Connect an AI tool" in Settings: writes the client's MCP config (with consent), shows the command for others.
- [x] Agent Skill (`SKILL.md`) and `AGENTS.md` snippet in the docs; `llms.txt` already exists.
- [x] No built-in LLM or cloud AI service (decision Q4): agents bring the model; Teitunnel stays local and free.
- [x] MCP exposure preset: detect a local MCP server (Streamable HTTP probe on `/mcp`, SSE), share it on your own domain (Quick Tunnels don't carry SSE) with stream keep-alive and a bearer token checked by Lens (`Authorization: Bearer`, what MCP clients send), and print ready configs for Claude Code, Cursor, VS Code, Claude.ai and ChatGPT connectors. (`share --mcp`, MCP `expose_mcp_server`; Claude.ai/ChatGPT connectors get notes, not configs.)
- [x] Bearer protection preset for local AI servers (Ollama, vLLM, LM Studio): OpenAI-compatible clients send `Authorization: Bearer`, which Lens checks; Access service tokens as the Cloudflare-enforced alternative. (`share --ai`.)
- [ ] Later phase: OAuth 2.1 authorization in front of a local MCP server (Lens as the authorization server, each new client approved in the app), so claude.ai and ChatGPT can connect without a static key.

## M12-04 · Protection
- [x] Password page, secret link (`?key=` sets a cookie), HTTP basic auth, IP/CIDR allow and deny, user-agent block (bots), per share or route, enforced in Lens (works on Quick Shares too). (Inspection Settings ▸ Protection, and the shield on a Quick Share card.)
- [ ] Cloudflare-enforced options where the user has Access: email code (exists), GitHub/Google login presets, service tokens for machine callers (done, D-106), and "bypass for /webhooks/*" so a protected app still receives webhooks (done, D-131). Left: GitHub/Google login presets.
- [x] Clear labels on where it's enforced ("on this computer" vs "at Cloudflare": the latter keeps working when the app is closed).
- [x] Edge rules scoped to one hostname (never zone-wide: Bot Fight Mode applies to the whole domain, so there's no toggle for it): challenge or block bots/AI crawlers, rate limiting (Free: one rate-limiting rule per zone, 10-second window, per IP; five custom rules), header rules (Transform Rules, 10 on Free, no regex). Teitunnel owns only the rules it creates, merges its hostnames into one expression where the quota is one rule, shows the quota, and goes through plan → apply with undo. (Done: app, CLI `protect`, MCP. Verified 2026-09-24: a Free zone's rate limit can't match `http.host`, so rate limits need Pro; hostnames with the same limit share one rule. [research](../research/cloudflare-edge-rules.md))
- [x] Access service tokens for machine callers (free), created and shown once. (Done: the secret is never stored anywhere, not even the keychain: copied from Rust in the app, printed once by `teitunnel service-token create`, returned once to an approved agent; only id and client id are kept.)
- [x] No mTLS: client-certificate enforcement needs paid plans (Access: Enterprise), so free users couldn't use it.

## M12-05 · Analytics, uptime, alerts
- [ ] Per route and share: requests/s, p50/p95/p99 latency, 2xx/3xx/4xx/5xx, bandwidth, top paths, top countries, user agents/bots, from Lens (precise, local) and Cloudflare's GraphQL Analytics (edge view, any connector; needs Account Analytics Read, decision Q3).
- [x] Uptime: every route checked through the edge on a schedule; history, incidents, response time chart; notification when down/recovered; optional status badge.
- [x] Alerts: 5xx rate, latency, connector down, certificate/DNS problems, quota-like limits (429s on Quick Share), with quiet hours.
- [x] Overview becomes a live dashboard: health, traffic, errors, recent requests, all at a glance. (Traffic, Errors (5xx and unreachable over 5 min) and Uptime tiles; Recent Requests from the inspector, live, each opening selected in the Inspector.)

## M12-06 · Sharing power-ups
- [x] Pause/resume a share on your domain: the hostname stays reserved (route kept, connector paused, a friendly "paused" page served by Lens); resume with the same URL.
- [x] Stable names: `{project}.dev.example.com` from the detected project; `{branch}` from git (`teitunnel share 3000 --on {branch}.dev.example.com`); remember per folder.
- [x] Share a folder (static file server in Lens, directory listing optional, single-page-app fallback), from the app (drag and drop) and CLI.
- [x] Snapshot to the user's own Cloudflare (Workers static assets, [research](../research/cloudflare-snapshots.md)) so a preview stays online when the computer sleeps (decision Q5): from a folder, a build or a crawl of a running site; incremental uploads, versions and rollback, custom hostname or workers.dev, password or Access login, expiry; app, CLI and docs.
- [x] Feedback overlay/comments on shares (decision Q6). (Live shares and inspected routes keep comments on this computer, answered by Lens; Snapshots in a D1 database on the account; Comments view, notifications, CLI `comments`, MCP `comments_*`. Screenshots of a region were left out.)
- [x] Offline page: when this computer is off, a route or domain share shows a friendly page from a tiny Worker on the user's account instead of Cloudflare's 1033. (A fail-open Worker route `hostname/*` whose script proxies to the tunnel and answers 530 with the page; opt-in per route, plan → apply with undo; research: [cloudflare-workers-features.md](../research/cloudflare-workers-features.md).)
- [x] Scheduled shares: on during set hours/days, off otherwise (off: the paused page, applied by whoever serves the route).

## M12-07 · Everywhere
- [x] Tray/menu bar: share a detected service in one click, copy recent URLs, pause all. (`core::quick_actions`; pause and resume need an inspected share; also stop all.)
- [x] `teitunnel://` deep links (share port, open route, open inspector) for Raycast, Alfred and scripts. (D-102; registration by the installers, unverified in packaged builds.)
- [x] Raycast extension (share, list, copy, stop), VS Code extension (Ports view integration, inspector panel, status bar), in `integrations/` on a shared TypeScript client. The Ports view gets a context-menu item (its data API is still proposed); the inspector opens in the app. Not published yet. The optional browser extension is left for later.
- [x] Global shortcut to share the frontmost dev server. (Off by default, Settings ▸ Integrations; shares the one running dev server, else opens Quick Share; "frontmost" isn't detectable portably.)
- [x] Local HTTPS domains (`app.test`/`.local` with a local CA, LocalCan parity; decision Q7). (D-101, D-115; untested on real systems; share/route targets and a one-click `name.localhost` for detected services later.)
- [x] First: a local control connection to the running app (Unix socket / named pipe, current user only) that the CLI, extensions and launchers use; everything below builds on it. (D-102, `crates/control`; the Windows pipe is untested on Windows.)
- [x] JetBrains plugin (after VS Code; the VS Code extension also runs in Cursor and Windsurf through Open VSX). (`integrations/jetbrains`, IntelliJ Platform Gradle Plugin 2.x; not published.)
- [x] Live shell completion (hostnames, tunnels, domains from the local store, no network).
- [x] `teitunnel top`: a live terminal dashboard of shares, routes, traffic and requests. (The inspector now sends `requestArrived`, bounded and coalesced.)

## M12-08 · Cloudflare depth
- [ ] Private hostname routes for WARP users (GA 2026-08-11) next to private networks.
- [ ] Batch route creation (2026-09-02) for imports and multi-route changes.
- [ ] Token template scoped to one tunnel where Cloudflare allows it (per-tunnel permissions, 2026-05-21); confirm permission group keys against `GET /user/tokens/permission_groups`.
- [ ] Managed cloudflared on Intel Macs and 32-bit Windows after Cloudflare stops building them (2027): keep the last version with a notice, prefer Homebrew.
- [ ] Optional WAF/rate-limit toggles per route (Rulesets API) and cache bypass for dev.

## M12-09 · Robustness (from competitors' and cloudflared's issues)
- [ ] Large and multipart uploads, WebSocket and SSE through every path (E2E tests with fake-cloudflared streaming).
- [ ] Framework guides and automatic `X-Forwarded-Host`/`X-Forwarded-Proto` for Laravel/Livewire, Rails, Django, Next.js, Nuxt (wrong-domain assets, CORS).
- [ ] Sleep/wake and network change: reconnect fast, verify, notify only if it stays down.
- [ ] WSL: detect services in WSL and rewrite localhost (FlareDeck parity).
- [x] A leftover `~/.cloudflared/config.yml` never breaks Quick Shares (pass an empty config explicitly).

## M12-11 · CI/CD and teams
- [x] GitHub Action `teispace/teitunnel-action`: on a pull request, publish a preview at `pr-<n>.preview.example.com` either as a live share from the runner or as a snapshot (stays up after the job), comment the URL on the PR, update it on new commits, remove everything when the PR closes; account-owned tokens (D-095). GitLab CI template too.
- [x] Name reservations for teams sharing an account: the owner (person/machine) is written into the DNS record's comment, so every Teitunnel sees who holds a name; leases with expiry (`alice.dev.company.com` permanent, `pr-402…` until closed); conflicts shown before any change.

## M12-12 · More (maintainer: "any other best features", 2026-09-24)
- [x] Webhook inbox: a Worker on the user's account receives webhooks while this computer is off and delivers them in order when it's back, with the inspector showing both the arrival and the delivery. (D1, not Queues: Queues keep messages 24 h on Free; bounded and retained 1–30 days; optional signature check with the keychain secret; delivery through the route's service, so an inspected route shows each delivery.)
- [x] Project file `teitunnel.yml` in a repo: its shares, routes, protection, local domains and snapshots; `teitunnel up` (or opening the folder in the app) applies it through plan → apply; checked into git so a team shares it. (D-107; local domains applied since D-115.)
- [x] Exposure check before a share goes public: probe the origin for common leaks (`/.env`, `/.git/`, directory listings, debug pages such as Django/Laravel/Rails error pages, open admin panels, framework dev tools) and warn with details; never blocks, one click to continue. (D-108)
- [x] Auto-stop idle shares after N minutes without requests; notify when a request hits a watched path.
- [x] OpenAPI from traffic: infer an API description (paths, methods, parameters, JSON schemas from observed bodies) from captured exchanges; export for docs and agents.
- [x] Move to a new computer: an encrypted export of Teitunnel's setup (accounts by name only, routes, settings, local domains; never tokens) to restore elsewhere. (D-109; local domains included since D-115.)

## M12-10 · Reach
- [ ] Comparison pages (vs ngrok, LocalCan, Pinggy, Dev Tunnels, Tailscale Funnel, raw cloudflared), webhook guides per provider, "expose an MCP server" guide.
- [ ] Listings: awesome-tunneling, Raycast Store, VS Code Marketplace, Homebrew core, winget, Flathub; Show HN / Product Hunt when M12-02 and M12-03 ship.

## Design: local HTTPS domains (decision 7)
- **Names:** `name.localhost` by default: Chrome, Firefox and Safari (through macOS's resolver) send `*.localhost` to loopback with no setup (RFC 6761 §6.3; W3C Secure Contexts treat it as trustworthy). Optional `name.test` for tools that don't resolve `.localhost` (curl on some systems, Node before 17, Java, Windows/Linux system resolvers): a tiny DNS responder in Teitunnel on `127.0.0.1:<port>` plus a per-TLD resolver entry, added once with the user's approval (macOS `/etc/resolver/test` with `port`; Linux systemd-resolved drop-in `Domains=~test`; Windows NRPT rule). Optional LAN access for phones: advertise `name.local` over mDNS pointing at the LAN address, with a QR code to install the CA profile on the phone.
- **Certificates:** Teitunnel's own root CA (rcgen), **name-constrained** (X.509 NameConstraints: only `.localhost`, `.test`, `.local`, loopback and private IPs), so even a stolen key can't sign a real site (mkcert's CA isn't constrained). The CA key lives in the OS keychain; leaf certificates are issued on demand per name via SNI, short-lived (30 days) and renewed automatically.
- **Trust:** user-level where possible, no sudo: macOS user trust settings (the system asks for the user's password once), Windows CurrentUser Root store (Windows shows its own confirmation), Linux system store with a one-time privileged step plus NSS databases for Chrome/Firefox (`certutil`, via a typed process builder). Firefox on macOS/Windows: enterprise roots preference or its NSS store. Removing local domains removes the trust entries.
- **Ports:** Lens listens on 443/80. Windows allows loopback unprivileged; macOS allows it only on the wildcard address (verified on macOS 27), so Lens binds there and drops connections from other machines (D-101); on Linux, if `ip_unprivileged_port_start` > 443, offer the one-time fix (setcap or sysctl) or fall back to 8443/8080 with the port in the URL.
- **UI:** every share, route and detected service can get `https://name.localhost` in one click; a Local domains list; automatic names from the project (`myapp.localhost`) and wildcard subdomains (`*.myapp.localhost` to the same service).

## Design: comments and feedback (decision 6)
- An overlay script injected by Lens (live shares and routes being inspected) or by the snapshot Worker (HTMLRewriter): a small comment button; reviewers click a point on the page to pin a comment (page path, CSS selector, position relative to the element, viewport size, optional screenshot of the region), reply, resolve. No external assets; accessible; can be switched off per share.
- Identity: reviewer name (and email) typed once and remembered in their browser, or the Access identity when the share requires a login.
- Storage: live shares keep comments in Teitunnel's store on this computer (the overlay talks to Lens's reserved `/__teitunnel/` API through the tunnel); snapshots keep them in the user's Cloudflare account (the snapshot Worker with D1 or a Durable Object), synced into the app.
- App: a Comments view per share/snapshot with threads, jump to page, resolve/reopen, notifications for new comments; MCP tools (`comments_list`, `comments_reply`, `comments_resolve`) so agents can close the feedback loop.

## Decisions (maintainer, 2026-09-24)
1. Quick Shares are inspected by default: in memory, last 1,000 exchanges per share, secrets masked, one switch off.
2. Routes: the most robust design (below, "Inspecting a route").
3. Analytics may add Account Analytics Read (token template, OAuth scope, asked in place).
4. No built-in "Explain this" / LLM. Agents come through an extremely capable MCP server.
5. Snapshots now: flexible and robust, on the user's own Cloudflare account.
6. Comments/feedback on shares and snapshots now.
7. Local HTTPS domains now.
8. Order as listed.
Work happens locally and reaches GitHub in batches (CI is slow). 0.2.0 still has to be out before 2026-10-05.

## Inspecting a route (decision 2)
Lens runs in the process that runs the route's connector: the app, `teitunnel up`, or `teitunnel serve`. Turning inspection on is a plan → apply change that points the rule's service at Lens's address and stores the original service in `inspected_routes` (store). It's reverted by: turning it off; the process stopping (on quit, and swept at next start after a crash, like domain shares, D-068); Always-on switching to a system service without Lens. The Doctor flags a rule pointing at a Lens address with no Lens listening (`inspect.orphan`) with a one-click restore. Routes on other machines use the log-based request feed only.

## Module boundaries
- `crates/lens` (new, no Tauri): listeners, taps, capture store, masking, replay, export, webhook verification, gates (protection), static upstream, HTML injection hook, host-based routing for local domains. Everything else calls its public API.
- `crates/cf-api`: analytics (GraphQL), Workers static assets / snapshots, anything Cloudflare.
- `crates/core`: orchestration (which share/route has a tap, persistence, schedules, uptime, alerts), no HTTP servers of its own.
- `crates/mcp` (new): the MCP server (tools, resources, prompts, approvals) over core; `teitunnel mcp` (stdio) and `teitunnel serve` (Streamable HTTP) host it.
- `apps/desktop`: UI only; `apps/cli`: commands only.

## Questions asked (answered above)
1. **Inspect Quick Shares by default?** Recommended: yes, in memory, last 1,000 requests, masked secrets, one switch to turn off.
2. **Inspecting a route** changes its Cloudflare service to Lens while the app runs (reverted on quit). Acceptable, or Quick Shares only at first?
3. **Analytics** needs the Account Analytics Read permission (new token permission and OAuth scope for existing users; the app asks in place). OK to add?
4. **AI:** MCP-first with no built-in LLM (recommended), or also an optional "Explain this" with the user's own API key?
5. **Snapshots** on the user's Cloudflare (Pages/Workers) need another permission and a build step. In M12 or later?
6. **Comments/feedback overlay** on shares: in M12 or later?
7. **Local HTTPS domains** (`.test`/`.local`): in M12 or later? They need a local certificate authority installed in the system trust store.
8. **Order:** as listed (01 → 10), or MCP before the inspector?
