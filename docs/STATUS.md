# Status: live handoff

> **Read this first in every new session.** It records where the project stands and what to do next.
> Update it after **every** task (see "Docs" in [CONVENTIONS.md](CONVENTIONS.md)).

**Last updated:** 2026-09-23
**Phase:** **M3: Routes engine** in progress
**Branch:** `milestone/m3-routes-engine` (draft PR #4, stacked on #3 → #2 → #1). The prototype is archived at tag `legacy-prototype` / branch `legacy/prototype`.

## Working mode: AUTONOMOUS
The maintainer starts a session with "start" or "continue" and is then **away**. Work unattended, following [AUTONOMOUS.md](AUTONOMOUS.md): loop task by task through the roadmap, build, test, verify visually, fix and polish, commit, push, and keep this file current. Don't stop to ask. Decide, record the decision in DECISIONS.md, and continue.

## Next up
1. **M3-09 Routes UI**: desktop commands over the engine (`routes_list`, `routes_preview`, `routes_apply` with a progress Channel, `routes_verify`, `routes_drift`, `routes_keep_theirs`), resume machine connectors at launch, then the Routes view, the add-route sheet with plan preview and apply progress, remove flow, undo toast.
2. M3-10 Tunnels view (Advanced), M3-11 tests (vitest flows, E2E with a fake CloudApi build feature).
3. **Maintainer:** review/merge PRs #1–#4 in order; v0.1 signing decision + tag; OAuth client; test token for the nightly real-account job.

## In progress
- **M3** on `milestone/m3-routes-engine`, draft PR #4. Done: cf-api tunnels/config/DNS (M3-01), domain types (M3-02), observer (M3-03), planner with snapshots + convergence property (M3-04), executor with per-step rollback and failure injection at every mutation (M3-05), verifier that never resolves the new hostname (M3-06, D-040), machine tunnel connector with keychain token and stable metrics port (M3-07), drift (M3-08). Next: UI.
- **M2** complete except maintainer items (OAuth client, real fixtures); PR #3. **M1** PR #2, **M0** PR #1 ready for review.

## Recently completed
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
- **Resume point:** branch `milestone/m3-routes-engine` (draft PR #4). Check `gh run list` first; fix red CI before new work. Then continue with "Next up".
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
