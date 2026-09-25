# Cloudflare API changes of 2026-10-05

Source: https://developers.cloudflare.com/changelog/post/2026-07-09-tunnel-routes-and-connections-api-changes/ (read 2026-09-24).

- Removed: `POST|PATCH|DELETE /accounts/{account_id}/teamnet/routes/network/{ip_network_encoded}`. Use `POST /accounts/{account_id}/teamnet/routes` (network in the body), `PATCH|DELETE …/teamnet/routes/{route_id}`. Teitunnel already does.
- Removed field: `connections` in `GET /accounts/{account_id}/cfd_tunnel`, `…/cfd_tunnel/{tunnel_id}`, `…/warp_connector`, `…/warp_connector/{tunnel_id}`. Use `GET …/cfd_tunnel/{tunnel_id}/connections` (one entry per connector: `id`, `version`, `arch`, `run_at`, `conns[]` with `colo_name`, `origin_ip`, `opened_at`, `is_pending_reconnect`). Handled in `cf_api::tunnels` (D-094).

Also from the Tunnel changelog (2026): cloudflared drops 32-bit Windows and Intel macOS builds in 2027 (announced 2026-09-18); `proxy-dns` removed from releases since 2026-02-02; connectivity pre-checks at startup (2026-05-27); per-tunnel granular permissions (2026-05-21).

## Full audit (2026-09-24, D-095)

Checked against https://developers.cloudflare.com/fundamentals/api/reference/deprecations/:
- `PATCH /zones/{zone_id}/dns_records/{id}` can't change `type` since 2026-06-30 → delete + create; used via `POST /zones/{zone_id}/dns_records/batch` (`deletes`, `patches`, `puts`, `posts`, executed in that order in one database transaction; any failure applies nothing; Free plans 200 records per batch). Source: https://developers.cloudflare.com/dns/manage-dns-records/how-to/batch-record-changes/
- Access `self_hosted_domains` retired 2025-11-21 → `destinations: [{ "type": "public", "uri": "…" }]`.
- Reusable Access policies: `POST /accounts/{account_id}/access/policies` (`name`, `decision`, `include`, `exclude`), attached as `policies: [{ "id", "precedence" }]`; legacy app-scoped policies have no end date, but "cannot be added to newly created Access applications" in the dashboard. `PUT …/access/apps/{app_id}/policies/{policy_id}/make_reusable` converts one. Source: https://developers.cloudflare.com/cloudflare-one/access-controls/policies/policy-management/
- Account-owned tokens verify at `GET /accounts/{account_id}/tokens/verify`; `GET /user/tokens/verify` answers 1000 "Invalid API Token" for them. They don't support the Zero Trust Client Platform. Source: https://developers.cloudflare.com/fundamentals/api/get-started/account-owned-tokens/
- Not used by Teitunnel: the devices list/revoke endpoints retired 2025-11-11 (we use `devices/settings` and `devices/policy`, unaffected), service keys (2026-09-30), zone settings batch (2027-03-31).
