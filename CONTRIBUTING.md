# Contributing to Teitunnel

Thank you for your interest in Teitunnel. Every contribution helps, whether it's a bug
report, a translation, a docs fix or a new feature. This guide explains how to take part.

- [Ways to contribute](#ways-to-contribute)
- [Before you start](#before-you-start)
- [Development setup](#development-setup)
- [A tour of the code](#a-tour-of-the-code)
- [Making a change](#making-a-change)
- [Pull requests](#pull-requests)
- [Recipes](#recipes)

## Ways to contribute

- **Report a bug** with the [bug form](https://github.com/teispace/teitunnel/issues/new?template=bug_report.yml).
  Include the version, your OS and the steps; **Export Diagnostics** (the Help menu on
  macOS, the command palette on Windows and Linux) gives a redacted bundle you can attach.
- **Suggest an idea** in [Discussions ▸ Ideas](https://github.com/teispace/teitunnel/discussions/categories/ideas).
  Once there's agreement, it becomes an issue.
- **Answer questions** in [Discussions ▸ Q&A](https://github.com/teispace/teitunnel/discussions/categories/q-a).
- **Improve the docs** in `apps/web/content/docs` (the website) or `docs/` (for contributors).
- **Translate** the app (see [Translating Teitunnel](#translating-teitunnel)).
- **Write code**: issues labelled
  [`good first issue`](https://github.com/teispace/teitunnel/labels/good%20first%20issue) and
  [`help wanted`](https://github.com/teispace/teitunnel/labels/help%20wanted) are a good start.

Security vulnerabilities are never reported in public: see [SECURITY.md](SECURITY.md).

## Before you start

- **Small fixes** (typos, obvious bugs, docs): open a pull request directly.
- **Anything larger** (a feature, a behaviour change, a new dependency, a refactor across
  crates): open an issue or an Ideas discussion first, so we can agree on the approach before
  you spend time on it. Comment on an issue to say you're working on it.
- **Design changes** that affect the architecture, the security model or the user experience
  are discussed in an issue labelled `proposal` before implementation.
- Read the contributor docs in [`docs/`](docs/README.md), in particular
  [ARCHITECTURE](docs/ARCHITECTURE.md), [CONVENTIONS](docs/CONVENTIONS.md) and, for UI work,
  [DESIGN](docs/DESIGN.md).

Teitunnel has a few rules that aren't negotiable, because users trust it with their
Cloudflare account:

- **Secrets never cross IPC** and are stored only in the OS keychain. Tunnel tokens are
  never passed on a command line.
- **No shells.** Processes start only through the typed builders in `crates/cloudflared`.
- **Every Cloudflare change goes through the plan → apply engine**, and nothing deletes a
  DNS record Teitunnel didn't create without asking.
- **IPC types are generated** (tauri-specta); never edit `bindings.ts` by hand.

## Development setup

You need:

- **Rust**: the toolchain in `rust-toolchain.toml` installs itself through
  [rustup](https://rustup.rs). Also `cargo install cargo-nextest cargo-deny --locked`.
- **Node** (the version in `.node-version`) and **pnpm** (the version in `package.json`'s
  `packageManager`; `corepack enable` provides it).
- **Platform libraries**:
  - macOS 14 or later: Xcode Command Line Tools (`xcode-select --install`).
  - Windows 10 or 11: Visual Studio Build Tools with "Desktop development with C++", and
    WebView2 (included in Windows 11).
  - Linux (Debian or Ubuntu):
    `sudo apt install libwebkit2gtk-4.1-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev build-essential`.

Then:

```sh
git clone https://github.com/teispace/teitunnel.git
cd teitunnel
pnpm install   # dependencies and git hooks
pnpm dev       # the app, with hot reload
```

`pnpm dev` uses your real keychain and data. To work on the UI without either, run the Vite
dev server in a browser (`pnpm --filter @teitunnel/desktop vite`): it runs against a mock of
the backend (`apps/desktop/src/dev/mock-ipc.ts`). Development builds also have a component
gallery under **Developer ▸ Gallery**.

### Commands

| Command | What it does |
|---|---|
| `pnpm verify` | Everything CI checks: Biome, tsc, rustfmt, clippy, Vitest, nextest and cargo-deny. Run it before you push. |
| `pnpm check` / `pnpm test` | The checks, or the tests, alone. |
| `pnpm fmt` | Formats everything. |
| `pnpm bindings` | Regenerates the TypeScript IPC bindings after changing a command or a type that crosses IPC. |
| `pnpm build` | A packaged app, to check native behaviour. |
| `pnpm --filter @teitunnel/desktop shoot <dir> [routes…]` | Screenshots of screens in WebKit, light and dark. |
| `pnpm --filter @teitunnel/desktop a11y` | Accessibility audit of every screen. |
| `pnpm --filter @teitunnel/desktop e2e:build && pnpm --filter @teitunnel/desktop e2e` | End-to-end tests against fake Cloudflare and fake cloudflared. |
| `pnpm --filter @teitunnel/web dev` | The website and user docs. |

To run part of the Rust tests: `cargo nextest run -p teitunnel-core -E 'test(verify)'`.

## A tour of the code

```
crates/cf-api        Cloudflare REST client: typed requests, retries, errors
crates/cloudflared   the cloudflared binary: find/install/verify it, build its commands,
                     parse its logs and metrics, render OS service definitions
crates/lens          the inspector's local reverse proxy: capture, replay, gates
crates/mcp           the MCP server for AI agents
crates/control       the app's local control connection (CLI, editors, browsers)
crates/localdomains  local HTTPS domains: the name-constrained CA and resolvers
crates/core          everything the app does, with no Tauri dependency:
  engine/              observe → plan → apply → verify for every Cloudflare change
  doctor.rs            problem checks (pure) and their fixes
  runtime/             connectors, supervision, sleep and network recovery
  traffic.rs, …        metrics history, logs, discovery, import, settings, store
apps/cli               teitunnel: the same engine from the terminal
apps/desktop/src-tauri the Tauri shell: IPC commands, menus, tray, windows. No business logic.
apps/desktop/src       the React UI: features/<name>/ with queries.ts for data
apps/web               the website: landing page and docs (Next.js + Fumadocs)
tools/                 fake-cloudflared and fake-cloudflare, the test doubles
```

A request travels like this: a React component calls a hook in `features/<f>/queries.ts`, which calls a generated command in `lib/ipc/bindings.ts`. That lands in a `#[tauri::command]` in `src-tauri/src/ipc/`, which calls into `core`. Changes to Cloudflare always go through the engine: `Engine::preview` returns a plan the user reviews, and `Engine::apply` runs it with a staleness check, undoes completed steps if one fails, and records an Activity entry. When something changes, Rust emits `EntityChanged` and the UI invalidates the affected queries; nothing is mirrored by hand.

Rules that aren't negotiable (see AGENTS.md): secrets never cross IPC and live only in the keychain; processes start only through typed builders, never a shell; nothing deletes a DNS record Teitunnel didn't create without asking; IPC types are generated, never hand-written.

## Making a change

1. Fork the repository and create a branch: `<type>/<short-slug>`, for example
   `fix/settings-double-click`.
2. Make the change, following [CONVENTIONS](docs/CONVENTIONS.md) and, for UI,
   [DESIGN](docs/DESIGN.md).
3. **Add tests.** Every behaviour change comes with tests; a bug fix comes with a test that
   fails without it. CONVENTIONS has the table of which kind for which change. Anything
   that talks to Cloudflare is tested against the in-memory fake (`engine/fake.rs`) or
   `tools/fake-cloudflare`, never a real account.
4. **Update the docs** the change affects, in the same pull request.
5. Run `pnpm verify`.
6. Commit with [Conventional Commits](https://www.conventionalcommits.org/):
   `feat(routes): …`, `fix(inspector): …`, `docs: …`. The type decides the changelog entry.

## Pull requests

- Keep each pull request focused on one change, and link the issue it closes.
- Explain **why**, and how you tested it. UI changes include screenshots in light and dark.
- CI runs on macOS, Windows and Linux and must pass. First-time contributors' runs start
  after a maintainer approves them.
- A maintainer reviews every pull request, usually within a few days. Reviews are about the
  change, never the person; please treat reviewers the same way.
- Pull requests are squash-merged, so the title becomes the commit message: make it a
  Conventional Commit.

### Using AI tools

You're welcome to use AI assistants. `AGENTS.md` and the skills in `.claude/skills/` describe
this project's rules and recipes for them. You are responsible for every line you submit:
understand it, test it, and make sure it meets the same bar as hand-written code.

### Licensing

By contributing, you agree that your contributions are licensed under the project's
[MIT License](LICENSE).

## Recipes

### Adding a Doctor check
1. **Gather the fact.** If the check needs information the Doctor doesn't collect yet, add it to `Facts`/`AccountFacts` in `crates/core/src/doctor.rs` and fill it in `gather` (or `run`). Keep gathering and judging separate.
2. **Judge it in `diagnose`** (pure): call `found.add(id, severity, subject, title, detail, evidence, fixes)`. The id is `area.problem`, e.g. `dns.missing`; the issue id adds the account and subject, so "Ignore" survives restarts. Titles say what's wrong in a sentence; details say what it means and what to do.
3. **Offer a fix if you can.** A fix that changes Cloudflare is a `Fix::Change` holding a `Change`, so it goes through the planner like everything else. If it only touches what Teitunnel owns, consider whether `fix_safe` should apply it.
4. **Test it** in `doctor.rs`'s tests: build `Facts`, call `diagnose`, assert the issue and its fix.
5. **Notifications:** errors notify once when they appear. If the problem is already announced another way, or flaps with the network, add its id to `QUIET_CHECKS` in `doctor_monitor.rs`.
6. **Document it** in `apps/web/content/docs/reference/doctor.mdx`.

### Making an origin setting editable
Routes keep any `originRequest` settings they already have, but the only Advanced setting in the UI today is the path. To make one editable (say `noTLSVerify`):
1. Add the field to `RouteInput` (`crates/core/src/engine/views.rs`) and map it into `RouteSpec::options` in `to_intent`. Keep the settings you don't touch as they are.
2. Add planner tests (`engine/planner_tests.rs`): adding, changing and clearing the setting, and that unrelated settings survive. Review the new snapshots with `cargo insta review`.
3. Run `pnpm bindings`, then add the control to the **Advanced** disclosure in `route-sheet.tsx`, using the design system's components (see DESIGN.md), with a component test.
4. Mention it in the docs site if users need to know.

### Adding an IPC command
Write it in `apps/desktop/src-tauri/src/ipc/<area>.rs` with `#[tauri::command]` and `#[specta::specta]`, register it in `ipc.rs`, and keep it thin: validate input, call `core`, map errors to `AppError`. Run `pnpm bindings`, then call it through a hook in the feature's `queries.ts`. Types that cross IPC derive `specta::Type` behind the `specta` feature in `core`.

### Translating Teitunnel
The app's text is in `locales/en.json`: the interface's messages at the top level, and
those the Rust core produces (errors, the Doctor, menus, notifications) under `core`. To
add a language, create `locales/<language>.json` (a BCP 47 tag such as `de`, `fr` or `pt-BR`)
with the same keys and your translations; anything you leave out stays in English. Keep
`{placeholders}` exactly as they are. For counts, provide the plural forms your language
uses (`_zero`, `_one`, `_two`, `_few`, `_many`, `_other`, per the
[CLDR plural rules](https://www.unicode.org/cldr/charts/latest/supplemental/language_plural_rules.html)).
Then run:

```sh
pnpm --filter @teitunnel/desktop i18n:missing <language>   # what's left, and any problems
pnpm test                                                  # rejects unknown keys and wrong placeholders
```

The app uses the language set in System Settings (on macOS, per app under
**General ▸ Language & Region ▸ Applications**), for the window, the menu bar and
notifications alike. The command-line tool stays in English.
