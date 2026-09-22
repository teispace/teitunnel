# Status: live handoff

> **Read this first in every new session.** It records where the project stands and what to do next.
> Update it after **every** task (see "Docs" in [CONVENTIONS.md](CONVENTIONS.md)).

**Last updated:** 2026-09-22
**Phase:** **M0: Foundations** in progress
**Branch:** `milestone/m0-foundations` (PR #1). The prototype is archived at tag `legacy-prototype` / branch `legacy/prototype`.

## Working mode: AUTONOMOUS
The maintainer starts a session with "start" or "continue" and is then **away**. Work unattended, following [AUTONOMOUS.md](AUTONOMOUS.md): loop task by task through the roadmap, build, test, verify visually, fix and polish, commit, push, and keep this file current. Don't stop to ask. Decide, record the decision in DECISIONS.md, and continue.

## Next up
1. **M0-04 rest:** sonner Toaster (restyled), Zustand UI store.
2. **M0-06 rest:** tokens review in the gallery, reduced-motion/transparency checks, material tokens in use, **packaged-build soak test** (`pnpm build`, launch the `.app`).
3. **M0-07 primitives** → **M0-08 patterns** → **M0-09 menus/tray/⌘K** → **M0-10 store + settings** → M0-11/12 leftovers.

## In progress
- M0 on branch `milestone/m0-foundations`, draft PR #1 (https://github.com/teispace/teitunnel/pull/1).

## Recently completed
- 2026-09-22: M0-01 workspace, M0-02 crates (`Secret<T>`, redaction, cf-api envelope, cloudflared version), M0-03 Tauri shell (hidden-until-ready, single instance, window state, CSP, redacted rolling logs), M0-05 typed IPC (`app_info`, `app_ready`, `app_accent_color`, `EntityChanged`), CI workflow + cargo-deny + bundle budget + Dependabot, lefthook.
- 2026-09-22: Sidebar shell calibrated against macOS 27 System Settings/Finder (D-024, D-025). Accent colour follows the system live (D-023).
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
- Visual verification helpers (recreate in the scratchpad if missing): a Swift `winid` tool (CGWindowList → window id), `screencapture -x -o -l <id>`, and Pillow for pixel sampling. Reference screenshots: System Settings and Finder on macOS 27.
- Toggle the appearance with `osascript -e 'tell app "System Events" to tell appearance preferences to set dark mode to true|false'`. **The maintainer uses dark mode; restore it.** Accent tests: `defaults write -g AppleAccentColor -int 3` then `defaults delete -g AppleAccentColor` to restore (the key was absent) and post `AppleColorPreferencesChangedNotification`.
- Don't automate Finder via AppleScript (it triggers an Automation permission prompt and times out).
- pnpm 12 rejects packages published < 1 day ago (D-027); pin the previous release.
- Toolchain on the maintainer machine: macOS 27.0, rustc 1.98, Node 26.9, pnpm 12.4.1.
- Stay on Tauri 2.11.x. **Do not** use `NSGlassEffectView` (D-021). Verify material work in a **packaged** build.
- Commits: Conventional Commits, **no AI attribution**.
