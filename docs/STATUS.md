# Status: live handoff

> **Read this first in every new session.** It records where the project stands and what to do next.
> Update it after **every** task (see "Docs" in [CONVENTIONS.md](CONVENTIONS.md)).

**Last updated:** 2026-09-22
**Phase:** Planning complete → starting **M0: Foundations**
**Branch:** `main`. The prototype is archived at tag `legacy-prototype` / branch `legacy/prototype`.

## Next up
1. **M0-01 Workspace skeleton** ([plan](plans/M0-foundations.md#m0-01--workspace-skeleton))
2. Then M0-02 → M0-12 in order. M0-06/07/08 (design) deserve the most care: screenshot-compare against macOS 27 System Settings.

## In progress
- Nothing.

## Recently completed
- 2026-09-22: Analysed the prototype and decided to rewrite (D-001).
- 2026-09-22: Research on Cloudflare API, cloudflared endpoints and flags, OAuth, Tauri/macOS 27, and library versions (`research/`).
- 2026-09-22: Full docs set: VISION, ARCHITECTURE, DESIGN, SECURITY_MODEL, CONVENTIONS, DECISIONS (D-001…D-022), ROADMAP, plans M0–M9.
- 2026-09-22: Repo reset (prototype removed from `main`), repo prepared for public release.

## Blockers / maintainer actions needed
| Item | Needed by | Notes |
|---|---|---|
| Cloudflare OAuth public client + verify teispace.com | M2-04 | Redirects `http://127.0.0.1:{53682,53683,53684}/callback`. Scopes list is in `research/cloudflare.md` (TODO) |
| Test Cloudflare account/zone + API token for nightly E2E | M1-12 / M3-11 | Store as GitHub Actions secrets |
| Apple Developer ID (signing + notarization) | M6-01 (v0.1 can ship unsigned as a "developer preview") | Decide before M1-13 |

## Open questions
- v0.1 signing: unsigned developer preview vs waiting for a Developer ID? (decide at M1-13)

## Notes for the next session
- Toolchain on the maintainer machine: macOS 27.0, rustc 1.98, Node 26.9, pnpm 12.4.1.
- Stay on Tauri 2.11.x (not 3 alpha). Pin tauri-specta rc exactly.
- **Do not** use `NSGlassEffectView` / `tauri-plugin-liquid-glass` (crashes packaged builds on macOS 27; D-021).
- Verify visual/material work in a **packaged** build, not only `tauri dev`.
- Commits: Conventional Commits, **no AI attribution**.
