# Engineering Conventions

These apply to humans and agents alike. CI enforces what it can. Review enforces the rest.

## General
- **Small, focused changes.** One task ID (e.g. `M1-04`) per PR where practical.
- **DRY with judgement:** extract on the third repetition, or immediately if the logic is security- or correctness-critical.
- **No dead code, commented-out code, or speculative abstractions.** Build for the current milestone, and leave room for the next via clean boundaries, not stubs.
- **Every behaviour change ships with tests** (see Testing) and **doc updates** (see Docs).

## Rust
- Edition 2024, stable toolchain pinned in `rust-toolchain.toml`. Workspace-level `[workspace.lints]`:
  - `clippy::all = warn` (denied in CI via `-D warnings`), `clippy::pedantic` selectively.
  - `unwrap_used`, `expect_used`, `panic` = deny outside tests. `todo`/`unimplemented` = deny.
  - `unsafe_code = forbid` (except in the platform module, with a `// SAFETY:` justification).
- **Errors:** `thiserror` enums per crate. `core` maps them into `AppError` (code + message + hint). No `anyhow` in library crates. No stringly-typed errors.
- **Async:** Tokio. Never block in async. Use `spawn_blocking` for SQLite, keychain and heavy parsing. Every spawned task is owned (JoinSet / handle) and cancellable (`CancellationToken`).
- **Types over strings:** newtypes for ids (`TunnelId`, `ZoneId`, `RouteId`), validated `Hostname`, `Port`, `OriginUrl`. Parse, don't validate: construction is the validation.
- **Secrets:** wrap them in `Secret<String>` (redacting `Debug`). Never log or serialize them to IPC.
- **Processes:** only via `crates/cloudflared` builders. `Command` with discrete args. No shells.
- **Serialization:** `#[serde(rename_all = "camelCase")]` on IPC types. `#[derive(specta::Type)]` for anything crossing IPC.
- **Modules:** one concept per file, `mod.rs`-free layout (`foo.rs` + `foo/`). Public API is re-exported from `lib.rs`, and everything else is `pub(crate)`.
- **Docs:** `///` on every public item of `cf-api`, `cloudflared` and `core`.

## TypeScript / React
- TypeScript strict, plus `noUncheckedIndexedAccess`, `exactOptionalPropertyTypes`, `noImplicitOverride`. No `any`; use `unknown` and narrow. No non-null `!` except on documented invariants.
- **Biome** for lint + format (config at repo root). Imports are sorted automatically.
- Filenames: `kebab-case.tsx`. Components: `PascalCase`. Hooks: `useThing`. One component per file (tiny private helpers are allowed).
- ~200–250 lines per file as a guideline. Split when a component gets a second responsibility.
- **Data:** only through `lib/ipc` + TanStack Query hooks in `features/<f>/queries.ts`. Never call `invoke` directly from components.
- **State:** server state in Query; UI state in Zustand/URL; form state in react-hook-form. Don't mirror server data into Zustand.
- **Styling:** Tailwind utilities mapped to semantic tokens (`bg-surface-content`, `text-secondary`). No raw hex, arbitrary colours, or `style={{}}` for static values. `cn()` for conditional classes; `cva` for variants.
- **Accessibility:** Radix primitives for interactive widgets. `aria-label` on icon-only controls.
- **Text:** every user-visible string (labels, `aria-label`s, placeholders, toasts) comes from `t("area.key")` with the English text in `locales/en.json` (D-061). Placeholders are `{name}`; counts use `_one`/`_other` keys and `{count}`. Module-level label maps hold `MessageKey`s and call `t()` at render, since the language loads before the first render, not at import. Write whole sentences as one message; never concatenate translated fragments.
- **Platform wording (D-063):** a message that names a macOS thing (this Mac, Finder, keychain, System Settings, the menu bar, ⌘, Homebrew) needs `key@windows` and `key@linux` wordings; the catalog test enforces it.
- **Text from Rust (D-062):** never build a sentence for the user in Rust. Add it to `locales/en.json` under `core`, create it with the generated `text::msg::…` function, and return the `Text`; the UI translates it with `translate()`. Errors implement `UserText` and derive `Display` from it (English, for logs). Technical values go in as arguments (`msg::raw` for text shown as it is).
- **No `useEffect` for data fetching or derived state.** Effects only sync with external systems (events, DOM).

## Naming
- IPC commands: `<area>_<verb>` (`routes_plan_add`). Events: `PascalCase` types (`EntityChanged`).
- A command that touches the disk, other processes, the keychain or a system service is `async` (blocking work through `ipc::off_main`), and so is one that builds a window: synchronous commands run on the main thread, where they freeze every window on macOS and deadlock WebView2 on Windows. Synchronous commands only read memory.
- Query keys: `['routes', accountId]`, built by `lib/ipc/query-keys.ts` only.
- Branches: `<type>/<task-id>-<slug>` (e.g. `feat/m1-04-log-parser`).

## Testing
| Change | Required tests |
|---|---|
| Parser / pure function | Unit tests plus fixture corpus. Property tests where the input space is large. |
| Planner | `insta` snapshot per scenario, plus idempotency property (`plan(apply(plan)) == ∅`) |
| cf-api endpoint | `wiremock` test with a recorded response, plus the error-envelope case |
| Supervisor / runtime | Integration test with `tools/fake-cloudflared` |
| UI component (primitive/pattern) | Gallery entry. Vitest test if it has logic. |
| UI flow | Vitest + `mockIPC` flow test. E2E for critical paths (onboarding, add route, quick share). |
| Bug fix | A regression test that fails before the fix |

- Tests must be deterministic: no real network (except the nightly real-account job), no real clock (`Clock` port), and no sleeps. Use time control or events.
- The coverage goal is meaningful rather than a number. Planner, parsers and cf-api stay at or above 90% line coverage.

## Commits & PRs
- **Conventional Commits:** `feat(routes): add wildcard hostnames`, `fix(runtime): …`, `docs: …`, `chore: …`, `test: …`, `refactor: …`, `perf: …`.
- **No AI/assistant attribution** in commits, PR titles/bodies, or branch names.
- The PR description links the task ID(s), explains *why*, lists test evidence, and includes screenshots (light + dark) for UI.
- CI must be green. Squash merge to `main`.

## Docs (keep the project resumable)
After completing any task:
1. Tick the task in the milestone plan (`docs/plans/Mx-*.md`) and in `docs/ROADMAP.md` if it's an epic-level item.
2. Update `docs/STATUS.md`: current task, last completed, next up, blockers, and notes for the next session.
3. Record any new decision in `docs/DECISIONS.md`. Update `ARCHITECTURE.md`/`DESIGN.md` if reality changed.
4. Record newly verified external facts in `docs/research/*` with source and date.
