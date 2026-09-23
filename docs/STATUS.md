# Status: live handoff

> **Read this first in every new session.** It records where the project stands and what to do next.
> Update it after **every** task (see "Docs" in [CONVENTIONS.md](CONVENTIONS.md)).

**Last updated:** 2026-09-23
**Phase:** **M6 (non-release parts)** in progress on `milestone/m6-polish-docs` (stacked on #6). The maintainer deferred release work (signing, notarization, updater, release automation, Homebrew) on 2026-09-23.
**Branch:** `milestone/m5-observability-always-on` (stacked on #5 → #4 → #3 → #2 → #1). The prototype is archived at tag `legacy-prototype` / branch `legacy/prototype`.

## Working mode: AUTONOMOUS
The maintainer starts a session with "start" or "continue" and is then **away**. Work unattended, following [AUTONOMOUS.md](AUTONOMOUS.md): loop task by task through the roadmap, build, test, verify visually, fix and polish, commit, push, and keep this file current. Don't stop to ask. Decide, record the decision in DECISIONS.md, and continue.

## Next up
0. **M10/M11 done** ([plan](plans/M10-parity.md), Cloudify analysis in [research/cloudify.md](research/cloudify.md)). Small follow-ups: listing terminal-started trycloudflare shares in the app; per-endpoint load-balancer health. Next: release work (signing with the maintainer's Developer ID, updater, release automation, channels incl. ghcr.io image and winget/Homebrew, website deploy), done last per the maintainer (2026-09-23).
1. M7 Windows / M8 Linux: what's left needs a real Windows 11 machine or Linux desktop (see the plans: try Always-on, the look, tray, notifications; real Mica). Maintainer decision still open: Authenticode check (M7-02).
2. Distribution for Windows/Linux is deferred with the other release work.
3. M9 advanced features (`docs/plans/M9-advanced.md`): Export, CLI and Access done (incl. Doctor `access.orphan`); remote connectors and logs done (M9-04); CLI extras (`share`, `doctor`, completions) done; private networks and `cloudflared access` helpers (M9-05) done; M9-06 i18n: UI and Rust-side text done (D-061, D-062); community translations remain (needs translators).
4. M6 leftovers needing an unlocked screen or the maintainer: VoiceOver walk-through, native material checks in a packaged build, screen recording, app icon (designer), Pages/labels/Discussions (repo settings).
5. **Deferred by the maintainer:** M6-01 signing/notarization, M6-02 updater, M6-03 release automation, M6-04 Homebrew/channels.
6. **Maintainer:** review/merge PRs #1–#7 in order; OAuth client; test token; enable Pages + `DEPLOY_DOCS=true` for the docs site.

## In progress
- **M6 (non-release)** on `milestone/m6-polish-docs`, draft PR #7: polish (Title Case, contrast tokens, Reduce Motion, Help menu), a11y audit (D-052), performance baseline, docs site (D-053), CONTRIBUTING.
- **M5** complete on `milestone/m5-observability-always-on`, PR #6 (ready for review). Done: Always-on via launchd (gapless switch, token file, log tailing, real-launchd nightly test, D-045). Earlier (on M4): Activity timeline, connector logs, traffic sparkline, routes in the menu bar.
- **M4** PR #5, **M3** PR #4 ready for review. **M2** PR #3, **M1** PR #2, **M0** PR #1.

## Recently completed
- 2026-09-23: M11 website (D-073): `apps/web` (Next.js + Fumadocs, static): landing page with real app screenshots, all docs migrated and reworded for every platform, new Accounts and permissions and Server API pages, static search; Astro site removed; Docs workflow builds it (deploy with the release work). Also M10-02 adopt an existing tunnel (D-072).
- 2026-09-23: M10-07 load balancing across machines (D-071): monitor, pool of tunnel endpoints and load balancer through plan → apply with full undo; routes join/leave pools; app, CLI `route balance|unbalance`, dashboard; guide.
- 2026-09-23: M10-06 server dashboard and API (D-070): `teitunnel-cli serve` (tunnels + locked-down web dashboard + preview/apply API with OpenAPI), argon2id password, API keys, rate limiting, CSP; E2E over raw HTTP.
- 2026-09-23: M10-04/05 headless and containers (D-069): API token from the environment (never stored), `setup`, `up`, `always-on on|off|status` with sandboxed systemd system units as root, `routes --check`, CLI says "this machine"; Docker image + Compose recipe, CI build and smoke test; site guide Servers and containers (VPS, AWS, Azure, GCP, Docker, Compose, Kubernetes).
- 2026-09-23: M10-03 share on your domain (D-068): temporary routes (migration 9) with owner and expiry, swept every 30 s, at launch and on quit; Quick Share "Address" menu, domain share cards, Temporary badge in Routes, CLI `share --on`. Test timeouts raised for loaded machines.
- 2026-09-23: M10-02 several tunnels per machine (D-067): `local_tunnels` (migration 8), `Context.tunnel`, `Intent::CreateTunnel`, one hostname per machine, merged overview with per-route tunnel and health, per-tunnel Always-on/drift/resume/Doctor, Tunnels view New tunnel + tunnel picker in New Route, CLI `tunnels`/`tunnel create|delete`/`--tunnel`, site guide. Adopting an existing account tunnel is still open.
- 2026-09-23: M10-01 fix in place (D-066): `PermissionFix` for every permission gap (route sheet before review and on refusal, Doctor, private networks, connector logs, account and domain permission lists), `ZeroTrustFix`, links for "add a domain" and foreign Access apps, every core error classified with a completeness test. Token template asks for Access up front (D-065). Cloudify analysed; M10/M11 planned.
- 2026-09-23: Access permission fixed in place (D-064): a route needing a login on a token without Access shows how to add the two permissions to the existing token (opens the dashboard's token list; the token value doesn't change), re-checks when the window regains focus, and continues the form or re-runs a refused review; cert/OAuth accounts (or anyone) can paste a new token with login permissions instead. `accessEdit` now needs both Access permissions. Also: CI tests no longer assume macOS wording, port tests have their own ranges, CI runs all tests without fail-fast.
- 2026-09-23: M7/M8 shell pass (D-063): opaque windows with native title bars on Windows/Linux, solid platform surfaces and fonts, neutral navigation selection (Windows accent pill), no menu bar there (Ctrl shortcuts matched by physical key, Help and Quit in the palette), tray left-click opens the app on Windows, closing quits when the tray icon is hidden, `key@windows`/`key@linux` wording for macOS terms (UI and core) with a coverage test, winget advice on Windows. Preview any platform in dev with `?platform=windows|linux` (shoot).
- 2026-09-23: M9-06 Rust-side text: `Text { key, args }` from the core with typed constructors generated from `locales/en.json` (`core.*`), translated by the UI; errors, plan, activity, Doctor, verify, menus (incl. macOS predefined items), tray and notifications (D-062). Catalogs moved to `locales/`. Fixed: the Quick Share service list closed the moment the field was clicked (Radix treated the field as outside the list).
- 2026-09-23: M9-06 i18n (UI): every user-visible string in the React UI moved to typed JSON catalogs with `t()` (plurals via `Intl.PluralRules`, locale-aware numbers, lists and durations), system language with per-message English fallback, catalog validation test, `i18n:missing` script, translator guide (D-061). Durations now read "12 sec", "1 hr 5 min" (Intl units).
- 2026-09-23: M9-05 private networks: share a CIDR range with WARP clients through this Mac's tunnel (engine steps with rollback, default virtual network, conflicts/overlaps/public-range confirmation), Tunnels ▸ Private Networks + sheet, CLI `network add/remove` and `networks`, Doctor WARP checks (`network.excluded`, `network.not_included`, `network.proxy_off`), `cloudflared access` commands for SSH/RDP/SMB/TCP routes (no HTTPS verify for them), docs guide (D-060). fake-cloudflare serves teamnet endpoints.
- 2026-09-23: M9-02 CLI extras: `share` (Quick Share for the command's lifetime, reaping of a killed CLI's connector), `doctor` (checks, safe fixes, exit codes), shell completions (D-059). Fixed: concurrent processes could pick the same metrics port (allocation now spread by pid).
- 2026-09-23: M9-04 remote connectors: machines per tunnel with This Mac marked, live logs of any connector through Cloudflare's management relay, Doctor warning when another machine runs this Mac's tunnel; replicas deliberately left to Export (D-058).
- 2026-09-23: M9-03 Require a login (Cloudflare Access): engine steps with rollback, ownership index, Access read only when needed, verify recognizes the login redirect, route sheet + list + inspector, CLI `--allow`, docs guide (D-057). Fixed: views using charts crashed on WebKitGTK under `LANG=C` (uPlot passed the invalid `navigator.language` to `Intl`); the app now repairs the language at startup (Linux E2E failure).
- 2026-09-23: M9-02 `teitunnel-cli` (routes, route add/remove with plan + confirmation, export, --json) (D-056).
- 2026-09-23: M9-01 Export (config.yml, Docker Compose, Terraform v5 with import blocks) (D-055). M7/M8 groundwork: services per platform, no console windows, Secret Service error, E2E jobs for Linux/Windows (D-054). Fixed: bare IPv6 origins.
- 2026-09-23: M6 non-release work: accessibility (WCAG AA under Increase Contrast, axe audit clean), Help menu + About credits, performance baseline (14.7 MB, ~0.6 s start), docs site (apps/site), CONTRIBUTING deep-dive; dev mocks and screenshots use neutral names.
- 2026-09-23: M5 complete. Service adapters for systemd/Task Scheduler behind a neutral `ServiceSpec`; exit criteria measured with `pnpm --filter @teitunnel/desktop perf` (60 fps under load, D-051).
- 2026-09-23: M5-03 complete: virtualized log viewer, Save to Downloads (redacted).
- 2026-09-23: M5-03 per-route logs (backend filter by rule + service), bounded and tail-read Always-on log files (D-050).
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
| Apple Developer ID (signing + notarization) | M6-01 | The maintainer has a Developer ID and will set up signing; release work is done last (2026-09-23) |

## Open questions
- Windows Authenticode check of cloudflared: allow a small isolated crate with audited `unsafe` (WinVerifyTrust), or rely on the release checksums on Windows? (M7-02)
- v0.1 signing: unsigned developer preview (ready: DMG + Gatekeeper instructions in the release notes) vs waiting for a Developer ID?

## Notes for the next session
- **Resume point:** branch `milestone/m6-polish-docs` (M9 work stacks here). M9-05 and M9-06 (except community translations) are committed. Next: M7/M8 platform work, or maintainer items; see the plan. When adding user text in Rust, follow CONVENTIONS (Text from Rust). Follow-ups from M9-05: other virtual networks are read but not managed; the real-edge behaviour of an HTTP probe against SSH routes was not checked (verify is now skipped for them).
- Previous resume point: branch `milestone/m5-observability-always-on`. Check `gh run list` first; fix red CI before new work. Then continue with "Next up".
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
- The build cache fills the disk (twice on 2026-09-23): `target/debug/deps` reached 48 GB of stale artifacts and `incremental` 7.5 GB. When `df -h /` gets low: `rm -rf target/debug/{deps,build,.fingerprint,incremental} target/e2e` (keeps the maintainer's `target/debug/Teitunnel`; the next build is a full one).
