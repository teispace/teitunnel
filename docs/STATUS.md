# Status: live handoff

> **Read this first in every new session.** It records where the project stands and what to do next.
> Update it after **every** task (see "Docs" in [CONVENTIONS.md](CONVENTIONS.md)).

**Last updated:** 2026-09-22
**Phase:** **M0: Foundations** in progress
**Branch:** `milestone/m0-foundations` (PR #1). The prototype is archived at tag `legacy-prototype` / branch `legacy/prototype`.

## Working mode: AUTONOMOUS
The maintainer starts a session with "start" or "continue" and is then **away**. Work unattended, following [AUTONOMOUS.md](AUTONOMOUS.md): loop task by task through the roadmap, build, test, verify visually, fix and polish, commit, push, and keep this file current. Don't stop to ask. Decide, record the decision in DECISIONS.md, and continue.

## Next up
1. **Maintainer:** review/merge PR #1 (M0), retarget PR #2 (M1) to `main` and merge; decide v0.1 signing (unsigned developer preview is ready to go), then push tag `v0.1.0` — `release.yml` builds a universal DMG into a **draft** release for review.
2. With the screen unlocked: native captures of the packaged app (M0-06 soak test), check the menu bar extra and notifications visually.
3. **M2** (accounts & domains) can start without the maintainer up to M2-04 (OAuth needs the registered client): cf-api foundation (M2-01), accounts/zones (M2-02), capabilities (M2-03), token flow (M2-05), cert.pem import (M2-06), account store (M2-07), connect UI (M2-08), domains view (M2-09). A test API token is needed for the nightly real-account job.

## In progress
- **M2** on `milestone/m2-accounts-domains`, draft PR #3 (stacked on #2): feature-complete except items needing the maintainer — OAuth client registration (the flow is built, tested and hidden until `CLIENT_ID` is set), real-account fixtures (needs a test token). Next: **M3 routes engine** on a branch stacked on M2.
- **M1** complete; PR #2 ready for review. **M0** complete; PR #1 ready for review.

## Recently completed
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
- **Resume point:** branch `milestone/m0-foundations` (draft PR #1). Check `gh run list` first; fix red CI before new work. Then continue with "Next up".
- **Locked screen:** during unattended sessions the screen locks; macOS then stops compositing windows and `screencapture` returns blank content. Never send clicks/keys while locked. Use `pnpm --filter @teitunnel/desktop shoot` (WebKit) instead (D-030). Keep the Mac awake with `caffeinate -dimsu`.
- Visual verification helpers (recreate in the scratchpad if missing): a Swift `winid` tool (CGWindowList → window id), `screencapture -x -o -l <id>`, and Pillow for pixel sampling. Reference screenshots: System Settings and Finder on macOS 27.
- Toggle the appearance with `osascript -e 'tell app "System Events" to tell appearance preferences to set dark mode to true|false'`. **The maintainer uses dark mode; restore it.** Accent tests: `defaults write -g AppleAccentColor -int 3` then `defaults delete -g AppleAccentColor` to restore (the key was absent) and post `AppleColorPreferencesChangedNotification`.
- Don't automate Finder via AppleScript (it triggers an Automation permission prompt and times out).
- pnpm 12 rejects packages published < 1 day ago (D-027); pin the previous release.
- Toolchain on the maintainer machine: macOS 27.0, rustc 1.98, Node 26.9, pnpm 12.4.1.
- Stay on Tauri 2.11.x. **Do not** use `NSGlassEffectView` (D-021). Verify material work in a **packaged** build.
- Commits: Conventional Commits, **no AI attribution**. Gate every commit on `pnpm verify` (exit code, not eyeballing output).
- cf-api retries every method on 5xx; before adding POST creates in M3, add a no-retry path for non-idempotent requests.
- Push in batches: every push cancels the running CI (concurrency group), and Windows is only checked in CI.
