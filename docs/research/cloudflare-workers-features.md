# Research: Workers features on the user's account (comments, offline page, webhook inbox)

Facts the comments overlay (Snapshots), the offline page and the webhook inbox (M12-06, M12-12) rely on. Checked **2026-09-25** against developers.cloudflare.com. Snapshot upload facts are in [cloudflare-snapshots.md](cloudflare-snapshots.md). Re-verify before changing the Worker or storage code.

## What the Free plan gets

| Product | Free plan | Limits that matter here | Source |
|---|---|---|---|
| Workers | Yes | 100,000 requests/day per account (resets 00:00 UTC; over it: error 1027, or the Worker is skipped when the route "fails open"); 10 ms CPU per HTTP request; 50 subrequests per request; 128 MB memory; 5 cron triggers per account | [limits](https://developers.cloudflare.com/workers/platform/limits/) |
| D1 | Yes | 10 databases, 500 MB each, 5 GB per account; 5 million rows read and 100,000 rows written per day ("D1 API will return errors … daily limits have been exceeded"); 50 queries per Worker invocation; 100 bound parameters per query; 100 KB per statement; 2 MB per row | [limits](https://developers.cloudflare.com/d1/platform/limits/) · [pricing](https://developers.cloudflare.com/d1/platform/pricing/) |
| Durable Objects | Yes, **SQLite-backed only** | 100,000 requests/day, 13,000 GB-s/day, 5 M rows read, 100,000 rows written, 5 GB | [pricing](https://developers.cloudflare.com/durable-objects/platform/pricing/) |
| KV | Yes | 100,000 reads/day, **1,000 writes/day**, 1 write per second per key, 1 GB | [limits](https://developers.cloudflare.com/kv/platform/limits/) |
| Queues | Yes | 10,000 operations/day; retention **24 hours, not configurable** | [pricing](https://developers.cloudflare.com/queues/platform/pricing/) |

### Choice of storage: D1

- **KV** is out: 1,000 writes a day is a few busy review sessions, and it's eventually consistent (a reviewer wouldn't see their own comment on reload everywhere).
- **Queues** is out for the inbox: 24-hour retention means a laptop closed over a weekend loses webhooks; the free 10,000 operations are 3 per message (write, read, delete).
- **Durable Objects** would work on Free, but their migrations moved to a newer "exports" model in the script upload API that the reference page doesn't fully document yet, and reading them from the app needs a Worker endpoint with its own secret.
- **D1** is a plain resource with a documented create/query/delete API, a documented binding shape, strong consistency, SQL for ordering and retention, and generous free rows. The app reads it with the user's own API token through the query endpoint, so neither Worker exposes an owner API or holds an owner key. **Decision: one D1 database per account, `teitunnel`, shared by snapshot comments and webhook inboxes.**

## D1 API (checked 2026-09-25)

- Create: `POST /accounts/{account_id}/d1/database` `{"name": "…", "primary_location_hint"?: "wnam|enam|weur|eeur|apac|oc", "jurisdiction"?: "eu|us|fedramp"}` → `{uuid, name, created_at, file_size, …}`. Permission **D1 Write**. [source](https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/create/)
- List: `GET /accounts/{account_id}/d1/database?name=…` (D1 Read).
- Delete: `DELETE /accounts/{account_id}/d1/database/{database_id}` (D1 Write). The reference doesn't say it's reversible; Teitunnel treats it as not. [source](https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/delete/)
- Query: `POST /accounts/{account_id}/d1/database/{database_id}/query` with `{"sql": "…", "params": [...]}` → `result: [{results: [...], success, meta: {changes, last_row_id, rows_read, rows_written, duration}}]`. Needs D1 Read or D1 Write. [source](https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/query/)
- From a Worker: `env.DB.prepare(sql).bind(...).run() / .all() / .first()`, `env.DB.batch([...])` (a transaction: one failure rolls back the batch). [source](https://developers.cloudflare.com/d1/worker-api/d1-database/)

## Script upload metadata: bindings (checked 2026-09-25)

`PUT /accounts/{id}/workers/scripts/{name}` (multipart, `metadata` part) accepts `bindings`:

```json
{"type": "d1", "name": "DB", "id": "<D1 uuid>"}
{"type": "plain_text", "name": "NAME", "text": "value"}
{"type": "secret_text", "name": "NAME", "text": "secret"}
{"type": "kv_namespace", "name": "KV", "namespace_id": "…"}
{"type": "queue", "name": "Q", "queue_name": "…"}
{"type": "durable_object_namespace", "name": "DO", "class_name": "…"}
```

Sources: https://developers.cloudflare.com/workers/configuration/multipart-upload-metadata/ · https://developers.cloudflare.com/api/resources/workers/subresources/scripts/methods/update/

Cron triggers: `PUT /accounts/{id}/workers/scripts/{name}/schedules` with `[{"cron": "…"}]` (Workers Scripts Write); **5 per account on Free**. Teitunnel doesn't use them: retention is enforced on each write instead, so no cron slot is taken. [source](https://developers.cloudflare.com/api/resources/workers/subresources/scripts/subresources/schedules/methods/update/)

## Worker routes in front of a tunnel hostname (offline page, inbox)

- A route runs a Worker for requests matching a pattern on a **proxied** hostname; "Calling `fetch()` on the incoming `Request` object will trigger a subrequest to your application server, as defined in the DNS settings of your Cloudflare zone": for a tunnel route, that's the tunnel. [source](https://developers.cloudflare.com/workers/configuration/routing/routes/)
- Patterns: `hostname/*`, `hostname/webhooks/*`; the most specific pattern wins.
- API: `POST /zones/{zone_id}/workers/routes` `{"pattern", "script"}` → `{id, pattern, script}`; `GET` lists; `PUT …/{route_id}`; `DELETE …/{route_id}`. Permission **Workers Routes Write** on the zone (template key `workers_routes`, already asked for Snapshot custom domains). [source](https://developers.cloudflare.com/api/resources/workers/subresources/routes/methods/create/)
- **Fail open:** a route can "fail open" (when the daily limit is used up, "requests behave as if no Worker is configured") or "fail closed" (error 1027). The field is `request_limit_fail_open` on the routes API; Cloudflare's 2025-04-15 changelog says the routes API now returns it correctly, but the create reference doesn't list it. Teitunnel sends `"request_limit_fail_open": true` on create. **Verified 2026-09-25** on a live Free-plan account with an OAuth token: the route was created and the dashboard's "Request limit failure mode" showed "Fail open (proceed)". Sources: [limits](https://developers.cloudflare.com/workers/platform/limits/) · [changelog](https://developers.cloudflare.com/changelog/post/2025-04-15-workers-api-fixes/)
- **Custom Domains can't be used:** "You cannot create a Custom Domain on a hostname with an existing CNAME DNS record" (the tunnel's), and a Custom Domain replaces the origin. Routes keep the tunnel as the origin.
- **Cost:** every request to a hostname with a route Worker runs the Worker and counts against the 100,000/day free requests, even when the computer is on. With fail-open the site keeps working normally past the limit (only the offline page is lost for the rest of the day). This is why the offline page and the inbox are **opt-in per route**, with the trade-off shown.
- WebSockets and streamed bodies pass through `fetch(request)` unchanged; CPU time is only what the script spends, not the wait for the origin.

## Error 1033

"Error 1033: Cloudflare Tunnel error … your tunnel is not connected to Cloudflare's network because Cloudflare's network cannot find a healthy `cloudflared` instance." The docs don't name the HTTP status; it is served with **HTTP 530** (observed widely, e.g. community reports titled "Error 1033/530"). The offline Worker treats a 530 from the origin subrequest as "this computer is off"; 502 from cloudflared (the local service refused) is shown as "the app isn't running" only when the person opts in. Sources: https://developers.cloudflare.com/support/troubleshooting/http-status-codes/cloudflare-1xxx-errors/error-1033/ · https://community.cloudflare.com/t/error-1033-530-on-zone-apex-www-only-reproducible-on-two-independent-tunnels-fr/959370

## Access identity

When a hostname is behind Cloudflare Access and reached through a tunnel, Access adds `Cf-Access-Authenticated-User-Email` and `Cf-Access-Jwt-Assertion`; "Validation of the header alone is not sufficient … unless your application connects through Cloudflare Tunnel". Teitunnel trusts the email header only for shares, routes and Snapshots that have Teitunnel's own Access login (otherwise a visitor could send it). [source](https://developers.cloudflare.com/cloudflare-one/identity/authorization-cookie/application-token/)

## Permissions

| Need | API token permission | Template key | Status |
|---|---|---|---|
| Create/delete the D1 database, read/answer comments, drain the inbox | Account › D1 › Edit ("D1 Write") | `d1` / `edit` | **Unverified key** (permission name verified on the permissions page, 2026-09-25) |
| Worker routes on a zone | Zone › Workers Routes › Edit | `workers_routes` / `edit` | As for Snapshots |
| Upload the Workers | Account › Workers Scripts › Edit | `workers_scripts` / `edit` | As for Snapshots |

Source: https://developers.cloudflare.com/fundamentals/api/reference/permissions/ (D1 Read/Write, Workers Routes Read/Write, Workers Scripts Read/Write)
