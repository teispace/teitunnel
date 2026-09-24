# Cloudflare API changes of 2026-10-05

Source: https://developers.cloudflare.com/changelog/post/2026-07-09-tunnel-routes-and-connections-api-changes/ (read 2026-09-24).

- Removed: `POST|PATCH|DELETE /accounts/{account_id}/teamnet/routes/network/{ip_network_encoded}`. Use `POST /accounts/{account_id}/teamnet/routes` (network in the body), `PATCH|DELETE …/teamnet/routes/{route_id}`. Teitunnel already does.
- Removed field: `connections` in `GET /accounts/{account_id}/cfd_tunnel`, `…/cfd_tunnel/{tunnel_id}`, `…/warp_connector`, `…/warp_connector/{tunnel_id}`. Use `GET …/cfd_tunnel/{tunnel_id}/connections` (one entry per connector: `id`, `version`, `arch`, `run_at`, `conns[]` with `colo_name`, `origin_ip`, `opened_at`, `is_pending_reconnect`). Handled in `cf_api::tunnels` (D-094).

Also from the Tunnel changelog (2026): cloudflared drops 32-bit Windows and Intel macOS builds in 2027 (announced 2026-09-18); `proxy-dns` removed from releases since 2026-02-02; connectivity pre-checks at startup (2026-05-27); per-tunnel granular permissions (2026-05-21).
