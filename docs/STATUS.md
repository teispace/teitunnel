# Status: live handoff

> **Read this first in every new session.** It records where the project stands and what to do next.
> Update it after **every** task (see "Docs" in [CONVENTIONS.md](CONVENTIONS.md)).

**Last updated:** 2026-09-22
**Phase:** **M0: Foundations** in progress
**Branch:** `milestone/m0-foundations` (PR #1). The prototype is archived at tag `legacy-prototype` / branch `legacy/prototype`.

## Working mode: AUTONOMOUS
The maintainer starts a session with "start" or "continue" and is then **away**. Work unattended, following [AUTONOMOUS.md](AUTONOMOUS.md): loop task by task through the roadmap, build, test, verify visually, fix and polish, commit, push, and keep this file current. Don't stop to ask. Decide, record the decision in DECISIONS.md, and continue.

## Next up
1. **M0-06 packaged-build soak test** (needs an unlocked screen: native captures of the `.app` in light/dark, active/inactive, resize storm, sleep/wake). Launch-level checks were done unattended; see notes.
2. Merge PR #1 once CI is green and the soak test passes, then start **M1** (`milestone/m1-binary-quick-share`, plan `plans/M1-binary-quick-share.md`). M1 work that doesn't need the screen can start on its own branch while #1 waits.

## In progress
- M0 on branch `milestone/m0-foundations`, draft PR #1 (https://github.com/teispace/teitunnel/pull/1). All M0 tasks are done except the soak test.

## Recently completed
- 2026-09-22: M0-07 primitives (Radix-based, measured against macOS 27), M0-08 patterns (SplitView, ListPane, Inspector, KeyValueGrid, CopyField, GroupedList, Error/EmptyState, resizable/collapsible sidebar), M0-09 menu bar + tray + ⌘K palette, M0-10 SQLite store + typed settings + Settings window, M0-12 hygiene (lefthook, README development section, PR template). Screenshots in `docs/screenshots/M0-0x/`.
- 2026-09-22: M0-01/02/03/05, CI (green on macOS/Linux/Windows), cargo-deny, bundle budget, Dependabot.
- 2026-09-22: Analysis, research, full docs set, repo reset and made public.

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
- Commits: Conventional Commits, **no AI attribution**.
