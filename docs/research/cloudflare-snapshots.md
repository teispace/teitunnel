# Research: hosting Snapshots on the user's Cloudflare account

Facts the Snapshots feature (M12-06) relies on. Checked 2026-09-24 against developers.cloudflare.com; re-verify before changing the upload or deploy code.

## Choice: Workers Static Assets, not Pages Direct Upload

| | Workers Static Assets | Pages Direct Upload |
|---|---|---|
| API for direct upload | Documented, stable: upload session → buckets → script/version with an assets JWT | Documented for Wrangler; the REST flow is less complete |
| Content addressing | Manifest of path → hash; Cloudflare answers which hashes it still needs (only changed files travel) | Similar, by hash |
| Atomic switch | A version only exists once its asset upload completed; deploying it is one call | Deployment becomes live when finished |
| Versions and rollback | Every upload is a version with its own assets; roll back by deploying an older version (last 100) | Deployment history, rollback per deployment |
| Custom hostname | Custom Domains: Cloudflare creates the DNS record and certificate | Custom domains with a CNAME the user adds |
| Without a domain | `<script>.<subdomain>.workers.dev` | `<project>.pages.dev` |
| Logic in front (password, overlay) | The same Worker (`run_worker_first`) | Pages Functions (a separate model) |
| Cost of plain asset requests | Free and unlimited | Free |

Workers has "a distinctly broader set of features" and is where Cloudflare invests (migration guide). The one Pages advantage (custom domains on zones not on Cloudflare) doesn't apply: Teitunnel only offers the account's own zones. **Decision: Workers Static Assets.**

Sources: https://developers.cloudflare.com/workers/static-assets/direct-upload/ · https://developers.cloudflare.com/workers/static-assets/migration-guides/migrate-from-pages/ · https://developers.cloudflare.com/workers/static-assets/billing-and-limitations/

## Direct upload flow (checked 2026-09-24)

1. `POST /accounts/{account_id}/workers/scripts/{script_name}/assets-upload-session` with `{"manifest": {"/index.html": {"hash": "<32 hex>", "size": 1234}}}`. Result: `{"jwt": "...", "buckets": [["hash", ...], ...]}`. The JWT is valid for one hour; when `buckets` is empty every file is already stored and the JWT is the completion token.
2. For each bucket: `POST /accounts/{account_id}/workers/assets/upload?base64=true`, `multipart/form-data`, `Authorization: Bearer <session JWT>` (not the API token). One part per hash: field name = the hash, body = the file's base64, part `Content-Type` = the file's media type. The response to the last bucket (HTTP 201) carries the completion `jwt`.
3. Deploy: `PUT /accounts/{account_id}/workers/scripts/{script_name}` (multipart: a `metadata` JSON part plus the module file) with `metadata.assets = {"jwt": "<completion token>", "config": {...}}` and a binding `{"type": "assets", "name": "ASSETS"}`. `keep_assets: true` reuses the previous version's assets instead.
4. Hash: 32 hex characters. Cloudflare's example uses `sha256(base64(content) + extension)[0..32]`; the hash is only a content key (Teitunnel uses the same recipe).

Sources: https://developers.cloudflare.com/workers/static-assets/direct-upload/ · https://developers.cloudflare.com/api/resources/workers/subresources/scripts/methods/update/

## Versions, deployments, rollback

- `POST /accounts/{id}/workers/scripts/{name}/versions` uploads a version **without deploying it** (response: `id`, `number`, `metadata`). Metadata accepts `bindings`, `keep_bindings`, `assets`, `annotations`.
- `POST /accounts/{id}/workers/scripts/{name}/deployments` with `{"strategy": "percentage", "versions": [{"version_id": "...", "percentage": 100}]}` makes a version live. `annotations["workers/message"]` up to 1000 bytes.
- `GET …/deployments` lists deployments, newest first (`versions[].version_id`).
- Each version has its own static assets (version affinity exists because version A's HTML references A's hashed files).
- "You can only roll back to the 100 most recently published versions."

Teitunnel therefore publishes an update as *upload version, then deploy it* (atomic switch), and rolls back by deploying an older version id. It keeps the last 10 versions' manifests locally for the version list and change counts.

Sources: https://developers.cloudflare.com/api/resources/workers/subresources/scripts/subresources/versions/methods/create/ · https://developers.cloudflare.com/api/resources/workers/subresources/scripts/subresources/deployments/methods/create/ · https://developers.cloudflare.com/workers/configuration/versions-and-deployments/rollbacks/ · https://developers.cloudflare.com/workers/static-assets/routing/advanced/gradual-rollouts/

## Routing options

- `not_found_handling`: `"single-page-application"` (serve `/index.html` with 200), `"404-page"` (nearest `404.html`), `"none"`.
- `html_handling`: `"auto-trailing-slash"` (default), `"force-trailing-slash"`, `"drop-trailing-slash"`, `"none"`.
- `run_worker_first`: `false` (default, assets served without invoking the script), `true`, or up to 100 route patterns (`!` negates). Requests that run the Worker count against Worker request limits; plain asset requests are free and unlimited.
- With a compatibility date of 2025-04-01 or later, navigation requests (`Sec-Fetch-Mode: navigate`) prefer assets over the script for SPAs.
- `_headers` and `_redirects` are supported by Workers static assets (compatibility matrix); through the API they're sent in `assets.config` (`_headers`, `_redirects` strings), not uploaded as files.

Teitunnel sets `run_worker_first: true` only when a password (or, later, the comments overlay) needs the script; otherwise the script never runs and a Snapshot costs nothing per request.

Sources: https://developers.cloudflare.com/workers/static-assets/binding/ · https://developers.cloudflare.com/workers/static-assets/routing/single-page-application/ · https://developers.cloudflare.com/workers/static-assets/billing-and-limitations/

## Addresses

- **Custom Domains:** `PUT /accounts/{id}/workers/domains` `{"hostname", "service", "zone_id"}` → `{id, hostname, service, zone_id, zone_name, cert_id}`; `GET /accounts/{id}/workers/domains?service=…&hostname=…`; `DELETE /accounts/{id}/workers/domains/{domain_id}`. "Cloudflare will create a new DNS record for you" and an Advanced Certificate. "You cannot create a Custom Domain on a hostname with an existing CNAME DNS record." Needs an active zone on Cloudflare; no wildcards. Deleting a Custom Domain leaves the certificate.
- **workers.dev:** `GET /accounts/{id}/workers/subdomain` → `{"subdomain": "name"}` (an account must have chosen one in the dashboard); per script `POST /accounts/{id}/workers/scripts/{name}/subdomain` `{"enabled": true, "previews_enabled": false}`. Address: `https://<script>.<subdomain>.workers.dev`.

Sources: https://developers.cloudflare.com/api/resources/workers/subresources/domains/methods/update/ · https://developers.cloudflare.com/workers/configuration/routing/custom-domains/ · https://developers.cloudflare.com/api/resources/workers/subresources/scripts/subresources/subdomain/methods/create/ · https://developers.cloudflare.com/api/resources/workers/subresources/subdomains/methods/get/

## Limits (Free / Paid)

| Limit | Free | Paid |
|---|---|---|
| Static asset files per Worker version | 20,000 | 100,000 |
| Size of one asset file | 25 MiB | 25 MiB |
| Workers per account | 100 | 500 |
| Requests that run a Worker | 100,000 / day | unlimited (billed) |
| Environment variables / secrets per Worker | 64 | 128 |
| Size of one variable | 5 KB | 5 KB |
| Custom domains per zone | 100 | 100 |
| Requests to static assets | free and unlimited | free and unlimited |

Teitunnel enforces 20,000 files and 25 MiB per file before uploading (the free limits), and warns that a password-protected Snapshot counts every request against the 100,000/day Worker limit.

Source: https://developers.cloudflare.com/workers/platform/limits/ (checked 2026-09-24)

## Permissions

| Need | API token permission | Template key | Notes |
|---|---|---|---|
| Upload, deploy, version, delete, workers.dev | Account › Workers Scripts › Edit | `workers_scripts` / `edit` | Every scripts/versions/deployments/subdomain/domains endpoint lists "Workers Scripts Write" |
| Custom Domains | Zone › Workers Routes › Edit, per affected zone | `workers_routes` / `edit` | "To add, update, or remove Routes or Custom Domains, you need Editor access to the Worker and Workers Routes Write permission for every affected zone." Custom Domains don't support per-Worker roles yet |
| Read the workers.dev subdomain | Workers Scripts Read or Write | (covered) | |

OAuth scope names mirror token permissions; the exact ids are confirmed when the OAuth client is registered (see cloudflare.md). Teitunnel asks for them as optional scopes.

Sources: https://developers.cloudflare.com/fundamentals/api/how-to/account-owned-token-template/ · https://developers.cloudflare.com/workers/authorization/workers/ (checked 2026-09-24)

## Password protection inside the Worker

- Workers implement WebCrypto (`crypto.subtle.importKey`, `deriveBits` with PBKDF2/SHA-256, HMAC `sign`). PBKDF2 runs natively.
- Secrets are bindings of type `secret_text` in the script metadata; `keep_bindings: ["secret_text"]` keeps them on a new version without resending them.
- CPU time on the Free plan is 10 ms per request; PBKDF2 at 20,000 iterations runs only on a login attempt, not on each request (a signed cookie is checked with one HMAC).
