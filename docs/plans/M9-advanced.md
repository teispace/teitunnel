# M9: Advanced features (v1.x)

**Goal:** the features power users ask for next, built on the same engine (every Cloudflare change still goes plan → apply).
**Order:** by value to macOS users now and by what can be verified here; each is its own PR-sized task.

### M9-01 · Export
- [x] Export this Mac's tunnel and routes as a cloudflared `config.yml`, a Docker Compose service (token from `${TUNNEL_TOKEN}`, image pinned to the local cloudflared version), or Terraform for the Cloudflare provider v5 with `import` blocks for the tunnel, its config and each record (records keep their comment, TTL and proxied flag, so the plan is empty). Routes ▸ Export; copy or save to Downloads (D-055).

### M9-02 · CLI
- [ ] `apps/cli` over `teitunnel-core`: `teitunnel route add app.xyz.com :3000`, `route list/remove`, `share 3000`, `doctor`, `export`. Same plan → apply engine, plan shown before applying (`--yes` to skip), keychain credentials shared with the app.

### M9-03 · Protect with Access
- [ ] Per-route "Require login" (emails / email domain / one-time PIN) via Access applications + policies; optional OAuth scopes; the Access application is part of the route's plan and ownership.

### M9-04 · Replicas and remote connectors
- [ ] See connectors of this Mac's tunnel running elsewhere; run the same tunnel on several machines; remote log streaming via the Management API.

### M9-05 · Private networks
- [ ] CIDR routes and virtual networks for WARP clients; `cloudflared access` helpers for SSH/RDP/TCP.

### M9-06 · i18n
- [ ] Extract strings, community translations.
