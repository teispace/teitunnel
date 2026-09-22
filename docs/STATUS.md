# Status: live handoff

> **Read this first in every new session.** It records where the project stands and what to do next.
> Update it after **every** task (see "Docs" in [CONVENTIONS.md](CONVENTIONS.md)).

**Last updated:** 2026-09-22
**Phase:** **M0: Foundations** in progress
**Branch:** `milestone/m0-foundations` (PR #1). The prototype is archived at tag `legacy-prototype` / branch `legacy/prototype`.

## Working mode: AUTONOMOUS
The maintainer starts a session with "start" or "continue" and is then **away**. Work unattended, following [AUTONOMOUS.md](AUTONOMOUS.md): loop task by task through the roadmap, build, test, verify visually, fix and polish, commit, push, and keep this file current. Don't stop to ask. Decide, record the decision in DECISIONS.md, and continue.

## Next up
1. **M1-10** onboarding (first-run binary step; Settings → cloudflared pane), **M1-02 leftover** update check, **M1-12** E2E (`@wdio/tauri-service` on macOS CI with the fake binary; nightly real Quick Share), **M1-13** release workflow (tagging/publishing stays with the maintainer).
2. **Maintainer:** review/merge PR #1 (M0), then retarget PR #2 to `main`. Decide v0.1 signing (unsigned developer preview vs Developer ID).

## In progress
- **M1** on `milestone/m1-binary-quick-share`, draft PR #2 (stacked on #1). Done: M1-01..09 (minus update check), M1-11, Quick Share flow tests. Quick Share works end to end against the fake binary; the real cloudflared command line was verified manually (2026-09-23) and the managed install was verified against the real GitHub release.
- **M0** complete; PR #1 ready for review.

## Recently completed
- 2026-09-23: Quick Share (core, commands, UI with detected-services picker, QR, stats, log, auto-stop), menu bar share list, notifications, Overview, verified managed install (digest + checksum + Team ID), exit hook and orphan reaping, discovery.
- 2026-09-23: M1-01/03/04/05/06/07; packaged build verified at launch level.
- 2026-09-22: M0 complete. CI green on macOS/Linux/Windows.

## Blockers / maintainer actions needed
| Item | Needed by | Notes |
|---|---|---|
| Cloudflare OAuth public client + verify teispace.com | M2-04 | Redirects `http://127.0.0.1:{53682,53683,53684}/callback`. Scopes list is in `research/cloudflare.md` (TODO) |
| Test Cloudflare account/zone + API token for nightly E2E | M1-12 / M3-11 | Store as GitHub Actions secrets |
| Apple Developer ID (signing + notarization) | M6-01 (v0.1 can ship unsigned as a "developer preview") | Decide before M1-13 |

## Open questions
- v0.1 signing: unsigned developer preview vs waiting for a Developer ID? (decide at M1-13)

## Notes for the next session
- **Resume point:** branch `milestone/m0-foundations` (draft PR #1). Check `gh run list` first; fix red CI before new work. Then continue with "Next up".
- **Locked screen:** during unattended sessions the screen locks; macOS then stops compositing windows and `screencapture` returns blank content. Never send clicks/keys while locked. Use `pnpm --filter @teitunnel/desktop shoot` (WebKit) instead (D-030). Keep the Mac awake with `caffeinate -dimsu`.
- Visual verification helpers (recreate in the scratchpad if missing): a Swift `winid` tool (CGWindowList → window id), `screencapture -x -o -l <id>`, and Pillow for pixel sampling. Reference screenshots: System Settings and Finder on macOS 27.
- Toggle the appearance with `osascript -e 'tell app "System Events" to tell appearance preferences to set dark mode to true|false'`. **The maintainer uses dark mode; restore it.** Accent tests: `defaults write -g AppleAccentColor -int 3` then `defaults delete -g AppleAccentColor` to restore (the key was absent) and post `AppleColorPreferencesChangedNotification`.
- Don't automate Finder via AppleScript (it triggers an Automation permission prompt and times out).
- pnpm 12 rejects packages published < 1 day ago (D-027); pin the previous release.
- Toolchain on the maintainer machine: macOS 27.0, rustc 1.98, Node 26.9, pnpm 12.4.1.
- Stay on Tauri 2.11.x. **Do not** use `NSGlassEffectView` (D-021). Verify material work in a **packaged** build.
- Commits: Conventional Commits, **no AI attribution**. Gate every commit on `pnpm verify` (exit code, not eyeballing output).
- Push in batches: every push cancels the running CI (concurrency group), and Windows is only checked in CI.
