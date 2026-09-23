# M6: Distribution & v1.0 (macOS)

**Goal:** a signed, notarized, auto-updating macOS app that is easy to install and easy to contribute to.
**Release:** **v1.0.0 (macOS)**.
**Exit criteria:**
- `brew install --cask teitunnel` works. The DMG opens without Gatekeeper warnings. Auto-update from v0.x → v1.0 works.
- Docs site is live. CONTRIBUTING lets a newcomer ship a fix in an afternoon.
- The whole of DESIGN §12 passes on every screen. Performance budgets are met and recorded.

---

### M6-01 · Signing & notarization
- [ ] Apple Developer ID (maintainer prerequisite). CI secrets: certificate p12, password, App Store Connect API key for notarytool.
- [ ] Hardened runtime + entitlements (network client/server for the loopback OAuth listener; no unnecessary ones).
- [ ] `release.yml`: build a universal binary (or separate arm64/x64 builds), sign, notarize, staple, then a DMG with a designed background and Applications link.

### M6-02 · Updater
- [ ] `tauri-plugin-updater` with a minisign keypair (private key in CI secrets only). `latest.json` on GitHub Releases.
- [ ] UX: a quiet check on launch and daily. A native-style "Update available" in the menu and Settings; install on quit or "Restart now". Release notes shown.
- [ ] Setting to disable update checks (D-019).

### M6-03 · Release automation
- [ ] release-please (conventional commits → changelog → version bump PR → tag → release workflow).
- [ ] Versioning: SemVer. Pre-1.0 minors are milestone releases.

### M6-04 · Channels
- [ ] Homebrew: own tap `teispace/homebrew-tap` first (cask auto-bumped by CI), then submit to homebrew/cask once notability criteria are met.
- [ ] GitHub Releases as the canonical source.

### M6-05 · Polish pass
- [ ] Full DESIGN review of every screen against macOS 27 System Settings (light/dark, active/inactive, transparency slider extremes, increased contrast, reduce motion). *(Web-level pass done 2026-09-23 with `shoot` (+`SHOOT_CONTRAST=more`, `SHOOT_REDUCED_MOTION=1`): Title Case buttons, untitled Appearance group, contrast tokens. Native material checks in a packaged build need an unlocked screen.)*
- [ ] App icon (designed to the macOS icon grid, light/dark/tinted variants), About window, Help menu links.
- [ ] Performance measurement (cold start, idle memory, bundle) recorded in the release notes. Regressions block the release.
- [ ] Accessibility pass with VoiceOver. *(Automated part done: axe-core audit of every screen in light/dark × default/increased contrast, no violations (`pnpm --filter @teitunnel/desktop a11y`, D-052); Reduce Motion now reaches `motion` animations. Manual VoiceOver walk-through needs an unlocked screen.)*

### M6-06 · Docs & community
- [ ] Docs site (`apps/site`, Astro Starlight or similar, deployed via GitHub Pages) with Getting started, Concepts (routes, tunnels, run modes), Troubleshooting (mirrors the Doctor catalogue), Security, FAQ.
- [ ] Landing page with real screenshots and a short screen recording.
- [ ] CONTRIBUTING deep-dive (architecture tour, how to add a Doctor check, how to add an origin option), `good first issue` labels, discussions enabled.
