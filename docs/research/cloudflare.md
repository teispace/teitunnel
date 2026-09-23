# Research: Cloudflare & cloudflared facts

Verified facts the implementation relies on. Each entry has its source and the date it was checked. Re-verify before implementing the related milestone, and update the date.

## cloudflared binary

| Fact | Value | Source (checked 2026-09-22) |
|---|---|---|
| Latest release | 2026.9.1 (2026-09-11) | github.com/cloudflare/cloudflared/releases/latest |
| Latest download URL | `https://github.com/cloudflare/cloudflared/releases/latest/download/<asset>` → redirects to `/download/<version>/<asset>` | curl -I |
| macOS assets | `cloudflared-darwin-arm64.tgz`, `cloudflared-darwin-amd64.tgz` (+ `.pkg`) | release page |
| Linux assets | `cloudflared-linux-amd64`, `cloudflared-linux-arm64` (+ `.deb`, `.rpm`) | release page |
| Windows assets | `cloudflared-windows-amd64.exe` (+ `.msi`, 386) | release page |
| Checksums | In the release body, lines like `cloudflared-amd64.pkg: <sha256>`. **For `.tgz` assets the listed hash is of the extracted `cloudflared` binary, not the archive** (verified 2026-09-23: body `9a0b19f6…` = `shasum` of the binary; the tgz itself is `c27ab8fd…`) | release page + local check |
| Archive digest | The GitHub API asset object has `digest: "sha256:<hash of the asset file>"` | `GET /repos/cloudflare/cloudflared/releases/latest` (2026-09-23) |
| macOS signing | Official binaries are signed "Developer ID Application: Cloudflare Inc. (**68WVV388M8**)" and pass `codesign --verify --strict`. Homebrew's build is ad-hoc signed (no Team ID), so the Team ID check applies to managed downloads only | `codesign -dv --verbose=2` (2026-09-23) |
| API rate limit | Unauthenticated: 60 requests/hour per IP; responses carry an `ETag` | response headers (2026-09-23) |

## Run parameters we use

Source: https://developers.cloudflare.com/tunnel/reference/run-parameters/

| Flag | Env | Min version | Use |
|---|---|---|---|
| `--token` | `TUNNEL_TOKEN` | — | Session mode, via env only |
| `--token-file` | `TUNNEL_TOKEN_FILE` | 2025.4.0 | Always-on mode |
| `--output json` | `TUNNEL_LOG_OUTPUT` | 2025.6.1 | Structured logs |
| `--metrics ip:port` | `TUNNEL_METRICS` | — | Our fixed port per connector |
| `--protocol auto\|http2\|quic` | `TUNNEL_TRANSPORT_PROTOCOL` | — | Doctor fix for blocked UDP |
| `--edge-ip-version auto\|4\|6` | `TUNNEL_EDGE_IP_VERSION` | — | Advanced |
| `--loglevel` | `TUNNEL_LOGLEVEL` | — | `info` default |
| `--log-directory` | `TUNNEL_LOGDIRECTORY` | — | Always-on (rotates at 1 MB, keeps 5) |
| `--grace-period` | `TUNNEL_GRACE_PERIOD` | — | Default 30 s |
| `--post-quantum` | `TUNNEL_POST_QUANTUM` | — | Advanced (QUIC only) |
| `--dns-resolver-addrs` | `TUNNEL_DNS_RESOLVER_ADDRS` | 2025.7.0 | Advanced |
| `--no-autoupdate` | — | — | Always. We manage updates. |

## Metrics server endpoints

Source: `metrics/metrics.go` in cloudflare/cloudflared; https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/monitor-tunnels/metrics/

- Default address: first free port in `127.0.0.1:20241–20245` (`0.0.0.0` in containers). We always pass an explicit port.
- `/metrics`: Prometheus text.
- `/healthcheck`: `OK`.
- `/ready`: readiness JSON (includes `readyConnections`).
- `/quicktunnel`: `{"hostname":"…trycloudflare.com"}`.
- `/config`: current versioned config (remote-managed).
- `/diag/*`: diagnostics, used by `cloudflared tunnel diag`.
- `/debug/pprof/cmdline` is deliberately blocked (it would leak tokens).

Metrics worth charting: `cloudflared_tunnel_ha_connections`, `cloudflared_tunnel_total_requests`, `cloudflared_tunnel_request_errors`, `cloudflared_tunnel_concurrent_requests_per_tunnel`, `cloudflared_tunnel_response_by_code`, `quic_client_latest_rtt`, `quic_client_smoothed_rtt`, `cloudflared_tcp_active_sessions`, `cloudflared_udp_active_sessions`, `cloudflared_tunnel_timer_retries`.

## Diagnostics

`cloudflared tunnel diag [--metrics 127.0.0.1:PORT]` (≥ 2024.12.2) writes `cloudflared-diag-<ts>.zip`. Sections can be skipped with `--no-diag-*`.
Source: https://developers.cloudflare.com/cloudflare-one/networks/connectors/cloudflare-tunnel/troubleshoot-tunnels/diag-logs/

## Cloudflare API v4: tunnels

Source: https://developers.cloudflare.com/api/resources/zero_trust/subresources/tunnels/subresources/cloudflared/

| Op | Endpoint |
|---|---|
| List | `GET /accounts/{acc}/cfd_tunnel` (filters: `is_deleted`, `name`, `status`, pagination) |
| Create | `POST /accounts/{acc}/cfd_tunnel` `{name, config_src: "cloudflare", tunnel_secret?}` |
| Get / Patch / Delete | `…/cfd_tunnel/{id}` |
| Get config | `GET …/cfd_tunnel/{id}/configurations` (includes `version`) |
| Put config | `PUT …/cfd_tunnel/{id}/configurations` `{config: {ingress: [...], originRequest: {...}}}` |
| Connections | `GET …/cfd_tunnel/{id}/connections`; clean stale: `DELETE …/connections` |
| Connector | `GET …/cfd_tunnel/{id}/connectors/{connector_id}` |
| Run token | `GET …/cfd_tunnel/{id}/token` |
| Management token (remote logs) | `POST …/cfd_tunnel/{id}/management` |

## DNS

- Tunnel route = proxied CNAME `<host>` → `<tunnel-uuid>.cfargotunnel.com`.
- DNS record `comment` field is available on all plans (used for ownership). `tags` need paid plans, so we don't use them.
- Endpoints: `GET/POST /zones/{zone}/dns_records`, `PATCH/DELETE …/{record}`. Filters: `name`, `type`, `content`, `comment.contains`.
- Error 81053: a record with that host already exists. Error 81057: identical record exists.

## Auth

### OAuth (self-managed clients, GA to all customers June 2026)
Sources: https://developers.cloudflare.com/changelog/post/2026-06-03-public-oauth-clients/ · https://developers.cloudflare.com/fundamentals/oauth/create-an-oauth-client/ · https://www.ubitools.com/cloudflare-oauth-client/ · https://developers.cloudflare.com/changelog/post/2026-08-20-oauth-optional-scopes/

- Only the Authorization Code flow is supported. Public clients use PKCE (S256) with token auth method `none`.
- Endpoints: `https://dash.cloudflare.com/oauth2/auth`, `/oauth2/token`, `/oauth2/revoke`, `/oauth2/userinfo`, `/.well-known/openid-configuration`.
- Redirect URIs: **exact match**. `http://127.0.0.1:<port>/callback` is allowed. **Custom schemes are rejected.** → Register a small fixed port set.
- Public (any-user) clients need **domain verification** (teispace.com).
- Scope names mirror API-token permission names (dot-delimited, e.g. `account.read`). The list is at `GET https://api.cloudflare.com/client/v4/oauth/scopes` (requires auth). **TODO (M2):** fetch it with a maintainer token and record the exact tunnel/DNS/zone/access scope ids here.
- Optional scopes (Aug 2026): users may decline them. Use this for Access features.
- `offline_access` gives a refresh token. **TODO (M2):** record access/refresh token lifetimes.

### API token template URL
Source: https://developers.cloudflare.com/fundamentals/api/reference/template/
- User token: `https://dash.cloudflare.com/profile/api-tokens?permissionGroupKeys=<urlencoded JSON [{"key":"…","type":"edit|read"}]>&accountId=*&zoneId=all&name=Teitunnel`
- Verified 2026-09-23 (cloudflare-docs `fundamentals/api/how-to/account-owned-token-template.mdx` and PR #33557):
  - `permissionGroupKeys` is a URL-encoded JSON array of `{"key": "<short key>", "type": "read" | "edit" | "revoke" | "run" | "purge"}` (user tokens use `edit`, not `write`).
  - **User token URLs** accept short keys only; entries that don't match a permission group are **dropped silently**. Account token URLs (`https://dash.cloudflare.com/?to=/:account/api-tokens&permissionGroupKeys=…&name=…`) also accept permission group IDs and show a notice for unresolved keys.
  - Documented short keys we need: `dns` (DNS records), `zone` (zone management), `account_settings`, `access`, `access_acct`. Example: `[{"key":"dns","type":"edit"}]`.
  - **Unverified:** the short key for *Cloudflare Tunnel* (not in the docs table). Candidate `argotunnel` (the legacy permission name). Because unknown keys are dropped silently, the token screen must tell users to add "Account → Cloudflare Tunnel → Edit" if it isn't pre-selected, and capability probing (M2-03) must catch a missing tunnel permission. **Maintainer:** open the generated URL once and confirm; alternatively use an account token URL with the permission group ID from `GET /accounts/{id}/tokens/permission_groups`.

### cert.pem (from `cloudflared tunnel login`)
- The PEM block `ARGO TUNNEL TOKEN` holds base64 JSON `{zoneID, accountID, apiToken}`. The token is scoped to the zone chosen at login, so it counts as a limited credential.

## Quick Share DNS timing (measured 2026-09-23, cloudflared 2026.9.1)
- `/quicktunnel` returns the hostname ~2 s before the first edge connection registers (`/ready` still reports 0).
- The hostname isn't in public DNS for the first ~2–3 s after it appears: first lookups at +0/+1/+2 s returned NXDOMAIN; +4 s and +7 s resolved (probed once per fresh hostname via `dns.google/resolve`).
- NXDOMAIN answers for `*.trycloudflare.com` carry the zone SOA (minimum 1800), so resolvers may cache the negative answer for **up to 30 minutes**. An early lookup (by us or a browser) breaks the URL for that resolver.
- Consequence (D-037): never query DNS early; show a share as live (and enable Open) only 6 s after the hostname first appears and a connection is registered.

## Request-scoped log fields (verified 2026-09-23)
Source: `proxy/logger.go` and `proxy/proxy.go` in cloudflare/cloudflared (master).
- Every HTTP request's logger carries `originService` (the rule's service string), `ingressRule` (index of the matched rule), `connIndex`, and `cfRay` when present; `lbProbe` for load-balancer probes. TCP streams carry `destAddr` and `flowID`.
- Failed requests are logged at **error** level with those fields (`logRequestError`); successful requests (`logHTTPRequest`, with `host` and `path`) and origin responses only at **debug**.
- So at the default `info` level a route's log shows its failures; a per-route view matches `ingressRule` + `originService` against the applied ingress (the index alone can point at another route in lines logged before a config change).
