# M5: Observability & Always-on

**Goal:** see exactly what your tunnels are doing, live, and keep them running across app quits and reboots.
**Release:** v0.5.0 (feature complete for v1.0 on macOS).
**Exit criteria:**
- Always-on connectors survive app quit and reboot. The app observes them and can switch modes without downtime surprises.
- The log viewer sustains 2,000 lines/s at 60 fps. Charts run for 1 h at 1 Hz without jank or memory growth.
- The menu bar extra shows live health and can toggle routes/shares.

---

### M5-01 · Metrics pipeline
- [ ] Scraper per connector (1 s while subscribed, 10 s otherwise). Derived series: requests/s, error rate, response-code classes, HA connections, RTT (latest/smoothed), active TCP/UDP sessions.
- [ ] In-memory ring buffer (3,600 points per series). Minute rollups go to `metrics_rollup` (7-day retention, pruned daily).
- [ ] Edge locations from the connections API (colo names) and from log events.
- [ ] Commands: `metrics_subscribe(tunnel) -> Channel<MetricsBatch>`, `metrics_history(tunnel, range)`.

### M5-02 · Charts
- [ ] `Sparkline` and `TimeSeriesChart` patterns on uPlot, styled from tokens (theme-aware, redrawn on theme change), with hover crosshair and tooltip, tabular digits, and no animation except appending data.
- [ ] Route/tunnel inspector "Traffic" section, plus an Overview with health summary and top routes by traffic.

### M5-03 · Log viewer
- [ ] `logs_subscribe(connector, filter) -> Channel<LogBatch>` (server-side filter by level/text to save IPC), plus `logs_history(connector, before, limit)`.
- [ ] `LogViewer` pattern: virtualized, monospace, level colouring (tokens), follow-tail with auto-pause on scroll up and a "Jump to latest" pill, search with highlight, level filter, pause/resume, copy selection, export to file.
- [ ] Per-route view (filters log events mentioning its hostname) and per-connector view.

### M5-04 · Activity view
- [ ] Timeline of applied plans and steps (who/what/when), with the before/after diff, "Copy as command", and re-run verify. Filters by domain/tunnel/type.
  *(Started early in M4: timeline by day with outcome and step details per account. Diff, copy-as-command, re-verify and filters remain.)*

### M5-05 · Always-on (macOS launchd)
- [ ] `ServiceManager` launchd adapter: generate the plist (label `com.teispace.teitunnel.connector.<tunnel-id>`, managed binary path, args from the `RunCmd` builder with `--token-file`, `--log-directory <app_data>/logs/connectors/<id>`, `KeepAlive`, `RunAtLoad`, `ProcessType=Background`, `ThrottleInterval`), then install via `launchctl bootstrap gui/<uid>`, and remove via `bootout`. Status via `launchctl print` parsing plus the metrics endpoint.
- [ ] Token file handling per SECURITY_MODEL (0700 dir, 0600 file, removed on uninstall).
- [ ] Mode switch Session ↔ Always-on as a plan (start new → wait healthy → stop old, so there's no gap).
- [ ] Log tailing for service-run connectors (tail the log directory's current file, then parse JSON).
- [ ] Binary update with always-on connectors: stage the new binary, then restart services one at a time and verify health.
- [ ] Tests: plist snapshot tests; recording fake ServiceManager; a macOS-only integration test in CI (bootstrap and bootout a fake-cloudflared service).
- [ ] Linux systemd --user and Windows Task Scheduler adapters: **compile + unit tests only** here, polished in M7/M8.

### M5-06 · App lifecycle & menu bar extra
- [ ] Closing the window keeps the app in the menu bar while anything is running (setting). Launch at login (`tauri-plugin-autostart`), starting hidden into the menu bar.
- [ ] Menu bar menu: overall health line, routes (status + Open/Copy/Disable), Quick Shares, "New Quick Share…", "Open Teitunnel", "Quit". The icon changes for degraded or error state (template variants). *(Started: routes with status + Copy URL / Open in Browser above Quick Shares, refreshed after changes and every minute. Health line, Disable and icon variants remain.)*
- [ ] Quit confirmation when Session connectors are running ("Switch them to Always-on?" as a shortcut).

### M5-07 · Notifications policy
- [ ] Settings: notify on connector down / recovered / crash loop / Doctor error / Quick Share events. Coalesced (no storms), and suppressed while the window is focused.
