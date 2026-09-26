---
name: adding-cli-commands
description: Adds or changes a subcommand, flag or output of the teitunnel command line (apps/cli), with clap definitions, the shared options, plans that ask before applying, JSON output, exit codes, the generated CLI reference page and end-to-end tests against the fakes. Use when a feature needs to work from the terminal, on servers or in CI.
---

# Adding a CLI command

`teitunnel` (`apps/cli`) is the same product from the terminal: it uses the accounts, engine
and inspector in `teitunnel_core`, goes through the running app when there is one, and runs on
servers and in CI without one. The binary is built as `teitunnel-cli` and presents itself as
`teitunnel`.

## Checklist

```
- [ ] 1. Behaviour in crates/core, not in apps/cli
- [ ] 2. clap definition in apps/cli/src/main.rs (enum Command) with help text
- [ ] 3. Handler in the area's module (apps/cli/src/<area>.rs)
- [ ] 4. Shared options, plans, --json and exit codes follow the conventions
- [ ] 5. Examples in apps/cli/src/docs.rs; regenerate the reference page
- [ ] 6. Tests (unit and apps/cli/tests end to end)
- [ ] 7. pnpm verify
```

## 2. The definition

Subcommands are variants of `enum Command` in `apps/cli/src/main.rs` (clap derive); larger
areas have their own subcommand enum in their module (`traffic::TrafficCommand`). The `///`
doc comments are the help text and the generated reference, so write them for a person
reading `--help`: what it does in one sentence, then what matters.

Reuse the options every command shares, with the same names and meaning:

| Option | Meaning |
|---|---|
| `-a, --account` | Which account, when several are connected |
| `-y, --yes` | Apply a plan without asking (required without a terminal) |
| `--replace` | Also allow what needs confirmation (replacing records Teitunnel didn't create) |
| `--take-over` | Take a hostname someone else holds |
| `--json` | Machine-readable output |
| `--app` / `--here` | Go through the running app, or never |

## 3–4. The handler

- Print results with the `out!` macro, never `println!`: a write error such as a closed pipe
  ends the command with an error instead of a panic.
- A change to Cloudflare shows its plan and asks before applying unless `--yes`, through the
  engine (`changing-cloudflare-resources`). Without a terminal and without `--yes`, refuse
  with a message saying to add `--yes`.
- `--json` prints one JSON document on stdout; everything else (progress, notes) goes to
  stderr so scripts can pipe the output.
- Exit status: 0 success, 1 failure, 2 usage error (clap), 3 a hostname held by someone
  else. Don't invent new codes without documenting them in `apps/cli/src/docs/intro.mdx`.
- Secrets are read from stdin or an environment variable (`…_TOKEN`, `…_TOKEN_FILE`), never
  from an argument, where they would end up in shell history and process lists.
- Messages are complete sentences that say what happened and what to do next. The CLI stays
  in English.

## 5. The reference page

`apps/web/content/docs/reference/cli.mdx` is generated from the clap definitions by
`apps/cli/src/docs.rs`. Add examples for the command there (every example is parsed by the
real parser, so a wrong flag fails the test), then regenerate:

```sh
UPDATE_DOCS=1 cargo test -p teitunnel-cli --bin teitunnel-cli docs
```

Without `UPDATE_DOCS` the test fails when the page is out of date. Never edit `cli.mdx` by
hand; the prose around the generated part is in `apps/cli/src/docs/intro.mdx`. Shell
completion (`complete.rs`) picks up new subcommands and flags by itself.

## 6. Tests

- Unit tests next to the handler for parsing and formatting.
- End to end in `apps/cli/tests/` against `tools/fake-cloudflare` and `fake-cloudflared`,
  running the real binary: the output, the exit code, and what changed on the fake.

## 7. Verify

```sh
CARGO_INCREMENTAL=0 cargo nextest run -p teitunnel-cli
pnpm verify
```
