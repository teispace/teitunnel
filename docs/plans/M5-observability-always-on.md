# M5: Observability & Always-on

**Goal:** see exactly what your tunnels are doing, live, and keep them running across app quits and reboots.
**Release:** v0.5.0 (feature complete for v1.0 on macOS).
**Exit criteria:**
- Always-on connectors survive app quit and reboot. The app observes them and can switch modes without downtime surprises.
- The log viewer sustains 2,000 lines/s at 60 fps. Charts run for 1 h at 1 Hz without jank or memory growth.
- The menu bar extra shows live health and can toggle routes/shares.

**Exit check (2026-09-23):** `pnpm --filter @teitunnel/desktop perf 30` in WebKit: ~2,000 log lines/s plus a 3,600-point chart at 20 Hz: 60 fps, p99 19 ms, max 20 ms, no long frames, DOM constant (D-051). Always-on across quit/reboot: launchd KeepAlive + RunAtLoad, real-launchd nightly test; mode switches and binary updates are gapless (D-045, D-049).

---

### M5-01 · Metrics pipeline
- [x] Scraper per connector (1 s while watched, 10 s otherwise). Derived series: requests/s, failed/s, response-code classes, HA connections, smoothed RTT, requests in flight. *(`core::traffic`; the 1 s rate is a lease renewed by each read, D-046. TCP/UDP session counts are parsed (`active_sessions`) but not charted yet.)*
- [x] In-memory ring buffer (3,600 points per series). Minute rollups go to `metrics_rollup` (7-day retention, pruned on write).
- [x] Edge locations from the connections API (colo names, Tunnels ▸ Connectors) and from `/metrics` (Traffic ▸ Edge).
- [x] Commands: `tunnels_traffic(tunnel, since)` (cursor polling instead of a Channel subscription, D-046), `tunnels_traffic_history(tunnel, day|week)`.

### M5-02 · Charts
- [x] `TimeSeriesChart` pattern on uPlot, styled from tokens (theme-aware, repainted on appearance change), with hover crosshair and readout, tabular digits, gaps where data is missing, and no animation except appending data. *(The SVG `Sparkline` stays for tiny inline charts.)*
- [x] Route/tunnel inspector "Traffic" section, plus an Overview traffic summary. *(Tunnel inspector: Hour/Day/Week, requests and failures per second, round trip, response classes. Overview: this Mac's rate, total and last-hour sparkline, polled every 10 s. Per-route traffic and "top routes" need per-hostname metrics cloudflared doesn't expose.)*

### M5-03 · Log viewer
- [x] Log reads: `tunnels_logs(tunnel, limit)` and `routes_logs(account, hostname, path, limit)` polled every 2 s; the route filter runs in the backend, level/text in the window (D-050). *(A push subscription isn't needed at ≤1,000 lines; same reasoning as D-046.)*
- [x] `LogViewer` pattern: virtualized (`@tanstack/react-virtual`, measured rows for wrapped lines), monospace, level colouring (tokens), follow-tail with auto-pause on scroll up and a "Jump to latest" pill, search with highlight, level filter, pause/resume, copy, and save to a file (Downloads, redacted, `app_save_log`).
- [x] Per-route view and per-connector view. *(Routes ▸ Logs matches events by the route's `ingressRule` + `originService` in the applied ingress (research/cloudflare.md); Tunnels ▸ Logs per connector. Always-on log files are bounded and tail-read, D-050.)*

### M5-04 · Activity view
- [x] Timeline of applied plans and steps (what/when, per account), with the before/after diff, "Copy as command" (per step and all at once), and re-run verify. Filters by kind/problems and domain. *(Structured `ActivityRecord` per entry, D-047. No tunnel filter: one machine tunnel per account.)*

### M5-05 · Always-on (macOS launchd)
- [x] `ServiceManager` launchd adapter: generate the plist (label `com.teispace.teitunnel.connector.<tunnel-id>`, managed binary path, args from the `RunCmd` builder with `--token-file`, `--log-directory <app_data>/logs/connectors/<id>`, `KeepAlive`, `RunAtLoad`, `ProcessType=Background`, `ThrottleInterval`), then install via `launchctl bootstrap gui/<uid>`, and remove via `bootout`. Status via `launchctl print` parsing plus the metrics endpoint. *(StandardOut/ErrorPath to `<app_data>/logs/connectors/<id>.log` instead of `--log-directory`, so the JSON log is one file the app tails, D-045.)*
- [x] Token file handling per SECURITY_MODEL (0700 dir, 0600 file, removed on uninstall).
- [x] Mode switch Session ↔ Always-on as a plan (start new → wait healthy → stop old, so there's no gap). *(`MachineTunnels::set_always_on`; a new connector that doesn't connect in 30 s is removed and the old one keeps running.)*
- [x] Log tailing for service-run connectors (tail the log directory's current file, then parse JSON).
- [x] Binary update with always-on connectors: stage the new binary, then restart services one at a time and verify health. *(Bridged by a temporary app connector, so there's no gap; Session connectors restart in place, D-049.)*
- [x] Tests: plist snapshot tests; recording fake ServiceManager; a macOS-only integration test in CI (bootstrap and bootout a fake-cloudflared service). *(Real-launchd test is opt-in (`TEITUNNEL_TEST_LAUNCHD=1`) and runs in the nightly workflow; the gapless switch is tested with `ProcessServices` + fake-cloudflared in every CI run.)*
- [x] Linux systemd --user and Windows Task Scheduler adapters: **compile + unit tests only** here, polished in M7/M8. *(`cloudflared::service::ServiceSpec` is the neutral definition (tokens only from a file); `launchd`, `systemd` (user unit, `append:` logs) and `task_scheduler` (UTF-16 task XML, logon trigger, `CommandLineToArgvW` quoting) render it, with snapshot tests; `core::service::{Systemd, TaskScheduler}` implement `ServiceManager` but aren't selected yet. On Windows the connector must write its own log (`RunCmd::log_dir`).)*

### M5-06 · App lifecycle & menu bar extra
- [x] Closing the window keeps the app in the menu bar (always: routes and shares keep running; ⌘Q quits, with the quit confirmation). Launch at login (`tauri-plugin-autostart`), starting hidden into the menu bar. *(No "quit when the window closes" setting: a menu bar utility that quits on close would take routes down by surprise.)*
- [x] Menu bar menu: overall health line, routes (status + Open/Copy), Start/Stop Routes on This Mac, Quick Shares, "Share a Local Port…", "Open Teitunnel", "Quit". The icon shows a dot for a route that isn't working (not when the user stopped the routes). *(A per-route "Disable" has no engine counterpart (a route is served or removed); the switch stops/starts this Mac's connectors, like the Tunnels view.)*
- [x] Quit confirmation when Session connectors are running ("Switch them to Always-on?" as a shortcut). *(⌘Q or the menu bar Quit asks: Keep Routes Running (switches to Always-on, then quits) / Quit Anyway / Cancel. Only when routes run through the app and Always-on is available.)*

### M5-07 · Notifications policy
- [x] Settings: notify on connector down / recovered / crash loop / Doctor error / Quick Share events. Coalesced (no storms), and suppressed while the window is focused. *(Connector notices: `core::health` (20 s grace). Doctor errors: `core::doctor_monitor`, background runs every 5 min when the window isn't running them, ignores in settings (D-048).)*
