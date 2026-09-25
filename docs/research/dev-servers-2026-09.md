# Dev servers and quick tunnels (2026-09-24)

Facts behind M12-01 (dev servers just work) and the M12-09 config item, read from source on 2026-09-24.

## How dev servers refuse unknown hosts

| Server | Answer | Allow list | Reads Host for anything else? |
|---|---|---|---|
| Vite 5.4.12+, 6.0.9+ (CVE-2025-24010) and every framework on it | 403 `text/plain`: ``Blocked request. This host ("x") is not allowed.\nTo allow this host, add "x" to `server.allowedHosts` in vite.config.js.`` (`preview.allowedHosts` for `vite preview`) | `server.allowedHosts`; a leading `.` allows subdomains; `localhost`, `*.localhost` and IPs always pass | No (only the rebinding check) |
| webpack-dev-server 4/5 | 403 `Invalid Host header` (v3: same text with 200) | `devServer.allowedHosts` | No |
| Next.js 16 (15.2+ once `allowedDevOrigins` is set) | 403 `Unauthorized` for cross-origin requests to `/_next/*` and `/__nextjs*` dev resources; the page itself loads | `allowedDevOrigins` (`*.example.com` patterns) | Server actions compare `Origin` with `Host`/`X-Forwarded-Host` |
| Rails 6.0 / 6.1+ | 403 page `Blocked host: x` / `Blocked hosts: x` with `config.hosts << "x"` | `config.hosts` (leading `.` for subdomains) | CSRF origin check against `base_url` |
| Django | 400 `DisallowedHost … Invalid HTTP_HOST header: 'x'` with `DEBUG`; a bare `Bad Request (400)` page without | `ALLOWED_HOSTS` (with `DEBUG` and an empty list: localhost only) | `CSRF_TRUSTED_ORIGINS` / Origin check |
| SvelteKit, Astro | Vite's answer | Vite's `server.allowedHosts` | Form actions / `checkOrigin` compare Origin with the request URL built from Host |

Sources: `vitejs/vite` `packages/vite/src/node/server/middlewares/hostCheck.ts` and the `host-validation-middleware` package (403, `text/plain`); `webpack/webpack-dev-server` `lib/Server.js` (`res.statusCode = 403; res.end("Invalid Host header")`); `vercel/next.js` `packages/next/src/server/lib/router-utils/block-cross-site-dev.ts` (blocked by default, `res.end('Unauthorized')`); Rails `actionpack/lib/action_dispatch/middleware/templates/rescues/blocked_host.html.erb`; Django `django/http/request.py` (`DisallowedHost`).

## cloudflared and quick tunnels

- `cloudflared tunnel` takes `--config` (default: the first `config.yml`/`config.yaml` in `~/.cloudflared`, `~/.cloudflare-warp`, `~/cloudflare-warp`, and on Unix `/etc/cloudflared`, `/usr/local/etc/cloudflared`). Settings from the file are applied as flags, and `ingress` rules from it take precedence over `--url` (`ingress.ParseIngressFromConfigAndCLI`), so a leftover named-tunnel config breaks a quick tunnel. A file named with `--config` that's missing is an error; an empty file is accepted but logged at error level ("Configuration file … was empty"); `{}` is accepted silently. Source: `cloudflare/cloudflared` `config/configuration.go` (`ReadConfigFile`), `cmd/cloudflared/cliutil/handler.go`, `ingress/ingress.go`.
- `--http-host-header` sets the Host header sent to the origin for `--url` (quick tunnels too). Source: `cmd/cloudflared/tunnel/cmd.go`.
- Quick tunnels: 200 requests in flight (429 beyond), no Server-Sent Events (see [competitors-2026.md](competitors-2026.md)).
- Cloudflare's own 413 page (request body over 100 MB on Free and Pro) is an nginx-style page ending in `<center>cloudflare</center>`.
