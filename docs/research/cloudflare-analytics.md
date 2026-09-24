# Cloudflare GraphQL Analytics (verified 2026-09-24)

Facts M12-05 (analytics, uptime, alerts) relies on. Re-verify before changing the queries.

## Endpoint and errors
- `POST https://api.cloudflare.com/client/v4/graphql` with `{"query": …}`. The schema has a
  single root, `viewer`, with `zones(filter: {zoneTag | zoneTag_in}, limit)` and
  `accounts(filter: {accountTag})`. No mutations.
- Errors come as `{"data": …, "errors": [{"message", "path", "extensions": {"timestamp"}}]}`,
  usually with **HTTP 200**, so the body must always be checked. Documented messages:
  `"rate limiter budget depleted, try again after 5 minutes"` (429),
  `"in combination, your request queries too many nodes, zones and accounts"`,
  `"query time range is too large…"`, `"cannot request data older than…"`,
  `"limit must be positive number and not greater than…"`,
  `"number of fields can't be more than…"`, `"zones [...] are not authorized"`,
  `"does not have access to the path…"`, `"not authorized for that account"`,
  `"Unauthorized"`, `"unable to execute query, please try again later"`.
  Source: https://developers.cloudflare.com/analytics/graphql-api/errors/
- Teitunnel maps them onto the REST error shape: 429 for the budget, 403 for permissions,
  503 for "try again", 400 otherwise; an error whose `path` names an optional part's alias
  (`c0_latency`) drops that part and retries instead (`cf_api::analytics`).

## Limits
- **300 GraphQL queries per 5 minutes per user** by default, separate from the REST limit
  of 1,200 per 5 minutes. Source:
  https://developers.cloudflare.com/analytics/graphql-api/limits/ and Cloudflare's own
  agent skill reference (github.com/cloudflare/skills, `references/graphql-api/gotchas.md`).
  Teitunnel keeps 250/5 min per client and 200/5 min per account in the analytics cache.
- A zone-scoped query covers **up to 10 zones**; an account-scoped one exactly 1 account.
- Per-plan limits come from the `settings` node, not the docs:
  `viewer { zones(filter:{zoneTag_in:[…]}) { settings { httpRequestsAdaptiveGroups {
  enabled maxDuration maxNumberOfFields maxPageSize notOlderThan availableFields } } } }`
  (`maxDuration`: widest range in one selection, seconds; `notOlderThan`: history,
  seconds; `maxPageSize`: largest `limit`; `availableFields`: what the requester may use).
  Source: https://developers.cloudflare.com/analytics/graphql-api/features/discovery/settings/
  Teitunnel reads it per zone (cached 6 h), splits a wider range into aliased chunks
  (at most 8 in one request) and starts at the plan's history limit when the range is
  longer ("available from").

## Dataset and fields
- Zone dataset **`httpRequestsAdaptiveGroups`** (adaptive sampling; numbers are estimates at
  high volume). Hourly/daily rollups (`httpRequests1hGroups`, `httpRequests1dGroups`) have no
  hostname dimension, so per-hostname numbers must use the adaptive dataset.
- Dimensions used (from the published schema, pages.johnspurlock.com/graphql-schema-docs,
  checked 2026-09-24): `datetimeMinute`, `datetimeFiveMinutes`, `datetimeFifteenMinutes`,
  `datetimeHour`, `date`, `clientRequestHTTPHost`, `clientRequestPath`,
  `edgeResponseStatus`, `clientCountryName`, `userAgentBrowser`, `verifiedBotCategory`
  (empty for everything that isn't a verified bot), `cacheStatus`. `botScore` exists but
  needs Bot Management.
- `count`, `sum { edgeResponseBytes visits … }`, `ratio { status4xx status5xx }`, and
  `quantiles { originResponseDurationMsP50/P95/P99, edgeTimeToFirstByteMsP50/P95/P99, … }`.
  Timing quantiles are for **Pro, Business and Enterprise** ("Introducing Timing
  Insights", https://blog.cloudflare.com/introducing-timing-insights/).
- Filters: `datetime_geq`/`datetime_lt` (always required), `clientRequestHTTPHost_in`,
  `clientRequestPath_like` (`%` wildcard), `edgeResponseStatus_geq`/`_lt`; ordering with
  `orderBy: [count_DESC]` or `[datetimeMinute_ASC]`. Aliases let one request hold several
  selections of the same dataset. Source:
  https://developers.cloudflare.com/analytics/graphql-api/tutorials/end-customer-analytics/
- There is also an account-scoped `httpRequestsAdaptiveGroups` (under `accounts`); not
  used yet.

## Permissions
- API token: zone-scoped datasets need **Zone · Analytics · Read**; account-scoped ones
  need **Account · Account Analytics · Read** (Cloudflare's docs page shows the latter as
  "Account ▸ Account Analytics ▸ Read").
  Sources: https://developers.cloudflare.com/analytics/graphql-api/getting-started/authentication/api-token-auth/ ·
  https://developers.cloudflare.com/fundamentals/api/reference/permissions/ (names
  "Analytics Read", scope `com.cloudflare.api.account.zone`; "Account Analytics Read",
  scope `com.cloudflare.api.account`).
- Token template keys (`permissionGroupKeys`): **`analytics`** (Zone analytics) and
  **`account_analytics`** (Account analytics), type `read`. Source:
  https://developers.cloudflare.com/fundamentals/api/how-to/account-owned-token-template/
- Probe: the zone's `settings` query above; "not authorized" → no permission.
- OAuth: scope ids are `<permission-group>.<level>` in Cloudflare's examples
  (`zone.read`, `workers-scripts.write`, `workers-kv-storage.write`); optional scopes are
  marked in the client's dashboard settings and users may untick them on the consent
  screen (https://blog.cloudflare.com/task-based-oauth-consent/,
  https://developers.cloudflare.com/changelog/post/2026-08-20-oauth-optional-scopes/).
  **Not verified:** the analytics scope ids. Teitunnel requests `analytics.read` and
  `account-analytics.read` as optional; confirm them (with the other scope ids, which use
  an older `a:b` form in `oauth.rs`) against `GET /client/v4/oauth/scopes` when the
  OAuth client is registered.
