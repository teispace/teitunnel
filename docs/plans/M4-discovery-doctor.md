# M4: Discovery, Import & Doctor

**Goal:** Teitunnel already knows what's running, picks up existing setups, and finds and fixes problems before the user has to search for error codes.
**Release:** v0.4.0.
**Exit criteria:**
- The origin picker shows local services with project names and Docker containers.
- An existing `~/.cloudflared` setup (config.yml + credentials + cert.pem) and running foreign cloudflared processes can be imported or adopted.
- Every check in the catalogue below has either a fix plan or a precise explanation, with tests.
- "Fix all safe issues" never touches non-owned resources (property test).

---

### M4-01 · Discovery: processes & projects
- [x] Enrich listening ports with process cmdline and cwd, and derive the project name (`package.json#name`, `Cargo.toml`, `pyproject.toml`, `composer.json`, `go.mod`, folder name).
- [x] Framework hints from the cmdline (vite, next, nuxt, astro, remix, rails, django, flask, fastapi/uvicorn, laravel/artisan, hugo, jekyll…).

### M4-02 · Discovery: Docker
- [x] Socket detection (Docker Desktop, OrbStack, Colima, Podman compatible). Running containers with published ports are listed; names and compose project labels are used for suggestions. *(A minimal HTTP/1.0 client on the Unix socket instead of `bollard`: one endpoint, no extra dependency tree.)*
- [x] Handles an absent or unreachable daemon gracefully (no errors surfaced; the section is just hidden).

### M4-03 · Import existing cloudflared setups
- [x] Scan `~/.cloudflared`, `/etc/cloudflared`, `/usr/local/etc/cloudflared`, `/opt/homebrew/etc/cloudflared` for `config.yml`/`config.yaml`, `<uuid>.json` credentials and `cert.pem`.
- [x] `config_yaml` parse into our model (ingress + originRequest + unknown keys preserved) with fixtures.
- [x] Import wizard: show what was found, then match against the account's tunnels. Options: **Manage as-is** (locally-managed: Teitunnel runs `cloudflared tunnel --config <path> run` and edits the YAML through a YAML-preserving writer) or **Migrate to remote-managed** (upload ingress via configurations PUT, switch the connector to the token, and keep the old files as a backup). Plan preview as always. *(Implemented as "import routes into this Mac's tunnel" (`Intent::ImportRoutes`), D-044: the old tunnel's DNS records are repointed after confirmation; files untouched. "Manage as-is" with YAML editing is not planned for v1.)*
- [ ] Also detect launchd plists / systemd units running cloudflared and offer to take them over.

### M4-04 · Adoption of running processes
- [ ] Find foreign cloudflared processes, probe metrics ports (defaults + parsed `--metrics`), and show them as Discovered. **Adopt** = observe only (health, metrics). **Take over** = stop theirs and start ours (plan preview, with the explicit warning shown).

### M4-05 · Doctor framework
- [x] `Check` trait, registry, shared snapshot, scheduling (startup, post-apply, connector state change, manual, every 5 min while visible). *(Checks are pure functions over gathered `Facts` rather than trait objects; runs on open, focus, route changes, manual refresh and every 5 min.)*
- [x] Issue dedup and stable issue ids (so dismissals persist). "Ignore this issue" is stored in the persisted UI store (per Mac).
- [x] Issue → fix `Intent` → planner (fixes open the plan sheet). "Fix all safe" = fixes whose plans touch only owned resources and need no confirmation (DNS repairs and orphan deletions; property-tested).

### M4-06 · Check catalogue (one file + tests each)
| Id | Detects | Fix / guidance |
|---|---|---|
| `binary.missing` / `binary.outdated` / `binary.unsupported` | cloudflared state | Install/update managed |
| `origin.not_listening` | route origin port closed | Show the last process seen on that port; "Open project folder" if known |
| `origin.tls` | 502 + TLS handshake errors in logs for https origins | Plan: set `noTLSVerify` or `originServerName` (confirm) |
| `origin.http_error` | verify stage 4 5xx | Show response details |
| `dns.missing` | route without CNAME | Plan: create owned CNAME |
| `dns.wrong_target` | CNAME → other tunnel | Plan: update (confirm if not owned) |
| `dns.not_proxied` | CNAME grey-clouded | Plan: set proxied |
| `dns.conflict` | A/AAAA/CNAME blocks the hostname | Show the record, confirm replace |
| `dns.orphan_owned` | owned CNAME, no route | Plan: delete (safe) |
| `dns.orphan_foreign` | CNAME → nonexistent tunnel (takeover risk) | Plan: delete (confirm) |
| `zone.pending` | nameservers not switched | Show NS values |
| `tunnel.no_connections` / `tunnel.degraded` / `tunnel.crash_loop` | connector health | Restart; show the last errors |
| `tunnel.stale_connections` | stale connections listed | Plan: clean connections |
| `tunnel.unused_owned` | our tunnel, no routes, idle > 7 days | Plan: delete |
| `tunnel.duplicate_local` | two local connectors for one tunnel | Stop extras |
| `net.udp_blocked` | QUIC failures in logs, HTTP/2 works | Set `protocol: http2` for the connector |
| `net.clock_skew` | TLS time errors | Explain |
| `config.drift` | remote version > last applied | Keep theirs / restore mine |
| `auth.missing_scope` | a feature needs a missing capability | Re-consent / new token |
| `auth.expiring` | token expiry near (if known) | Renew |

### M4-07 · UI: Doctor
- [x] Sidebar badge with the issue count (errors only). The Doctor view groups issues by severity then subject. The Inspector shows explanation, evidence (log lines, records), and Fix (with a plan preview). *(Done; log-line evidence comes with M5 logs.)*
- [x] "Fix all safe issues". *(Applies each fix through its own fresh plan and reports fixed/skipped/failed, instead of one combined preview.)*
- [ ] Issues also appear inline on the affected route/tunnel rows.

### M4-08 · Cleanup center
- [ ] A Doctor sub-view listing everything removable: owned orphans, stale tunnels, old managed binaries, old logs, Quick Share history. Multi-select, then one combined plan.

### M4-09 · Diagnostics export
- [ ] Help → Export diagnostics: a redacted zip (app logs, Doctor report, versions, settings minus secrets, and optionally `cloudflared tunnel diag` for a selected connector). A preview of the contents is shown before saving.
