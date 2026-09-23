# M9: Advanced features (v1.x)

**Goal:** the features power users ask for next, built on the same engine (every Cloudflare change still goes plan → apply).
**Order:** by value to macOS users now and by what can be verified here; each is its own PR-sized task.

### M9-01 · Export
- [x] Export this Mac's tunnel and routes as a cloudflared `config.yml`, a Docker Compose service (token from `${TUNNEL_TOKEN}`, image pinned to the local cloudflared version), or Terraform for the Cloudflare provider v5 with `import` blocks for the tunnel, its config and each record (records keep their comment, TTL and proxied flag, so the plan is empty). Routes ▸ Export; copy or save to Downloads (D-055).

### M9-02 · CLI
- [x] `apps/cli` (`teitunnel-cli`) over `teitunnel-core`: `accounts`, `routes` (status probed from the connector's `/ready`), `route add/remove` (plan shown, `y/N`, `--yes`, `--replace` for foreign records, verified through the edge afterwards), `export <config-yaml|docker-compose|terraform>`, `--json`. Uses the app's data folder and keychain; never runs connectors (D-056).
- [ ] `share <port>` (a Quick Share that lives as long as the command), `doctor`, shell completions, and installing the binary with the app (a `teitunnel` symlink, since the build can't share the app's binary name).

### M9-03 · Protect with Access
Per-route "Require login": visitors sign in (one-time PIN to their email, or the account's other login methods) before reaching the service. API facts from Cloudflare's OpenAPI schema (`cloudflare/api-schemas`, 2026-09-23): a `self_hosted` Access application (`POST /accounts/{id}/access/apps`, `domain` = hostname + optional path, `session_duration`, inline `policies` with `decision: "allow"` and `include` rules `{email: {email}}` / `{email_domain: {domain}}`); it needs a Zero Trust organization (`GET /access/organizations`); login methods default to every configured identity provider, and One-time PIN is one (`type: "onetimepin"`).
- [x] cf-api: Access apps (list by domain, create, update, delete), organization, identity providers; `tools/fake-cloudflare` serves them; tests.
- [x] Engine: `RouteSpec.access` (emails, email domains); planner steps create/update/delete the route's Access app (and add One-time PIN when the account has no login method), each undoable; Teitunnel owns only apps it created (local index + name "Teitunnel · <host>"); removing a route removes its app; no organization → a clear plan error.
- [x] Verify: a redirect to the Access login counts as working ("Protected").
- [x] UI: Route sheet ▸ Advanced ▸ Require a login (emails / @domains), lock in the routes list, Login row in the inspector, "Protected by a login" after the check, login steps and Activity deltas; CLI `route add --allow` (D-057).
- [x] Follow-ups: Doctor check `access.orphan` for an owned Access app whose route no longer exists, fixed by a planned `RemoveLogin` change (safe: owned only). The token template stays least-privilege (Access is optional); a token without Access permissions gets an error naming the two permissions to add.
- [x] Docs: site guide "Require a login", CLI reference.

### M9-04 · Replicas and remote connectors
- [x] Connectors per machine (grouped by connector id from the tunnel list, This Mac identified by `/ready`'s `connectorId`); Doctor `tunnel.other_connectors` when another machine runs this Mac's tunnel.
- [x] Remote logs of any connector through the Management API (`cf_api::LogStream`, `core::remote_logs`): per-connector sessions, bounded ring, reconnect with a fresh token, stop after 30 s unread, token never leaves Rust. Tunnels ▸ connector ▸ Logs (D-058).
- [x] Replicas ("run the same tunnel on several machines") deliberately not a one-click feature: Teitunnel's routes point at this Mac's localhost, so a second connector would get a share of requests for services it doesn't have. Running elsewhere stays Export (Docker Compose / config.yml) for origins reachable from both (D-058).

### M9-05 · Private networks
- [ ] CIDR routes and virtual networks for WARP clients; `cloudflared access` helpers for SSH/RDP/TCP.

### M9-06 · i18n
- [ ] Extract strings, community translations.
