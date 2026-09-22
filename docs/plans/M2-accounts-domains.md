# M2: Accounts & Domains

**Goal:** connect a Cloudflare account in under 30 seconds, securely, and see your domains with clear status and permissions.
**Release:** v0.2.0.
**Exit criteria:**
- OAuth sign-in works end to end (if the client is approved; otherwise the token flow is the default and OAuth ships behind a flag).
- The token flow opens a pre-filled template, then verifies and stores the token in the keychain.
- cert.pem import works and is labelled as limited.
- Multiple accounts work. Sign-out removes every secret for that account (verified).
- The capability map drives disabled states with reasons.

**Maintainer prerequisites (not code):** verify teispace.com in the Cloudflare OAuth client settings, create a **public** OAuth client (PKCE, auth method `none`), register redirect URIs `http://127.0.0.1:{53682,53683,53684}/callback`, and record the client id in `crates/core/src/auth/oauth_client.rs`. A client id isn't a secret.

---

### M2-01 · `cf-api` foundation
- [ ] `Client { base_url, auth: Auth, http: reqwest::Client }`. `Auth::Bearer(Secret)`. The User-Agent is `Teitunnel/<ver>`.
- [ ] Envelope decoding (`success`, `errors[{code,message}]`, `result`, `result_info`) into a typed `ApiError { status, codes, messages }`.
- [ ] Pagination helper (`page`/`per_page` and cursor variants) as a `Stream`.
- [ ] Retries: 429 honours `Retry-After`; 502/503/504/network errors use exponential backoff (max 3); 4xx is never retried. A global rate limiter (token bucket, 1200 req / 5 min per credential).
- [ ] Timeouts: connect 5 s, request 20 s.
- [ ] Tests: wiremock for envelope errors, pagination, retry/backoff (with a paused clock).

### M2-02 · Accounts, zones, token verify
- [ ] `user_tokens_verify`, `accounts_list`, `zones_list(account)` (status, name_servers, original NS, plan), `zone_get`.
- [ ] `oauth_userinfo` for display name/email.
- [ ] Record the exact response shapes as fixtures from a real account (secrets scrubbed).

### M2-03 · Capability probing
- [ ] `core::auth::capabilities(account) -> Capabilities { tunnels_read/edit, dns_read/edit per zone, zones_read, access_edit, ... }`.
- [ ] Strategy: known OAuth scopes when present; for API tokens, try cheap read endpoints and interpret 403 / code 10000 per resource. Cache per session and refresh on "Re-check".
- [ ] Map missing capability → `DisabledReason` + a "Fix permissions" action (opens the template URL or OAuth re-consent with optional scopes).

### M2-04 · OAuth (PKCE, loopback)
- [ ] `core::auth::oauth`: PKCE verifier (64 random chars) + S256 challenge + random `state`. Bind the first free port from the registered set on `127.0.0.1` only. Open the browser with the authorize URL (scopes from research doc).
- [ ] Callback server: accepts exactly one request to `/callback`, validates `state`, returns a small styled "You can return to Teitunnel" page (static HTML with inline CSS; no JS). Times out after 5 minutes. Cancellable from the UI.
- [ ] Code exchange at `/oauth2/token`. The refresh token goes in the keychain; the access token and expiry stay in memory. Proactive refresh at 80% of lifetime, with a single-flight refresh lock.
- [ ] Revocation on sign-out (`/oauth2/revoke`).
- [ ] Focus returns to the app window after callback.
- [ ] Tests: full flow against a wiremock "authorization server"; state mismatch, timeout, port-in-use fallback, refresh race.

### M2-05 · API token flow
- [ ] Template URL builder (research doc keys; unit-tested encoding).
- [ ] Paste field (secure input, never echoed), then verify, list accounts reachable by the token (a token may cover several), and save in the keychain.

### M2-06 · cert.pem import
- [ ] Detect `~/.cloudflared/cert.pem`. Parse the `ARGO TUNNEL TOKEN` block into `{zoneID, accountID, apiToken}`, and copy the token into the keychain (the original file is left untouched).
- [ ] Mark the account `limited(zone)`. The UI explains that it only works for one domain, and offers an upgrade to OAuth/token.

### M2-07 · Account store & secrets
- [ ] `accounts` table migration. Keychain entries per ARCHITECTURE §6. `SecretStore` port (+ in-memory fake).
- [ ] Commands: `accounts_list`, `accounts_add_oauth_start/cancel`, `accounts_add_token`, `accounts_import_cert`, `accounts_remove`, `accounts_set_active`.
- [ ] Removal deletes every keychain item for the account (test with the fake store + a macOS integration test behind a feature flag).

### M2-08 · UI: connect & accounts
- [ ] Onboarding step 2: "Connect Cloudflare", with a primary **Sign in with Cloudflare** button, then "Use an API token" and "Import from cloudflared login" as secondary links. "Skip, just Quick Share" is also available.
- [ ] Waiting-for-browser state (animated status, Cancel, "Copy link" if the browser didn't open).
- [ ] Sidebar account switcher (popover with accounts, add, manage).
- [ ] Settings → Accounts: list, credential type, capabilities (a checklist with reasons), Re-check, Sign out.

### M2-09 · Domains view
- [ ] Zones list: name, status (active / pending nameservers / moved), plan, and route count (populated from M3).
- [ ] Inspector: status detail. For pending zones, show the required nameservers with copy buttons and a "Check again" action. Show existing tunnel CNAMEs in the zone (read-only until M3/M4).
- [ ] Search/filter for accounts with many zones (virtualized).
