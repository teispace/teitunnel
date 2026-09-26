# Research: edge rules and service tokens (M12-04)

Facts the edge protection feature relies on: per-hostname bot and AI-crawler rules, rate limiting, header rules and Access service tokens. Checked 2026-09-24 against developers.cloudflare.com and the `cloudflare/cloudflare-docs` repository; re-verify before changing `crates/cf-api/src/rulesets.rs`, `service_tokens.rs` or the planner's limits table.

## Rulesets API (checked 2026-09-24, pages last updated 2026-04-16)

Every zone-level rule Teitunnel writes lives in a phase's **entry point ruleset**:

| What | Endpoint |
|---|---|
| Read a phase's entry point | `GET /zones/{zone_id}/rulesets/phases/{phase}/entrypoint` (404 when the zone has none yet) |
| Create the entry point with our first rule | `POST /zones/{zone_id}/rulesets` with `{"name", "kind": "zone", "phase", "rules": [...]}` |
| Add one rule | `POST /zones/{zone_id}/rulesets/{ruleset_id}/rules` (appended at the end unless `position` is given: `{"before": id}`, `{"after": id}` or `{"index": n}`, 1-based) |
| Change one rule | `PATCH /zones/{zone_id}/rulesets/{ruleset_id}/rules/{rule_id}`: the **whole rule definition** must be sent, even unchanged fields |
| Delete one rule | `DELETE /zones/{zone_id}/rulesets/{ruleset_id}/rules/{rule_id}` |

All three rule endpoints answer with the **complete ruleset** after the change. `PUT …/phases/{phase}/entrypoint` replaces every rule of the phase, so Teitunnel never uses it: other rules must stay untouched. Cloudflare "automatically creates the entry point ruleset when you add a rule to it"; Teitunnel creates it explicitly with `POST /zones/{zone_id}/rulesets` only when the read said 404.

Sources: https://developers.cloudflare.com/ruleset-engine/rulesets-api/view/ · https://developers.cloudflare.com/ruleset-engine/rulesets-api/add-rule/ · https://developers.cloudflare.com/ruleset-engine/rulesets-api/update-rule/ · https://developers.cloudflare.com/ruleset-engine/rulesets-api/delete-rule/ · https://developers.cloudflare.com/ruleset-engine/rulesets-api/create/

## Phases

| Feature | Phase | Rule shape |
|---|---|---|
| Custom rules (block, challenge) | `http_request_firewall_custom` | `{"action": "block" \| "managed_challenge", "expression", "description", "enabled"}` |
| Rate limiting | `http_ratelimit` | `action` plus `"ratelimit": {"characteristics": ["cf.colo.id", "ip.src"], "period", "requests_per_period", "mitigation_timeout"}`; rate limiting rules must be at the end of the phase's list |
| Request header rules | `http_request_late_transform` | `{"action": "rewrite", "action_parameters": {"headers": {"X-Name": {"operation": "set", "value": "v"}, "X-Other": {"operation": "remove"}}}}` |
| Response header rules | `http_response_headers_transform` | the same, with operations `set`, `add`, `remove` |
| (URL rewrites, read only to count Transform Rules) | `http_request_transform` | |

Request header rules can't touch `cf-*`/`x-cf-*` headers (except removing `cf-connecting-ip`), forbidden names (e.g. `Accept-Encoding`), `x-forwarded-for`, `true-client-ip`, `x-real-ip`, `x-forwarded-proto`; `cookie` can only be removed (request header page, updated 2026-09-04). Teitunnel checks these before planning.

Sources: https://developers.cloudflare.com/waf/custom-rules/ · https://developers.cloudflare.com/waf/rate-limiting-rules/create-api/ · https://developers.cloudflare.com/rules/transform/request-header-modification/ · https://developers.cloudflare.com/rules/transform/request-header-modification/create-api/

## Plan limits (checked 2026-09-24)

| | Free | Pro | Business | Enterprise |
|---|---|---|---|---|
| Custom rules | 5 | 20 | 100 | 1,000 |
| Regex in expressions | no | no | yes | yes |
| Rate limiting rules | **1** | 2 | 5 | 100 |
| Rate limiting: fields in the rule expression | **Path, Verified Bot** | Host, URI, Path, Full URI, Query, Verified Bot | + Method, Source IP, User Agent | all |
| Rate limiting: counting characteristics | IP | IP | IP, IP with NAT | … |
| Rate limiting: counting period | 10 s | up to 1 min | up to 10 min | up to 65,535 s |
| Rate limiting: mitigation timeout | 10 s | up to 1 h | up to 1 day | up to 1 day |
| Transform Rules (all types combined) | 10 | 25 | 50 | 300 |

**Consequence:** a Free zone's single rate limiting rule **can't match a hostname** (`http.host` isn't among the Free fields; it arrives with Pro). A Free rate limit would apply to the whole domain, which Teitunnel never does, so on Free the plan refuses rate limiting with an explanation. From Pro on, Teitunnel owns one rule per zone and parameter set whose expression is `(http.host in {"a.example.com" "b.example.com"})`, merging every hostname it protects with the same limit; when the zone's quota has no room for another parameter set, the plan explains the conflict.

Supported rate-limit periods and timeouts are the documented list `10, 60, 120, 300, 600, 3600` (plus longer on Enterprise); Teitunnel uses the period as the mitigation timeout.

Sources: https://developers.cloudflare.com/waf/custom-rules/ (updated 2026-08-25) · https://developers.cloudflare.com/waf/rate-limiting-rules/ (updated 2026-08-25) · https://github.com/cloudflare/cloudflare-docs/blob/production/src/content/partials/waf/rate-limiting-availability-by-plan.mdx · https://developers.cloudflare.com/rules/transform/ (updated 2026-08-14)

### Reading the limits

There's no documented entitlements endpoint for these quotas. The zone object's `plan.legacy_id` (`free`, `pro`, `business`, `enterprise`) from `GET /zones/{zone_id}` selects a row of the table above; an unknown plan is treated as Free (the smallest limits). Usage is counted from the entry point rulesets (every rule, Teitunnel's and others'). Cloudflare stays the final judge: a refused write fails the step and the plan rolls back.

Source: https://developers.cloudflare.com/api/resources/zones/methods/get/ (`plan.legacy_id`, "free" in the example).

## Bots and AI crawlers on every plan

- **Bot Fight Mode** and **Block AI bots** are zone-wide switches ("Block (on all pages) – issues the block across the entire zone", updated 2026-07-01). Teitunnel doesn't offer them.
- `cf.client.bot` (boolean, a known good bot, the same as `cf.bot_management.verified_bot`) is usable on Free (Cloudflare's "Stop malicious bots (Free, Pro, and Business)" guide uses it, updated 2026-08-25).
- `cf.verified_bot_category` values include `"AI Crawler"`, `"AI Assistant"`, `"AI Search"` (verified bot categories page, updated 2026-07-01). The docs don't restrict the field by plan; community guides use `cf.verified_bot_category eq "AI Crawler"` on Free. **Unconfirmed for Free by Cloudflare's docs**: if Cloudflare refuses it, the step fails and rolls back with Cloudflare's message.
- Bot score (`cf.bot_management.score`) is Enterprise Bot Management only, so "bots" on Free/Pro means: not a verified bot and a user agent of an automated client (empty, `curl`, `wget`, `python-requests`, `Go-http-client`, `HeadlessChrome`, `Scrapy`, …), matched with `lower(http.user_agent) contains …` (no regex).

Teitunnel's per-hostname rules therefore are `(http.host eq "<hostname>") and (…)` custom rules: "Challenge" uses `managed_challenge`, "Block" and "Block AI crawlers" use `block`.

Sources: https://developers.cloudflare.com/bots/additional-configurations/block-ai-bots/ · https://developers.cloudflare.com/bots/concepts/bot/verified-bots/categories/ · https://developers.cloudflare.com/use-cases/solutions/stop-malicious-bots/ · https://developers.cloudflare.com/ruleset-engine/rules-language/fields/reference/cf.client.bot/

## Access service tokens (checked 2026-09-24, page updated 2026-09-22)

- Create: `POST /accounts/{account_id}/access/service_tokens` with `{"name", "duration"}` (e.g. `"8760h"`, one year). The answer carries `id`, `client_id`, `client_secret` and `expires_at`; "This is the only time Cloudflare Access will display the Client Secret".
- List: `GET /accounts/{account_id}/access/service_tokens`. Rotate (new secret, same client id): `POST …/service_tokens/{id}/rotate`. Extend by a year: `POST …/{id}/refresh`. Delete: `DELETE …/{id}`.
- Machines send `CF-Access-Client-Id` and `CF-Access-Client-Secret` headers.
- A policy accepting a token must use the **Service Auth** action, `"decision": "non_identity"` in the API (an `allow` policy would still send the caller to a login page), with an include rule `{"service_token": {"token_id": "<id>"}}` (the token's `id`, not its client id) or `{"any_valid_service_token": {}}`. Teitunnel adds a reusable policy named `Teitunnel · <domain> · Machines` to the route's Access application (D-095 style).
- Limits: 50 service tokens per account, 500 applications, 500 reusable policies; the Cloudflare One limits page lists no different numbers for Free.

Sources: https://developers.cloudflare.com/cloudflare-one/access-controls/service-credentials/service-tokens/ · https://developers.cloudflare.com/cloudflare-one/policies/access/#actions · https://developers.cloudflare.com/cloudflare-one/account-limits/ · https://github.com/izzywdev/FuzeInfra/pull/1039 (non_identity for service-token policies)

## Permissions

| Feature | Permission (dashboard name) | Template key | OAuth scope (`<group>.<level>`) |
|---|---|---|---|
| Custom and rate limiting rules | Zone › Zone WAF › Edit ("Zone WAF Write" is what the custom rules API page asks for) | `zone_waf` (verified 2026-09-25) | `zone-waf.write` (confirmed 2026-09-25) |
| (older firewall features) | Zone › Firewall Services › Edit | `firewall_services` (documented) | `firewall-services.write` **unconfirmed** |
| Header rules | Zone › Transform Rules › Edit | `zone_transform_rules` (verified 2026-09-25; `transform_rules` is Account › Transform Rules) | `zone-transform-rules.write` (confirmed 2026-09-25) |
| Service tokens | Account › Access: Service Tokens › Edit | `access_service_token` (verified 2026-09-25; the plural is dropped) | `access-service-token.write` (confirmed 2026-09-25) |
| Zone plan | Zone › Zone › Read (already in the template) | `zone` | |

The documented template keys (`account-owned-token-template.mdx`) include `firewall_services` but no key for Zone WAF, Transform Rules or Access service tokens. Unknown keys are dropped silently from the pre-filled link, so capability probes (`GET …/phases/http_request_firewall_custom/entrypoint`, `GET …/phases/http_request_late_transform/entrypoint` and `GET /accounts/{id}/access/service_tokens`, 403 = missing) and the in-place fix name the dashboard permission to add. The keys were checked on 2026-09-25 by opening the pre-filled link signed in to the dashboard and reading which rows it filled (`d1` gives Account › D1 › Edit too). The OAuth scope names were read from the Teitunnel OAuth client's scope list after adding them as optional scopes on 2026-09-25. Access apps and policies at the account level are `access-app.write` and `access-policy.write`; the dashboard's "Access: Apps and Policies" scope is `zone-access.write` (zone-level apps only).

Sources: https://developers.cloudflare.com/fundamentals/api/reference/permissions/ (updated 2026-09-16) · https://github.com/cloudflare/cloudflare-docs/blob/production/src/content/docs/fundamentals/api/how-to/account-owned-token-template.mdx · https://developers.cloudflare.com/waf/custom-rules/create-api/ · https://developers.cloudflare.com/ruleset-engine/rulesets-api/delete-rule/

## Cache Rules (checked 2026-09-26, for "bypass cache for dev", M12-08)
Sources: developers.cloudflare.com/cache/how-to/cache-rules/create-api/ and
/cache/how-to/cache-rules/ and /fundamentals/api/reference/permissions/ (2026-09-26).
- Phase `http_request_cache_settings`; action `set_cache_settings`; bypass is
  `"action_parameters": { "cache": false }`, e.g. expression `(http.host eq "app.example.com")`.
- Rules per zone: Free 10, Pro 25, Business 50, Enterprise 300.
- Token permission: "Cache Rules Edit" (Zone; also listed as "Cache Settings Write"), read
  with "Cache Rules Read". **Not verified:** the token template's short key and the OAuth
  scope name (not in the docs' tables; the others were checked against the dashboard's
  pre-filled form on the maintainer's account). Reading this phase must stay optional:
  existing tokens lack the permission, and a failed read must not break the other rules.
