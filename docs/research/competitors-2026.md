# Competitors and developer pain points (2026-09-24)

Research for the M12 plan ([plans/M12-platform.md](../plans/M12-platform.md)). Facts with sources, read 2026-09-24.

## At a glance

| | Price / free limits | GUI | Inspector + replay | Access control | AI / MCP | Own domain | Notes |
|---|---|---|---|---|---|---|---|
| **ngrok** | Free: 3 endpoints, 1 GB/mo, 20k requests/mo, 1 dev domain, interstitial page on HTML; paid from $8/mo | Web dashboard only | Yes (Traffic Inspector: full capture, edit & replay, 24 h retention free, 72 h paid, up to 90 d add-on) | OAuth/OIDC, IP rules, Traffic Policy (rate limit, rewrite, block bots) | Agent Skill (May 2026), no first-party MCP; AI Gateway product | Paid | Most polished; limits and interstitial drive people away |
| **LocalCan** 3.x | Free: 1 URL at a time, 60-min sessions, 1 GB/mo; Solo $8, Pro $12, Teams $45+$15/seat, Lifetime $99 | Mac + Windows app, CLI everywhere (Linux CLI-only) | Yes: list/filter/watch, replay, export cURL/HAR/HTTP/JSON/Markdown with `--redact` | Password page, secret links, IP and user-agent rules (paid), enforced at their edge | MCP server over stdio with read-only / read-write modes and redaction toggle; agents read comments, publish snapshots | Paid (2–unlimited) | `.local` domains with local CA + mDNS; Snapshots (static copies online while the machine is off); Figma-like comments on snapshots; YAML projects; Go daemon |
| **Pinggy** | Free: 60-min tunnels, random subdomains | Desktop app, TUI, CLI (SSH, no install) | Web debugger: inspect, modify, replay | Basic auth, bearer/key, IP allowlist (CIDR) | Agent Skill + MCP server | Paid | HTTP/TCP/UDP/TLS, header rewrite rules, QR, Node/Python SDKs |
| **Cloudflare Quick Tunnels / Wrangler & Vite `t`** | Free, no account | None (CLI keypress) | No | Named tunnels + Access (manual) | Cloudflare skills/MCP for the platform | Named tunnels | Quick Tunnels: 200 concurrent requests (429 after), no SSE, random host each run, no SLA |
| **Microsoft Dev Tunnels** | Free preview: 5 GB/mo, 10 tunnels, 10 ports each | Built into VS Code Ports view | No | GitHub/Microsoft login by default, or public | via VS Code | No | Preview, no SLA |
| **Tailscale Funnel** | Free tier (6 users) | CLI only for Funnel | No | Tailnet policy | No | No (ts.net only) | Ports 443/8443/10000 only, fixed bandwidth limits |
| **zrok** | Free/open source; managed $7/$20 | Agent UI | No | Private shares between zrok users | No | Self-hosted | OpenZiti; files and web content, self-hostable |
| **FlareDeck** | Free, MIT, ~5 stars | Tauri desktop (Linux "in testing") | "Bounded webhook inspection" (redacted) | Via Cloudflare | Local stdio MCP, trusted workspaces | Via Cloudflare | Closest in stack (Rust + Tauri + cloudflared): profiles, visual ingress editor, raw YAML editor, WSL localhost rewrite, config backups |
| **LocalXpose, Localtonet, InstaTunnel, localhost.run, frp, inlets, Expose** | Mixed | Some GUIs | LocalXpose: request/response view, header editing | Basic auth, IP allowlist | InstaTunnel: MCP | Mixed | Crowded low end |

## What developers complain about

- **Limits and friction** (ngrok): 1 GB and 20k requests a month on free, the interstitial page on every HTML visit (breaks demos and webhooks from browsers), 3 endpoints, paid custom domains. ([ngrok free plan](https://ngrok.com/docs/pricing-limits/free-plan-limits), ["The Great ngrok Migration"](https://instatunnel.substack.com/p/the-great-ngrok-migration-why-developers))
- **Quick Tunnels**: random URL each run breaks anything pasted elsewhere; 200 in-flight requests then 429; no SSE (breaks MCP servers on SSE transport and streaming UIs); a leftover `~/.cloudflared/config.yml` stops quick tunnels working; no SLA. ([Pinggy](https://pinggy.io/blog/best_ngrok_alternatives/), [Cloudflare docs](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/do-more-with-tunnels/trycloudflare/))
- **Dev servers behind a tunnel**: Vite 6.0.9+ answers "Blocked request. This host is not allowed" until `server.allowedHosts` lists the tunnel host; webpack "Invalid Host header"; HMR and CSRF issues. ([Cloudflare Workers docs](https://developers.cloudflare.com/workers/development-testing/local-dev-tunnels/))
- **Framework host issues** (LocalCan issues): Laravel/Livewire assets requested from the wrong domain, Nuxt CORS errors, multipart POSTs failing, body size limits, long stretches of 502s, `.local` domains slow or broken after network changes and macOS updates. ([LocalCan issues](https://github.com/LocalCan/LocalCanApp/issues))
- **Cloudflare Tunnel itself** (most-reacted [cloudflared issues](https://github.com/cloudflare/cloudflared/issues)): gRPC support (#491), Upgrade header stripped on WebSocket POST (#883, #990), SSE buffering (#199), log format option (#1033), `~` clutter / XDG (#119), no DNS route delete from the CLI (#781), deleting a tunnel leaves DNS (#354), "context canceled" errors (#1012, #1379), QUIC slower than HTTP/2 for some (#895). Error 1033 (no healthy connector) is the most searched tunnel error. The 100 MB request body limit (Free/Pro) surprises people uploading files.
- **Webhooks**: you don't control the sender; people want capture, inspect, replay and signature verification (Stripe, GitHub, Slack, Shopify, Clerk). ([WebhookRelay guide](https://webhookrelay.com/blog/how-to-test-webhooks/))
- **AI agents** need to expose local MCP servers and dev servers, read traffic and act on it; transport must be Streamable HTTP or SSE-capable. ([InstaTunnel on MCP](https://instatunnel.substack.com/p/the-ai-agent-workflow-native-mcp), [ngrok Agent Skill](https://ngrok.com/blog/new-ngrok-ai))

## Where Teitunnel already stands out

Free and open source with no bandwidth or request caps, no interstitial, your own Cloudflare account and domains, native app on all three systems plus CLI, server dashboard and API, every change previewed with undo, Doctor with fixes in place, Access logins, private networks, load balancing, export to config/Compose/Terraform, import, Always-on. It already solves cloudflared #781 and #354.

## Gaps against the field

1. No traffic inspector, replay or export (ngrok, LocalCan, Pinggy, LocalXpose).
2. No MCP server or agent skill (LocalCan, Pinggy, FlareDeck, InstaTunnel).
3. No password / secret-link / IP protection short of Cloudflare Access emails (LocalCan, Pinggy).
4. No per-route analytics beyond this machine's connector metrics; no uptime history or alerts.
5. Dev-server host errors are documented, not detected or fixed in place.
6. No static folder sharing / snapshots (LocalCan, zrok).
7. No pause/resume of a share with the same address; no branch- or project-named addresses.
8. No editor or launcher integrations (Dev Tunnels lives in VS Code; Raycast/Alfred).
9. No local HTTPS domains (LocalCan `.local`).

## Rechecked for the comparison pages (2026-09-26)

Every claim on the website's comparison pages (`apps/web/content/docs/compare/`) comes from this list. Read 2026-09-26.

- **ngrok** ([pricing](https://ngrok.com/pricing), [free plan limits](https://ngrok.com/docs/pricing-limits/free-plan-limits/), [inspection interface](https://ngrok.com/docs/agent/web-inspection-interface/)): Free has 1 GB data out and 20,000 HTTP requests a month, 5,000 TCP connections a month, up to 3 online endpoints, 1 user, one assigned dev domain (`*.ngrok-free.app`), no custom domains, and an interstitial page on HTML browser traffic (skipped with the `ngrok-skip-browser-warning` header or a non-browser User-Agent). Paid: Hobbyist $10/month, Pay-as-you-go $20/month plus usage, Enterprise. The agent's inspection interface is at `http://localhost:4040`, with replay and edit-and-replay. *Changed since 2026-09-24:* the cheapest paid plan is $10, not $8.
- **Tailscale Funnel** ([docs](https://tailscale.com/kb/1223/funnel)): only ports 443, 8443 and 10000; only names in the tailnet's `*.ts.net` domain (no custom domains); available on all plans; needs Tailscale v1.38.3+, MagicDNS, HTTPS certificates and a `funnel` node attribute in the policy file; "non-configurable bandwidth limits"; runs where the Tailscale CLI runs.
- **Microsoft Dev Tunnels** ([overview](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/overview), [FAQ](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/faq), [security](https://learn.microsoft.com/en-us/azure/developer/dev-tunnels/security)): public preview, "for adhoc testing and development, not for production workloads", no SLA; creating a tunnel needs a Microsoft, Entra ID or GitHub account; private to the creator by default, `--allow-anonymous`, `--tenant` or `--organization` to widen; URLs like `https://tunnelid-3000.devtunnels.ms`; an anti-phishing page on the first browser visit (skipped for non-GET, non-HTML `Accept`, or the `X-Tunnel-Skip-AntiPhishing-Page` header); unused tunnels deleted after 30 days by default; inspection through browser DevTools; Windows, macOS, Linux. Monthly limits are on Microsoft's limits page (not readable on 2026-09-26, so the page links to it instead of quoting numbers).
- **Pinggy** ([home](https://pinggy.io/)): starts with plain `ssh -p 443 -R0:localhost:8000 … free.pinggy.io`, no install; free tunnels time out after 60 minutes with a random subdomain that changes on restart; HTTP(S), TCP, UDP and TLS tunnels; custom domains on Pro and Enterprise; web debugger (inspect, modify, replay) in its dashboard. Prices weren't readable on 2026-09-26, so the page doesn't quote one.
- **LocalCan** ([pricing](https://www.localcan.com/pricing)): desktop app and CLI for Mac, Windows and Linux (*changed:* Linux was CLI-only on 2026-09-24; the pricing page now says "Desktop app + CLI Mac / Windows / Linux"); Free: 1 public URL, 60-minute sessions, 1 GB a month, 150 MB snapshots, MCP server, unlimited `.local` domains with HTTPS, inspection and replay; Solo $8, Pro $12, Teams $45 (3 seats, $15 each more), lifetime $99; custom domains from Solo (2), passwords and IP rules on paid plans; 14-day trial.
