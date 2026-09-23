# Cloudify (getcloudify.app): feature analysis

Researched 2026-09-23 from the landing page (a React SPA; strings read from its bundle `assets/index-D576TiRT.js`), the README and the source of `github.com/VickyDev810/Cloudify-Tunnel-Manager` (Python CLI + FastAPI, Next.js frontend, 4 stars, last push 2025-11-04).

## What it is
A Python CLI (`cloudify`) wrapping `cloudflared` by shelling out, plus an optional web UI (`cloudify serve`, FastAPI on :8765 and a Next.js frontend) with its own username/password accounts. Authentication with Cloudflare is `cloudflared tunnel login` (cert.pem). State is JSON files in the home directory.

## Features as claimed and as built

| Feature (their wording) | What the code does | Teitunnel today |
|---|---|---|
| Quick Tunnels: "temporary tunnels instantly" (`cloudify -p 3000`, `temp create/list/stop`) | `cloudflared tunnel --url`, parses the trycloudflare URL; `--subdomain` passes `--name` (doesn't give a custom trycloudflare name) | ✅ Quick Share (app, tray, CLI `share`), QR, stats, logs. Gap: `share list/stop` across CLI processes; a share on **your own** subdomain |
| Tunnel management: create, list, status, start, stop, delete, `use`, `adopt` | Several named tunnels per machine, one "current" tunnel; adopt = manage an existing tunnel by name | Partly: one machine tunnel per account (D-041), Tunnels view of all tunnels, import moves routes (D-044). Gap: **several tunnels per machine** (environments), start/stop/delete per tunnel, adopting an existing tunnel |
| Auto-start ("auto-restart functionality") | systemd user unit, cron `@reboot` fallback, LaunchAgent, Task Scheduler | ✅ Always-on (launchd, systemd --user, Task Scheduler). Gap: **system-level service for headless servers** (no user session), per-tunnel |
| Route management: subdomains, custom endpoints (container names), port mapping | Ingress rules in `config.yml`, `cloudflared tunnel route dns` | ✅ Richer: plan → apply with rollback, DNS ownership, paths, TCP/SSH/RDP, Access logins, drift, import, export |
| Web interface (`cloudify serve`, admin account, setup wizard, dashboard, routes, settings) | FastAPI + Next.js, bcrypt users, JWT, "Recent Activity", "System Information" | Desktop app (richer). Gap: **browser UI for headless servers** |
| "Comprehensive API" | The FastAPI endpoints behind the web UI (`/tunnels`, `/tunnel/route/add`, …) | Gap: a documented **local HTTP API** |
| Cross-platform: Linux, macOS, Windows (beta, run as admin), **Docker** | Install scripts, `.exe`, Docker image `vickydev810/cloudify` with a `--network` recipe | macOS/Windows/Linux builds. Gap: **Docker image**, one-line install script for the CLI |
| "Cloud integration: AWS, Azure, GCP" | Environment detection only (reads EC2 metadata, `/proc/version`); no cloud API use | Gap (docs + server mode): **guides and a server mode** that work on any VM |
| "Load balancing across multiple endpoints" | Not implemented (no code) | Declined replicas (D-058). Gap: real **Cloudflare Load Balancing** across machines with health checks, for accounts that have it |
| Real-time monitoring: status, traffic, performance | Status strings from `systemctl`/`ps` | ✅ Metrics, charts, logs, health, Doctor, notifications, menu bar |
| Setup wizard (`cloudify setup`, `quickstart`) | Interactive login + first tunnel | ✅ App onboarding. Gap: **CLI setup wizard** for headless machines |
| Security ("enterprise-grade") | Cloudflare's; local web UI password | ✅ Keychain, no shells, plan → apply, Access logins, no secrets over IPC |

Things Teitunnel has that Cloudify doesn't: multi-account OAuth/token/cert, domains view, DNS ownership and conflict handling, plan/undo/rollback, drift detection, Access logins, private networks and WARP checks, remote connectors and logs, Doctor with fixes, discovery of local services and Docker containers, import, export (config.yml/Compose/Terraform), metrics history, i18n, native apps.

## Gaps to close (planned in [M10](../plans/M10-parity.md))
1. Several tunnels per machine (environments), start/stop/delete per tunnel, adopt an existing tunnel.
2. Share on your own subdomain (a temporary route, removed when the share stops or expires).
3. `share list` / `share stop` across CLI processes and the app.
4. Headless server mode: CLI setup wizard, tunnel/always-on commands, system-level services (systemd system unit, cron fallback), environment-friendly credentials.
5. Docker image (CLI + cloudflared) and Compose recipes; guides for AWS, Azure, GCP and any VPS.
6. Browser UI and local HTTP API for headless machines (`teitunnel-cli serve`), authenticated, loopback by default.
7. Cloudflare Load Balancing across machines, with health monitors, through plan → apply.
8. One-line install script (with distribution, last).
