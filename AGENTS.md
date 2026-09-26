# AGENTS.md

Guidance for AI coding agents working on Teitunnel. Human contributors: start with
[CONTRIBUTING.md](CONTRIBUTING.md); everything here applies to you too.

Teitunnel is a desktop app (Tauri 2: Rust + React) and a command line for Cloudflare Tunnel.
It changes real Cloudflare accounts and puts local services on the internet, so correctness
and security come before speed.

## Rules that are never broken

- **Secrets never cross IPC** and are stored only in the OS keychain (`core::secrets`).
  Tunnel run tokens are never passed on a command line. Never log, serialize or put a secret
  in a test fixture; wrap them in `Secret<String>`.
- **No shells.** Processes start only through the typed builders in `crates/cloudflared`
  (`tokio::process::Command` with discrete arguments).
- **Every Cloudflare change goes through the plan → apply engine** (`core::engine`): planned,
  shown to the person, applied with rollback, recorded in Activity. Nothing deletes a DNS
  record, Access application or rule Teitunnel didn't create without asking.
- **`src-tauri` has no business logic.** `core`, `cf-api` and `cloudflared` don't depend on
  Tauri.
- **IPC types are generated** by tauri-specta. Never edit `apps/desktop/src/lib/ipc/bindings.ts`
  by hand; run `pnpm bindings`.
- **Every user-visible sentence comes from `locales/en.json`**, in the UI and in Rust.
- **`unsafe` is forbidden** except in `crates/core/src/secrets/macos.rs`.
- **Tests with every change.** A bug fix includes a test that fails without it.
- **Commits and pull requests** use Conventional Commits and contain no AI or assistant
  attribution (no `Co-Authored-By` trailers, no "Generated with" lines).

## Layout

```
crates/cf-api        Cloudflare REST and GraphQL client
crates/cloudflared   the cloudflared binary: install/verify, commands, logs, metrics
crates/core          everything the app does: engine, runtime, doctor, store, accounts, inspector
crates/lens          the inspector's local reverse proxy
crates/mcp           the MCP server for AI agents
crates/control       the app's local control connection (JSON-RPC)
crates/localdomains  local HTTPS domains: CA, certificates, resolvers
apps/cli             the `teitunnel` command
apps/desktop         Tauri shell (src-tauri) and React UI (src)
apps/web             website and user docs (Next.js + Fumadocs, static export)
integrations/        VS Code, JetBrains, Raycast, browser extension, GitHub Action
tools/               fake-cloudflare and fake-cloudflared, the test doubles
locales/             every user-visible string (en.json)
docs/                contributor docs: ARCHITECTURE, CONVENTIONS, DESIGN, SECURITY_MODEL
```

## Commands

- `pnpm install`: dependencies and git hooks.
- `pnpm dev`: the app with hot reload (real keychain and data). The Vite dev server alone
  (`pnpm --filter @teitunnel/desktop vite`) runs the UI on a mocked backend.
- `pnpm verify`: everything CI checks (Biome, tsc, rustfmt, clippy, Vitest, nextest,
  cargo-deny). **Must pass before every commit.**
- `pnpm check` / `pnpm test`: checks or tests alone.
- `pnpm bindings`: regenerate IPC bindings after changing a command or an IPC type.
- `CARGO_INCREMENTAL=0 cargo nextest run -p <package> -E 'test(<name>)'`: a subset of Rust
  tests. Packages: `teitunnel-core`, `teitunnel-cf-api`, `teitunnel-cloudflared`,
  `teitunnel-lens`, `teitunnel-mcp`, `teitunnel-control`, `teitunnel-localdomains`,
  `teitunnel-cli`, `teitunnel-desktop`, `fake-cloudflare`, `fake-cloudflared`.
- `pnpm --filter @teitunnel/desktop test -- <pattern>`: a subset of UI tests.
- `pnpm --filter @teitunnel/desktop shoot <dir> [routes…]`: WebKit screenshots, light and
  dark (`?platform=windows|linux` previews that chrome).
- `pnpm --filter @teitunnel/desktop a11y [routes…]`: accessibility audit.
- `pnpm --filter @teitunnel/desktop perf [seconds] [inspector]`: frame timing under load.
- `pnpm --filter @teitunnel/web dev` / `build` / `test`: the website and its docs link check.

## How to work

1. Read the relevant part of [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) and
   [docs/CONVENTIONS.md](docs/CONVENTIONS.md); for UI, [docs/DESIGN.md](docs/DESIGN.md).
2. Find the existing pattern and follow it. This codebase is consistent; a new pattern needs
   a reason.
3. Make the smallest change that solves the problem completely, with tests.
4. Update the docs the change affects: user docs in `apps/web/content/docs`, contributor docs
   in `docs/`. Generated pages (`apps/web/content/docs/reference/cli.mdx`, `apps/web/content/docs/reference/permissions.mdx`) are
   regenerated with `UPDATE_DOCS=1`, never edited.
5. Run `pnpm verify` and fix everything it reports.

Recipes for the common changes are skills in [`.claude/skills/`](.claude/skills): plain
Markdown any agent or person can follow.

## Keep this guidance current

The skills, this file and the contributor docs describe how the code works *now*. They are
maintained like code, by whoever changes the code, in the same pull request:

- **You changed a flow a skill describes** (a file moved, a function was renamed, a step
  was added or removed, a command changed): update that skill so its paths, names, commands
  and examples are true again.
- **You had to discover something a skill should have told you** (a missing step, a
  generated file to refresh, a test to update, a pitfall you hit): add it to the skill, in
  one or two lines, where the next person will need it.
- **A recurring kind of change has no skill yet**, and you had to piece the recipe together
  from several places: add a new skill in `.claude/skills/<gerund-name>/SKILL.md`, and list
  it in the table above.
- **A rule here is wrong or outdated**: fix it, and say why in the pull request.

Write skills the same way as the existing ones: a `name` and a `description` that says what
the skill does and when to use it, a checklist with exact paths, real code from this
repository (not invented examples), a way to check the result, and fewer than 500 lines.
Before you change a skill, confirm what it will say against the code. Never add dates,
session notes, or anything that's only true for one task.

`node scripts/ci/check-agent-docs.mjs` (part of `pnpm check`, and a CI job on every pull
request) fails when a skill or this file names a path that no longer exists, when a skill's
frontmatter is invalid, or when a skill is missing from the table above.

| Skill | For |
|---|---|
| `adding-ipc-commands` | A Rust command the UI calls |
| `changing-cloudflare-resources` | Anything that creates, changes or deletes something on Cloudflare |
| `adding-mcp-tools` | A tool for AI agents in the MCP server |
| `adding-cli-commands` | A `teitunnel` subcommand or flag |
| `writing-user-facing-text` | Any message, label or error people see |
| `building-ui` | Screens and components that follow the design system |
| `adding-doctor-checks` | A problem the Doctor finds and fixes |
| `writing-docs` | User and contributor documentation |
| `verifying-changes` | Choosing and running the right checks before a commit |
| `reviewing-pull-requests` | Reviewing a contribution against the project's bar |
