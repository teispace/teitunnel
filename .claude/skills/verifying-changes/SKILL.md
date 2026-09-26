---
name: verifying-changes
description: Chooses and runs the right checks for a change in the Teitunnel repository (targeted Rust and UI tests while iterating, generated files, screenshots, accessibility, performance, end-to-end, then the full pnpm verify) and fixes what they report before committing. Use before every commit or pull request, and when asked to test, check or validate a change.
allowed-tools: Bash(git status *)
---

# Verifying changes

Files changed in the working tree:

!`git status --short`

## While iterating: run what the change touches

| Changed | Run |
|---|---|
| A Rust crate | `CARGO_INCREMENTAL=0 cargo nextest run -p <package> -E 'test(<name>)'` |
| Planner output | the planner tests, then `cargo insta review`: read every snapshot diff |
| A Tauri command or an IPC type | `pnpm bindings`, then the UI tests that use it |
| UI code | `pnpm --filter @teitunnel/desktop test -- <pattern>` |
| A screen's look | `pnpm --filter @teitunnel/desktop shoot /tmp/shots <route>` in light and dark, and look at the images |
| Colours, text or focus order | `pnpm --filter @teitunnel/desktop a11y <route>` |
| Lists, charts, log views | `pnpm --filter @teitunnel/desktop perf` (add `inspector` for the Inspector list) |
| CLI definitions | `UPDATE_DOCS=1 cargo test -p teitunnel-cli --bin teitunnel-cli docs` |
| Token template, scopes or capabilities | `UPDATE_DOCS=1 cargo test -p teitunnel-core --test permissions_doc` |
| `locales/en.json` | `pnpm --filter @teitunnel/desktop test -- i18n` and `cargo build -p teitunnel-core` |
| Website or docs | `pnpm --filter @teitunnel/web test` and `pnpm --filter @teitunnel/web build` |
| Flows across app, engine and Cloudflare | `pnpm --filter @teitunnel/desktop e2e:build && pnpm --filter @teitunnel/desktop e2e` |
| Native look, windows, tray, materials | `pnpm build` and try the packaged app |

Rust packages: `teitunnel-core`, `teitunnel-cf-api`, `teitunnel-cloudflared`, `teitunnel-lens`,
`teitunnel-mcp`, `teitunnel-control`, `teitunnel-localdomains`, `teitunnel-cli`,
`teitunnel-desktop`, `fake-cloudflare`, `fake-cloudflared`.

## Before committing: everything

```sh
pnpm verify
```

It runs Biome, the version check, tsc for the app and website, rustfmt, clippy with
`-D warnings`, Vitest, the website tests, `cargo nextest` for the workspace and
`cargo deny`. It must exit 0.

## When something fails

1. Read the first error, not the last. Fix the cause, not the symptom.
2. Formatting: `pnpm fmt` (or `cargo fmt --all` and `pnpm exec biome check --write`).
3. Clippy: fix the code; `#[allow]` only with a comment saying why it's right here.
4. A failing test you didn't touch: find out why before changing it. A test is changed only
   when the behaviour it pins was meant to change.
5. Stale generated files (`bindings.ts`, `cli.mdx`, `permissions.mdx`): regenerate them with
   the commands above; never edit them by hand.
6. Run the failed step again, then `pnpm verify` again, until it passes.

Don't report a change as done while any check fails; say which check fails and why.

## Before opening the pull request

- The change works as a person would use it, not only in tests (run it in `pnpm dev`, or the
  CLI, against a test account or the fakes).
- UI changes have light and dark screenshots.
- Docs updated (see `writing-docs`).
- Any skill in `.claude/skills/` that describes what you changed still matches the code;
  update it in the same pull request (see "Keep this guidance current" in `AGENTS.md`).
- Commit messages are Conventional Commits, without AI attribution.
