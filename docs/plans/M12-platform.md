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
- [ ] When a share or route goes live, the verifier fetches it once; a Vite "Blocked request. This host is not allowed", webpack "Invalid Host header", Next.js/Angular host checks or Rails `HostAuthorization` answer is recognised.
- [ ] Fix in place: "Your dev server rejects this address. [Send Host: localhost]" (sets `httpHostHeader` for that share/route, re-checks) or "[Show the one-line config]" with the exact `allowedHosts` line for the detected framework and version.
- [ ] Discovery marks known dev servers; new shares of them get the host header by default when safe (not for Next.js server actions or OAuth callbacks, where it breaks origin checks), with an explanation.
- [ ] SSE on a Quick Share (Cloudflare doesn't carry it): detect `text/event-stream` and suggest "Share on my domain" (MCP servers on SSE transport, streaming UIs).
- [ ] Explain 413 (100 MB body limit on Free/Pro), 429 (Quick Share 200 in-flight), 502/1033 inline with the fix.

## M12-02 · Inspector
- [ ] `crates/lens`: streaming proxy, capture ring, masking, timing; property tests for header/body handling; fuzzed request parsing; benchmarks (added latency under 1 ms p99 locally, no buffering for streams).
- [ ] Quick Share and route integration (above), CLI integration.
- [ ] UI, per share/route and one "All traffic" view: live list (method, path, status, duration, size, time), filters (status class, method, path/host text, duration), full-text search in bodies, detail pane (request/response headers, body: pretty JSON, form, multipart parts, image preview, hex for binary), timing bar.
- [ ] Replay to the origin; edit and replay (method, path, headers, body); replay N times; compare two exchanges (diff).
- [ ] Export: cURL, HTTPie, fetch, raw HTTP, HAR, JSON, Markdown (for issues and agents); redacted by default.
- [ ] Webhooks: recognise Stripe, GitHub, Slack, Shopify, Clerk, Twilio, Linear, Discord and standard-webhooks signatures; verify with a secret kept in the keychain; show "signature valid / invalid / expired timestamp"; replay keeps or recomputes the signature (user's choice).
- [ ] Mock/stub: answer a path with a saved response when the origin is down (keeps webhook senders happy while you restart).
- [ ] Breakpoints (later): pause matching requests, edit, continue.
- [ ] Performance budget: 60 fps list with 10,000 exchanges (virtualised, like the log viewer, D-051).

## M12-03 · Agents (MCP) and AI
- [ ] `teitunnel mcp`: MCP server over stdio (Claude Code, Cursor, VS Code, Codex, Windsurf, Zed), and Streamable HTTP from `teitunnel serve` with API keys for remote agents.
- [ ] Tools: `share_port`, `stop_share`, `list_shares`, `list_routes`, `plan_change` (returns the same plan people review), `apply_plan` (by fingerprint), `verify_route`, `doctor`, `fix_issue`, `logs_tail`, `traffic_list`, `traffic_get`, `traffic_replay`, `wait_for_request` (block until a matching request arrives: webhook testing without polling), `traffic_stats`, `export_config`.
- [ ] Resources (routes, shares, domains, issues) and prompts ("debug this failing webhook", "put my dev server online on my domain with a login").
- [ ] Safety: three modes per client (read-only, ask, full). "Ask" shows a native approval in the app (or the terminal) with the plan before any Cloudflare change; secrets never exposed to agents (masked bodies unless allowed); every agent action in Activity, marked with the client's name; rate limits.
- [ ] One-click "Connect an AI tool" in Settings: writes the client's MCP config (with consent), shows the command for others.
- [ ] Agent Skill (`SKILL.md`) and `AGENTS.md` snippet in the docs; `llms.txt` already exists.
- [ ] No built-in LLM or cloud AI service (decision Q4): agents bring the model; Teitunnel stays local and free.

## M12-04 · Protection
- [ ] Password page, secret link (`?key=` sets a cookie), HTTP basic auth, IP/CIDR allow and deny, user-agent block (bots), per share or route, enforced in Lens (works on Quick Shares too).
- [ ] Cloudflare-enforced options where the user has Access: email code (exists), GitHub/Google login presets, service tokens for machine callers, and "bypass for /webhooks/*" so a protected app still receives webhooks.
- [ ] Clear labels on where it's enforced ("on this computer" vs "at Cloudflare": the latter keeps working when the app is closed).

## M12-05 · Analytics, uptime, alerts
- [ ] Per route and share: requests/s, p50/p95/p99 latency, 2xx/3xx/4xx/5xx, bandwidth, top paths, top countries, user agents/bots, from Lens (precise, local) and Cloudflare's GraphQL Analytics (edge view, any connector; needs Account Analytics Read, decision Q3).
- [ ] Uptime: every route checked through the edge on a schedule; history, incidents, response time chart; notification when down/recovered; optional status badge.
- [ ] Alerts: 5xx rate, latency, connector down, certificate/DNS problems, quota-like limits (429s on Quick Share), with quiet hours.
- [ ] Overview becomes a live dashboard: health, traffic, errors, recent requests, all at a glance.

## M12-06 · Sharing power-ups
- [ ] Pause/resume a share on your domain: the hostname stays reserved (route kept, connector paused, a friendly "paused" page served by Lens); resume with the same URL.
- [ ] Stable names: `{project}.dev.example.com` from the detected project; `{branch}` from git (`teitunnel share 3000 --on {branch}.dev.example.com`); remember per folder.
- [ ] Share a folder (static file server in Lens, directory listing optional, single-page-app fallback), from the app (drag and drop) and CLI.
- [x] Snapshot to the user's own Cloudflare (Workers static assets, [research](../research/cloudflare-snapshots.md)) so a preview stays online when the computer sleeps (decision Q5): from a folder, a build or a crawl of a running site; incremental uploads, versions and rollback, custom hostname or workers.dev, password or Access login, expiry; app, CLI and docs.
- [ ] Feedback overlay/comments on shares (later; LocalCan's newest feature; decision Q6).

## M12-07 · Everywhere
- [ ] Tray/menu bar: share a detected service in one click, copy recent URLs, pause all.
- [ ] `teitunnel://` deep links (share port, open route, open inspector) for Raycast, Alfred and scripts.
- [ ] Raycast extension (share, list, copy, stop), VS Code extension (Ports view integration, inspector panel, status bar), optional browser extension (open current localhost tab as a share).
- [ ] Global shortcut to share the frontmost dev server.
- [ ] Local HTTPS domains (`app.test`/`.local` with a local CA, LocalCan parity; decision Q7).

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
- [ ] A leftover `~/.cloudflared/config.yml` never breaks Quick Shares (pass an empty config explicitly).

## M12-10 · Reach
- [ ] Comparison pages (vs ngrok, LocalCan, Pinggy, Dev Tunnels, Tailscale Funnel, raw cloudflared), webhook guides per provider, "expose an MCP server" guide.
- [ ] Listings: awesome-tunneling, Raycast Store, VS Code Marketplace, Homebrew core, winget, Flathub; Show HN / Product Hunt when M12-02 and M12-03 ship.

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
