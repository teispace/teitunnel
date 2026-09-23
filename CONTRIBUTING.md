# Contributing to Teitunnel

Thanks for helping. Teitunnel aims to be the most pleasant way to use Cloudflare Tunnel, so both code quality and UI detail matter a lot here.

## Before you start
- Read [docs/README.md](docs/README.md). The key documents are ARCHITECTURE, DESIGN, CONVENTIONS and SECURITY_MODEL.
- Check [docs/STATUS.md](docs/STATUS.md) and the [roadmap](docs/ROADMAP.md) for what's being worked on. Past choices and their reasons are in [DECISIONS.md](docs/DECISIONS.md); please don't reopen one without new information.
- For anything non-trivial, open an issue or discussion first so we can agree on the approach.

## Development setup
Prerequisites: macOS 14+ (the primary platform), Rust (the version in `rust-toolchain.toml` installs itself), Node (see `.node-version`), and pnpm (the version in `package.json`'s `packageManager`; `corepack enable` provides it).

```bash
git clone https://github.com/teispace/teitunnel.git
cd teitunnel
pnpm install   # also installs the git hooks
pnpm dev       # the app, with hot reload
```

`pnpm dev` uses your real keychain and data. To try the UI without either, open the Vite dev server in a browser (`pnpm --filter @teitunnel/desktop vite`): it runs on a mock of the backend (`apps/desktop/src/dev/mock-ipc.ts`).

## Commands

| Command | What it does |
|---|---|
| `pnpm verify` | Everything CI checks locally: Biome, tsc, clippy, Vitest, nextest, cargo-deny. Must pass before every commit. |
| `pnpm check` / `pnpm test` | The checks, or the tests, alone. |
| `pnpm bindings` | Regenerates the TypeScript IPC bindings after changing a command or a type that crosses IPC. |
| `pnpm --filter @teitunnel/desktop shoot <dir> [routes…]` | Screenshots of screens in WebKit, light and dark (`SHOOT_CONTRAST=more`, `SHOOT_FILL`, `SHOOT_ACTIONS`…; see the script). |
| `pnpm --filter @teitunnel/desktop a11y` | Accessibility audit (axe-core) of every screen, light/dark × default/increased contrast. |
| `pnpm --filter @teitunnel/desktop perf` | Frame timing of the log viewer and charts under load. |
| `pnpm --filter @teitunnel/desktop perf:app [app]` | Bundle size, cold start and idle memory of a packaged build. |
| `pnpm --filter @teitunnel/desktop e2e:build && … e2e` | End-to-end tests against fake Cloudflare and fake cloudflared. |
| `pnpm --filter @teitunnel/site dev` | The docs site. |

## A tour of the code

```
crates/cf-api        Cloudflare REST client: typed requests, retries, errors
crates/cloudflared   the cloudflared binary: find/install/verify it, build its commands,
                     parse its logs and metrics, render OS service definitions
crates/core          everything the app does, with no Tauri dependency:
  engine/              observe → plan → apply → verify for every Cloudflare change
  doctor.rs            problem checks (pure) and their fixes
  machine.rs           this Mac's connectors (Session and Always-on)
  traffic.rs, …        metrics history, logs, discovery, import, settings, store
apps/cli               teitunnel-cli: the same engine from the terminal
apps/desktop/src-tauri the Tauri shell: IPC commands, menus, tray, windows. No business logic.
apps/desktop/src       the React UI: features/<name>/ with queries.ts for data
apps/site              the docs site (Astro Starlight)
tools/                 fake-cloudflared and fake-cloudflare, the test doubles
```

A request travels like this: a React component calls a hook in `features/<f>/queries.ts`, which calls a generated command in `lib/ipc/bindings.ts`. That lands in a `#[tauri::command]` in `src-tauri/src/ipc/`, which calls into `core`. Changes to Cloudflare always go through the engine: `Engine::preview` returns a plan the user reviews, and `Engine::apply` runs it with a staleness check, undoes completed steps if one fails, and records an Activity entry. When something changes, Rust emits `EntityChanged` and the UI invalidates the affected queries; nothing is mirrored by hand.

Rules that aren't negotiable (see AGENTS.md): secrets never cross IPC and live only in the keychain; processes start only through typed builders, never a shell; nothing deletes a DNS record Teitunnel didn't create without asking; IPC types are generated, never hand-written.

## How to add a Doctor check
1. **Gather the fact.** If the check needs information the Doctor doesn't collect yet, add it to `Facts`/`AccountFacts` in `crates/core/src/doctor.rs` and fill it in `gather` (or `run`). Keep gathering and judging separate.
2. **Judge it in `diagnose`** (pure): call `found.add(id, severity, subject, title, detail, evidence, fixes)`. The id is `area.problem`, e.g. `dns.missing`; the issue id adds the account and subject, so "Ignore" survives restarts. Titles say what's wrong in a sentence; details say what it means and what to do.
3. **Offer a fix if you can.** A fix that changes Cloudflare is a `Fix::Change` holding a `Change`, so it goes through the planner like everything else. If it only touches what Teitunnel owns, consider whether `fix_safe` should apply it.
4. **Test it** in `doctor.rs`'s tests: build `Facts`, call `diagnose`, assert the issue and its fix.
5. **Notifications:** errors notify once when they appear. If the problem is already announced another way, or flaps with the network, add its id to `QUIET_CHECKS` in `doctor_monitor.rs`.
6. **Document it** in `apps/site/src/content/docs/reference/doctor.md`.

## How to make an origin setting editable
Routes keep any `originRequest` settings they already have, but the only Advanced setting in the UI today is the path. To make one editable (say `noTLSVerify`):
1. Add the field to `RouteInput` (`crates/core/src/engine/views.rs`) and map it into `RouteSpec::options` in `to_intent`. Keep the settings you don't touch as they are.
2. Add planner tests (`engine/planner_tests.rs`): adding, changing and clearing the setting, and that unrelated settings survive. Review the new snapshots with `cargo insta review`.
3. Run `pnpm bindings`, then add the control to the **Advanced** disclosure in `route-sheet.tsx`, using the design system's components (see DESIGN.md), with a component test.
4. Mention it in the docs site if users need to know.

## How to add an IPC command
Write it in `apps/desktop/src-tauri/src/ipc/<area>.rs` with `#[tauri::command]` and `#[specta::specta]`, register it in `ipc.rs`, and keep it thin: validate input, call `core`, map errors to `AppError`. Run `pnpm bindings`, then call it through a hook in the feature's `queries.ts`. Types that cross IPC derive `specta::Type` behind the `specta` feature in `core`.

## Tests
Every change comes with tests (the table in [CONVENTIONS.md](docs/CONVENTIONS.md) says which kind). Snapshots use `insta`; review them, don't just accept them. Anything that talks to Cloudflare is tested against the in-memory fake (`engine/fake.rs`) or `tools/fake-cloudflare`, never a real account. UI changes need screenshots in light and dark; changes to colours or text need `a11y` to stay clean.

## Making changes
- Branch: `<type>/<task-id>-<slug>`, e.g. `feat/m1-04-log-parser`.
- Follow [CONVENTIONS.md](docs/CONVENTIONS.md) and [DESIGN.md](docs/DESIGN.md). Update the docs a change affects in the same PR.
- Commits: [Conventional Commits](https://www.conventionalcommits.org/).
- Run `pnpm verify` before pushing; the pre-commit hook runs the quick checks.

## Pull requests
- Keep them focused. Link the task ID or issue.
- Explain *why*, not only *what*. For UI changes, attach light and dark screenshots.
- CI must be green. PRs are squash-merged.

## Code of conduct
This project follows the [Contributor Covenant](CODE_OF_CONDUCT.md).
