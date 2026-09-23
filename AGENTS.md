# AGENTS.md: guide for contributors and coding agents

## Start of every session
1. Read [`docs/STATUS.md`](docs/STATUS.md). It tells you the current milestone, the next task, blockers, and notes from the last session.
2. Open the task's plan in [`docs/plans/`](docs/plans) and follow it.
3. Consult [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md), [`docs/DESIGN.md`](docs/DESIGN.md), [`docs/CONVENTIONS.md`](docs/CONVENTIONS.md) and [`docs/SECURITY_MODEL.md`](docs/SECURITY_MODEL.md) as needed. The reasoning behind past choices is in [`docs/DECISIONS.md`](docs/DECISIONS.md).

## Autonomous sessions
When the maintainer says "start" or "continue", follow [`docs/AUTONOMOUS.md`](docs/AUTONOMOUS.md): loop through tasks unattended (implement → validate → verify visually → commit → update docs), without stopping to ask.

## After every task (mandatory)
1. Tick the task in its plan file and in `docs/ROADMAP.md`.
2. Update `docs/STATUS.md`: next up, in progress, recently completed, blockers, notes for the next session.
3. Add new decisions to `docs/DECISIONS.md`, and newly verified external facts (with source and date) to `docs/research/`.
4. If reality diverged from ARCHITECTURE/DESIGN, update them in the same change.

## Non-negotiables
- **Secrets never cross IPC** and are only stored in the OS keychain. Tunnel run tokens are never passed in argv.
- **No shells.** Processes are started only through typed builders in `crates/cloudflared` (`tokio::process::Command` with discrete args).
- **All Cloudflare mutations go through the plan → apply engine.** Nothing auto-deletes DNS records Teitunnel doesn't own.
- **`src-tauri` has no business logic.** `core`, `cf-api` and `cloudflared` don't depend on Tauri.
- **IPC types are generated** (tauri-specta). Never hand-write them.
- **Native design rules** in DESIGN.md: no gradients, fake glass cards, emoji, or hand cursors, and semantic tokens only. Verify materials in a packaged build.
- **Tests with every change** (see the table in CONVENTIONS.md).
- **Commits:** Conventional Commits. **No AI/assistant attribution** in commits, PRs, or branch names.

## Layout
```
crates/cf-api        Cloudflare REST client
crates/cloudflared   cloudflared binary: locate/install/verify, commands, log/metrics parsing
crates/core          domain, engine (observe→plan→apply→verify), runtime, discovery, doctor, store
apps/cli             teitunnel-cli: routes and exports from the terminal
apps/desktop         Tauri shell (src-tauri) + React UI (src)
apps/web             website: landing page and docs (Next.js + Fumadocs, static export)
tools/fake-cloudflared  test double
docs/                all project documentation
```

## Commands
Keep this section in sync with `package.json`.
- `pnpm install`: install JS deps and git hooks (lefthook)
- `pnpm dev`: run the desktop app in dev mode
- `pnpm check`: biome + tsc + rustfmt + clippy
- `pnpm test`: vitest + cargo nextest
- `pnpm bindings`: regenerate IPC bindings
- `pnpm build`: packaged app (use it to verify materials and native behaviour)
- `pnpm --filter @teitunnel/desktop shoot <dir> [routes…]`: WebKit screenshots in light and dark (D-030); add `?platform=windows` or `?platform=linux` to a route to preview that platform's chrome, `?clean` to hide the developer section (website screenshots)
- `pnpm --filter @teitunnel/web dev` / `build`: the website (landing page + docs, `apps/web`, static export in `out/`, for teitunnel.teispace.com)
- `pnpm --filter @teitunnel/desktop perf [seconds]`: frame timing of the log viewer and charts under load, in WebKit (D-051)
- `pnpm --filter @teitunnel/desktop a11y [routes…]`: axe-core accessibility audit of every screen, light/dark × default/increased contrast (D-052)
- `pnpm --filter @teitunnel/desktop perf:app [app] [runs]`: bundle size, cold start and idle memory of a packaged build (docs/research/performance.md)
- Before every commit: `pnpm verify` (check + test + cargo-deny) must exit 0.
