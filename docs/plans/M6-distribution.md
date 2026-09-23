# M6: Distribution & v1.0 (macOS)

**Goal:** a signed, notarized, auto-updating macOS app that is easy to install and easy to contribute to.
**Release:** **v1.0.0 (macOS)**.
**Exit criteria:**
- `brew install --cask teitunnel` works. The DMG opens without Gatekeeper warnings. Auto-update from v0.x → v1.0 works.
- Docs site is live. CONTRIBUTING lets a newcomer ship a fix in an afternoon.
- The whole of DESIGN §12 passes on every screen. Performance budgets are met and recorded.

---

Scope and choices: D-074 (research in `docs/research/distribution.md`). Order: M6-02 → M6-01/03 → M6-07 → maintainer setup → dry run → v0.1.0 → M6-04.

### M6-01 · Signing & notarization
- [ ] Maintainer: Developer ID Application certificate (Account Holder of the Teispace team), App Store Connect API key (notarytool), updater keypair (`pnpm tauri signer generate`); secrets in a protected `release` environment.
- [ ] Hardened runtime + entitlements (network client/server for the loopback OAuth listener; no unnecessary ones).
- [ ] macOS: universal build, sign, notarize, staple the `.dmg`.
- [ ] Windows: SignPath Foundation after v0.1.0 is public (terms need a released project): code signing policy page, MFA, roles; signing step in the workflow; updater `.sig` regenerated after Authenticode signing. v0.1.0 ships unsigned with SmartScreen guidance on the download page.

### M6-02 · Updater
- [ ] `tauri-plugin-updater` with a minisign keypair (private key in CI secrets only). `latest.json` on GitHub Releases (`releases/latest/download/latest.json`).
- [ ] UX: a quiet check on launch and daily. A native-style "Update available" in the menu and Settings; install on quit or "Restart now". Release notes shown.
- [ ] Setting to disable update checks (D-019).

### M6-03 · Release automation
- [ ] `release.yml`: matrix (macOS universal; Windows x64 + arm64 NSIS; Linux x64 + arm64 `.deb`/`.rpm`/AppImage on Ubuntu 22.04), CLI archives, SHA256SUMS, build provenance, `latest.json`; draft release, published by a final job only when every asset is uploaded; `workflow_dispatch` dry run that builds without publishing; actions pinned by commit.
- [ ] release-please (conventional commits → changelog → version bump PR → tag → the builds above in the same workflow). One version for app, CLI and image; pre-1.0 breaking changes bump the minor.
- [ ] Versioning: SemVer. v0.1.0 is the public beta.

### M6-04 · Channels (after v0.1.0)
- [ ] Website only at first (D-074). Later: Homebrew tap (`teispace/homebrew-tap`), winget, ghcr.io multi-arch image, apt/dnf repository, "Install command line tool" in the app.

### M6-07 · Download experience (website)
- [ ] Hero button by detected OS (macOS universal; Windows x64/arm64 via User-Agent Client Hints; Linux menu of `.deb`/`.rpm`/AppImage per arch); fallback lists everything without JavaScript.
- [ ] `/download` page (version, date, release notes, requirements, checksums, CLI and Docker) and a per-OS "Your download is starting" page with install steps (Slack-style), incl. the unsigned-Windows SmartScreen note.
- [ ] Code signing policy, privacy, and "Verify your download" pages (SignPath requirements).
- [ ] Site rebuilt by the release workflow so links point at the latest release; deploy to teitunnel.teispace.com (GitHub Pages + Cloudflare DNS).

### M6-05 · Polish pass
- [ ] Full DESIGN review of every screen against macOS 27 System Settings (light/dark, active/inactive, transparency slider extremes, increased contrast, reduce motion). *(Web-level pass done 2026-09-23 with `shoot` (+`SHOOT_CONTRAST=more`, `SHOOT_REDUCED_MOTION=1`): Title Case buttons, untitled Appearance group, contrast tokens. Native material checks in a packaged build need an unlocked screen.)*
- [ ] App icon (designed to the macOS icon grid, light/dark/tinted variants), About window, Help menu links. *(About (with cloudflared credit) and Help done: Teitunnel and Cloudflare Tunnel docs, Check for Problems, Export Diagnostics…, Release Notes, Report an Issue…. The icon needs a designer's asset.)*
- [x] Performance measurement (cold start, idle memory, bundle) recorded in the release notes. Regressions block the release. *(Baseline in `docs/research/performance.md`: 14.7 MB bundle, ~0.6 s cold start, ~190 MB idle with WebKit; `pnpm --filter @teitunnel/desktop perf:app` re-measures. Copy into release notes when releasing.)*
- [ ] Accessibility pass with VoiceOver. *(Automated part done: axe-core audit of every screen in light/dark × default/increased contrast, no violations (`pnpm --filter @teitunnel/desktop a11y`, D-052); Reduce Motion now reaches `motion` animations. Manual VoiceOver walk-through needs an unlocked screen.)*

### M6-06 · Docs & community
- [x] Docs site (`apps/site`, Astro Starlight, GitHub Pages) with Getting started, Concepts (routes, tunnels, run modes, changes, DNS ownership), Guides (import, observability, menu bar), Troubleshooting (the Doctor catalogue), Security, FAQ. *(D-053. Deploys once the maintainer enables Pages and sets `DEPLOY_DOCS=true`.)*
- [ ] Landing page with real screenshots and a short screen recording. *(Screenshots done on the docs home; a screen recording needs an unlocked screen.)*
- [ ] CONTRIBUTING deep-dive (architecture tour, how to add a Doctor check, how to add an origin option), `good first issue` labels, discussions enabled. *(CONTRIBUTING done, including adding an IPC command; labels and Discussions are repo settings for the maintainer.)*
