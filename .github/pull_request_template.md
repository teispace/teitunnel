## What and why

<!-- What does this change, and why? Link the issue: "Closes #123". -->

## How it was tested

<!-- Tests added, commands run (`pnpm verify`), manual checks and on which systems. -->

## Checklist

- [ ] The title is a [Conventional Commit](https://www.conventionalcommits.org/) (`feat(routes): …`, `fix: …`, `docs: …`).
- [ ] `pnpm verify` passes, and `pnpm bindings` was run if a Rust IPC type or command changed.
- [ ] Tests cover the change; a bug fix has a test that fails without it.
- [ ] User docs (`apps/web/content/docs`) and contributor docs (`docs/`) are updated where the change affects them.
- [ ] Skills in `.claude/skills/` and `AGENTS.md` still match the code (updated if a flow or pattern changed).
- [ ] No secrets in logs, IPC, command-line arguments or fixtures; no shell invocations.

### For UI changes

- [ ] Screenshots in light and dark (and Windows or Linux if the change is platform-specific).
- [ ] Semantic tokens and existing components only ([DESIGN](../docs/DESIGN.md)); every string from `locales/en.json`.
- [ ] Works with the keyboard; `pnpm --filter @teitunnel/desktop a11y` is clean.
