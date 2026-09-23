# Performance baselines

Measured on the maintainer's Mac (macOS 27.0, Apple silicon), screen locked, nothing else
of note running. Re-measure after changes that could affect them; a clear regression
blocks a release (M6-05).

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
