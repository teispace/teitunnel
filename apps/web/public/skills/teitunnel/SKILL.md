---
name: teitunnel
description: Put local services on the internet and manage Cloudflare Tunnel routes on the user's own Cloudflare account with Teitunnel's MCP tools. Use when the user wants to share a dev server or port (a public link, or a hostname on their domain, optionally behind a login), test or debug webhooks (Stripe, GitHub, Slack…), add/change/remove routes, find out why a tunnel hostname is down (502, error 1033, DNS), read connector logs, or move routes to a server (Docker Compose, config.yml, Terraform).
---

# Teitunnel

Teitunnel manages Cloudflare Tunnel for the user: **routes** (a public hostname on one of
their domains → a service on a machine), **shares** (temporary public URLs), logins
(Cloudflare Access), DNS and the `cloudflared` connectors. Its MCP server is `teitunnel`
(tools below). If the tools aren't available, the user can connect them with
`teitunnel mcp install <client>` and restart the client.

## Rules

1. **Show before you change.** Every Cloudflare change is a plan: `plan_change` → show the
   user the steps and warnings → `apply_plan` with the plan's `planId` and `fingerprint`.
   Never apply a plan the user hasn't seen.
2. **Approval.** When a tool answers `needsApproval`, show the user its message, and call
   the tool again with `confirmed: true` **only after they agree**. Never pass
   `confirmed: true` on your own. A plan with `requiresConfirmation` replaces or deletes DNS
   records Teitunnel didn't create: say so explicitly.
3. **Stale plans.** If `apply_plan` answers `stale`, something changed in Cloudflare: show
   the `newPlan` and apply that one after the user agrees.
4. **Secrets.** Never ask the user to paste API tokens, tunnel tokens or cookies. Account
   problems are fixed in the Teitunnel app. Masked values (`[masked]`, `[redacted]`) stay
   masked.
5. **Temporary vs permanent.** `share_port` shares end when the session ends; use
   `plan_change` `addRoute` for something that should stay online.
6. Changes are recorded in Teitunnel's Activity under the client's name; `undo_last` plans
   the reverse of the latest one.

## Recipes

**Share a dev server**
1. `list_local_services` (pick the dev server; ask if several fit), or use the port given.
2. `share_port {target}` for a public `trycloudflare.com` link, or
   `share_port {target, hostname, allow}` for the user's domain with a login.
3. Give the user the URL. If the page says the host isn't allowed (Vite, webpack, Next.js),
   suggest adding the hostname to the dev server's allowed hosts, or set `httpHostHeader:
   "localhost"` with a `plan_change` `updateRoute` for a route.

**Debug a webhook**
1. `list_shares` (reuse a share of the port) or `share_port`.
2. Give the user the URL to set at the provider and ask them to send a test event.
3. `wait_for_request {pathContains, method: "POST", timeoutSeconds: 300}`; keep its `sinceMs`
   to wait again without missing a delivery.
4. Read the request and response (or `traffic_get`), find the bug, fix the code.
5. `traffic_replay {id}` until it answers 2xx.

**A route is down**
1. `verify_route {hostname}`: where it breaks (DNS, edge, tunnel, origin).
2. `doctor`; `connector_status` when every route of a tunnel is down; `logs_tail
   {hostname}` for 502/504.
3. Fix with `fix_issue {issueId}`, a plan, or tell the user what to do (start the dev server,
   open Teitunnel, turn on Always-on).
4. `verify_route` again.

**Test how an app copes**
1. Share it (`share_port`) or reuse a share; note its URL.
2. `configure_inspection {scope: url, ...}`, one thing at a time, then use or call the app and
   watch `traffic_list`/`traffic_stats`: `faults: [{path, percent, kind: "status", status: 503}]`,
   `kind: "timeout"`/`"reset"`, `network: "3g"`, `stubs: [{path, status, body, when: "always"}]`.
3. Report what broke and suggest fixes; put it back (`faults: []`, `stubs: []`, `network: "off"`).

**Put a local MCP server online**: `expose_mcp_server {origin, hostname}`. Claude Code, Cursor
and VS Code use the returned configuration (the user gets the token with `teitunnel token
<hostname>`); claude.ai and ChatGPT add the URL as a connector and sign in, which the user
approves in Teitunnel.

**Move routes to a server**: `list_routes`, then `export_config {format: "dockerCompose"}`;
explain that the token is set on the server (`TUNNEL_TOKEN`), never pasted into chat.

## Tools

Read: `list_routes`, `list_domains`, `list_tunnels`, `list_shares`,
`list_local_services`, `plan_change`, `verify_route`, `undo_last`, `doctor`, `logs_tail`,
`remote_logs`, `connector_status`, `export_config`, `import_scan`, `accounts`,
`recent_activity`, `traffic_list`, `traffic_get`, `traffic_stats`, `traffic_export`,
`wait_for_request`, `inspection_settings`.
Change (need approval in `ask` mode, absent in `read-only`): `share_port`, `stop_share`,
`apply_plan`, `fix_issue`, `traffic_replay`, `configure_inspection`, `expose_mcp_server`.

Docs: https://teitunnel.teispace.com/docs/guides/ai-agents/
