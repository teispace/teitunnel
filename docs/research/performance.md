# Performance baselines

Measured on the maintainer's Mac (macOS 27.0, Apple silicon), screen locked, nothing else
of note running. Re-measure after changes that could affect them; a clear regression
blocks a release (M6-05).

## Packaged app (2026-09-25, `next` after 0.2.0, arm64 release profile, unsigned)
Same command and machine, 5 runs.

| Metric | Result | vs 0.1.0 |
|---|---|---|
| Bundle (`Teitunnel.app`) | 49.3 MB: app 27 MB + bundled `teitunnel-cli` 22 MB; the `.dmg` is 24.9 MB | +34.6 MB |
| Cold start | median 604 ms (511–729 ms) | same, within the 0.1.0 range |
| Idle memory after 20 s | median 159 MB (154–168 MB) | −32 MB |

The size is 0.2.0's, not a regression since: the installed universal 0.2.0 has a 62 MB app
and a 50 MB CLI (two architectures each). It comes from the bundled CLI (D-077, added after the 0.1.0
measurement below) and M12's crates in the app (MCP, control, Lens, Workers, Snapshots). The
CLI stays a separate binary because it's installed as a stable link or copy on each
platform and ships alone (Homebrew, Docker, archives); sharing one binary with the app
would make every terminal command load WebKit and AppKit.

## Packaged app (2026-09-23, `v0.1.0` development build, release profile, unsigned)
`pnpm --filter @teitunnel/desktop perf:app ../../target/release/bundle/macos/Teitunnel.app 5`
(`scripts/measure-app.ts`): each run starts the app with an empty `TEITUNNEL_DATA_DIR`,
so no accounts, keychain items or connectors load.

| Metric | Result |
|---|---|
| Bundle (`Teitunnel.app`) | 14.7 MB |
| Cold start: process start → first painted frame (the app logs `startup_ms`) | median 585 ms (524–879 ms); ~1.6 s with a cold disk cache |
| Idle memory after 20 s: app + the WebKit processes it started | median 191 MB (159–211 MB) |

Idle memory is dominated by WebKit's content and GPU processes. They're attributed by
start time (launched within 3 s of the app), so the figure is approximate; compare runs
on the same machine rather than across machines.

## UI under load (2026-09-23, WebKit)
`pnpm --filter @teitunnel/desktop perf 30`: log at ~2,000 lines/s plus a 3,600-point
chart at 20 Hz: 60 fps, p50 17.0 ms, p99 19.0 ms, max 20.0 ms, no frame over 50 ms; DOM
constant at 186 nodes (D-051).
