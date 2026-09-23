# Status: live handoff

> **Read this first in every new session.** It records where the project stands and what to do next.
> Update it after **every** task (see "Docs" in [CONVENTIONS.md](CONVENTIONS.md)).

**Last updated:** 2026-09-23
**Phase:** **M5: Observability & always-on** in progress (M4 complete)
**Branch:** `milestone/m5-observability-always-on` (stacked on #5 → #4 → #3 → #2 → #1). The prototype is archived at tag `legacy-prototype` / branch `legacy/prototype`.

## Working mode: AUTONOMOUS
The maintainer starts a session with "start" or "continue" and is then **away**. Work unattended, following [AUTONOMOUS.md](AUTONOMOUS.md): loop task by task through the roadmap, build, test, verify visually, fix and polish, commit, push, and keep this file current. Don't stop to ask. Decide, record the decision in DECISIONS.md, and continue.

## Next up
1. M5 rest: log subscription with server-side filtering + per-route logs (M5-03), edge colo names (M5-01), close-window setting (M5-06), systemd/Task Scheduler adapter stubs (M5-05).
2. Nightly real-account job (needs the maintainer's test token).
3. M6 distribution (signing/notarization need the maintainer's Developer ID).
4. **Maintainer:** review/merge PRs #1–#6 in order; v0.1 signing decision + tag; OAuth client; test token.

## In progress
- **M5** on `milestone/m5-observability-always-on`, draft PR #6. Done: Always-on via launchd (gapless switch, token file, log tailing, real-launchd nightly test, D-045). Earlier (on M4): Activity timeline, connector logs, traffic sparkline, routes in the menu bar.
- **M4** PR #5, **M3** PR #4 ready for review. **M2** PR #3, **M1** PR #2, **M0** PR #1.

## Recently completed
- 2026-09-23: M5-02 complete: Overview traffic card.
- 2026-09-23: M5-05 cloudflared updates move running connectors onto the new binary without a gap (D-049).
- 2026-09-23: M5-06 menu bar Start/Stop Routes on This Mac (shared with the Tunnels view; a deliberate stop isn't flagged as a problem).
- 2026-09-23: M5-07 Doctor notifications: shared `DoctorMonitor`, background runs when the window isn't running them, ignores moved to settings (D-048).
- 2026-09-23: M5-04 Activity: structured records (kind, hostnames, step states, before/after), Copy All as Commands, Check again, kind/domain filters (D-047); rollbacks now mark implicitly reversed steps Undone.
- 2026-09-23: M5 metrics pipeline + uPlot charts: 1 s sampling while watched, 3,600-sample ring, minute rollups (7 days), Tunnels ▸ Traffic with Hour/Day/Week (D-046).
- 2026-09-23: M5 notifications + settings, quit confirmation, open at login, log viewer, Activity filters, tray health line and alert icon; E2E build separated from the debug app.
- 2026-09-23: M5 Always-on connectors (launchd).
- 2026-09-23: M4 import, foreign connectors, diagnostics, log checks; Overview/Activity/Tunnels logs.
- 2026-09-23: M4 Doctor (checks, view, fix safe) and Docker/framework discovery.
- 2026-09-23: M3 E2E against tools/fake-cloudflare; 31 planner scenarios; PR #4 ready.
- 2026-09-23: M3 Routes and Tunnels UI (sheet: form → review → apply progress → verify; undo; drift banner).
- 2026-09-23: M3 engine: observe → plan → apply (rollback) → verify, drift, machine connectors.
- 2026-09-23: M2 accounts & domains (token/cert flows, keychain, capabilities, Domains view).
- 2026-09-23: M1 feature-complete: verified managed install, Quick Share end to end (fake + real nightly), menu bar shares, notifications, onboarding and cloudflared settings, E2E on macOS CI (WebdriverIO + fake cloudflared), release workflow and notes, README install section.
- 2026-09-23: M1-01..08; packaged build verified at launch level.
- 2026-09-22: M0 complete. CI green on macOS/Linux/Windows.

## Blockers / maintainer actions needed
| Item | Needed by | Notes |
|---|---|---|
| Cloudflare OAuth public client + verify teispace.com | M2-04 (built; hidden until then) | Redirects `http://127.0.0.1:{53682,53683,53684}/callback`. Put the client id in `crates/core/src/accounts/oauth.rs` (`CLIENT_ID`) and confirm the scope ids in `SCOPES` |
| Confirm the "Cloudflare Tunnel" permission key in the token template | M2-05 | Open the link from Connect Cloudflare once; if Tunnel isn't pre-selected, tell me the right key (research/cloudflare.md) |
| Test Cloudflare account/zone + API token for nightly E2E | M1-12 / M3-11 | Store as GitHub Actions secrets |
| Apple Developer ID (signing + notarization) | M6-01 (v0.1 can ship unsigned as a "developer preview") | Decide before M1-13 |

## Open questions
- v0.1 signing: unsigned developer preview (ready: DMG + Gatekeeper instructions in the release notes) vs waiting for a Developer ID?

## Notes for the next session
- **Resume point:** branch `milestone/m5-observability-always-on`. Check `gh run list` first; fix red CI before new work. Then continue with "Next up".
- Engine entry points (crates/core/src/engine): `Engine::{preview, apply, verify, drift, keep_theirs}`; ports `CloudApi` (impl for `cf_api::Client`, fake in `engine/fake.rs`) and `Connectors` (impl `machine::MachineTunnels`). Desktop must call `MachineTunnels::forget_account` before `Accounts::remove`.
- **Locked screen:** during unattended sessions the screen locks; macOS then stops compositing windows and `screencapture` returns blank content. Never send clicks/keys while locked. Use `pnpm --filter @teitunnel/desktop shoot` (WebKit) instead (D-030). Keep the Mac awake with `caffeinate -dimsu`.
- Visual verification helpers (recreate in the scratchpad if missing): a Swift `winid` tool (CGWindowList → window id), `screencapture -x -o -l <id>`, and Pillow for pixel sampling. Reference screenshots: System Settings and Finder on macOS 27.
- Toggle the appearance with `osascript -e 'tell app "System Events" to tell appearance preferences to set dark mode to true|false'`. **The maintainer uses dark mode; restore it.** Accent tests: `defaults write -g AppleAccentColor -int 3` then `defaults delete -g AppleAccentColor` to restore (the key was absent) and post `AppleColorPreferencesChangedNotification`.
- Don't automate Finder via AppleScript (it triggers an Automation permission prompt and times out).
- pnpm 12 rejects packages published < 1 day ago (D-027); pin the previous release.
- Toolchain on the maintainer machine: macOS 27.0, rustc 1.98, Node 26.9, pnpm 12.4.1.
- Stay on Tauri 2.11.x. **Do not** use `NSGlassEffectView` (D-021). Verify material work in a **packaged** build.
- Commits: Conventional Commits, **no AI attribution**. Gate every commit on `pnpm verify` (exit code, not eyeballing output).
- Push in batches: every push cancels the running CI (concurrency group), and Windows is only checked in CI.
- E2E builds go to `target/e2e` (never over `target/debug/Teitunnel`, which the maintainer runs by hand). Standalone debug app: `pnpm --filter @teitunnel/desktop tauri build --debug --no-bundle`.
- `target/debug/incremental` grows to tens of GB and filled the disk once (2026-09-23); `rm -rf target/debug/incremental` is safe when space runs low.
