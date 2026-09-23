# M10: Everything, everywhere (parity and beyond)

**Goal:** Teitunnel covers every feature of comparable tools ([research/cloudify.md](../research/cloudify.md)) and runs anywhere: desktop, headless server, container. Every error the user can resolve has an in-place fix.
**Release:** v1.x (before distribution, per the maintainer: release work comes last).
**Verification:** unit + E2E against fake-cloudflare/fake-cloudflared; server mode in the Linux CI job; Docker image built in CI.

### M10-01 · Fix in place (no dead ends)
- [ ] Shared "fix" card (link to the right Cloudflare page, re-check on focus, resume) for every credential/permission gap: Tunnel · Edit, DNS · Edit per domain, Zone · Read, private networks (Tunnel · Edit / Zero Trust), connector logs, Access (done, D-064).
- [ ] Zero Trust organization missing: open the Zero Trust onboarding page, re-check, continue.
- [ ] Sweep every user-facing error (core catalog) into "fixable" (gets an action) or "informational" (clear wording); a test keeps the list complete.

### M10-02 · Several tunnels per machine
- [ ] Tunnels on this machine: create named tunnels (e.g. production, staging) besides the default; each route chooses its tunnel (default: the machine tunnel).
- [ ] Start/stop, Always-on and delete per tunnel (plan → apply; deleting removes its routes' DNS it owns).
- [ ] Adopt an existing tunnel of the account to run on this machine (fetch its token; refuse if it's remotely managed by another tool without consent).
- [ ] CLI: `tunnel list|create|start|stop|delete|adopt`, `route add --tunnel`.

### M10-03 · Share on your own domain
- [ ] Quick Share option "On my domain": a temporary route (hostname on a chosen zone) on the machine tunnel, removed when the share stops, expires or the app quits (and cleaned by the Doctor if left behind).
- [ ] Optional expiry and login (Access) for it; CLI `share --on app.example.com`.
- [ ] `share list` / `share stop` across the app and CLI processes.

### M10-04 · Headless server mode
- [ ] `teitunnel-cli setup`: interactive (or flags/env) account connect with a token, stored in the OS keychain, or in a root-only file on servers without one (`TEITUNNEL_TOKEN_FILE`).
- [ ] Connectors without the app: `teitunnel-cli up` runs the machine's tunnels in the foreground (for containers/systemd), `always-on on|off|status` installs services.
- [ ] System-level services for servers: systemd system unit when root, cron `@reboot` fallback without systemd.
- [ ] Guides: any VPS, AWS, Azure, GCP.

### M10-05 · Container
- [ ] Docker image (`ghcr.io/teispace/teitunnel`: CLI + verified cloudflared, non-root, multi-arch) running `teitunnel-cli up`.
- [ ] Compose recipe: route to containers by service name; healthcheck; docs.

### M10-06 · Browser UI and local API for headless machines
- [ ] `teitunnel-cli serve`: the same React UI in a browser over an HTTP transport (the IPC commands behind one typed adapter), loopback-only by default.
- [ ] Auth: a password set at `setup` (argon2), session cookie (HttpOnly, SameSite=Strict), CSRF protection, rate limiting; secrets never returned.
- [ ] Documented HTTP API (OpenAPI) with API keys for automation.

### M10-07 · Load balancing across machines
- [ ] When the account has Cloudflare Load Balancing: a route can be served by several machines' tunnels through a pool with a health monitor (plan → apply, undo, ownership).
- [ ] Health per origin in the UI; the Doctor explains when the add-on is missing.

### M10-08 · Everything documented
- [ ] Docs site on Fumadocs (every feature and step, CLI and API references, guides) and a landing page (see M11).

# M11: Landing page and docs site
- [ ] Landing page in the style of cursor.com: product-led hero with the real app, feature sections, platform downloads, FAQ; fast, accessible, dark/light.
- [ ] Docs on Fumadocs (Next.js): getting started, every feature, guides (server, Docker, cloud VMs, Access, private networks), CLI reference, API reference, troubleshooting, search.
- [ ] Deploy (with the release work).
