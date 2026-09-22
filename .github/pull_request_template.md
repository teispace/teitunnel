## What & why

<!-- Link the task ID (e.g. M1-04) or issue. Explain why this change is needed. -->

## How it was tested

<!-- Commands run, tests added, manual checks. -->

## Checklist

- [ ] `pnpm check && pnpm test` pass locally; bindings regenerated (`pnpm bindings`) if Rust IPC changed
- [ ] Tests added/updated (see docs/CONVENTIONS.md → Testing)
- [ ] Docs updated (plan checkbox, docs/STATUS.md, DECISIONS/ARCHITECTURE/DESIGN if affected)
- [ ] No secrets in logs, IPC, argv, or fixtures; no shell invocations

### UI changes (DESIGN.md §12)

- [ ] Semantic tokens only; no raw colours or ad-hoc spacing
- [ ] Screenshots in light and dark, active and inactive window
- [ ] Keyboard path works; menu item and shortcut where appropriate
- [ ] No hand cursor, no layout shift, no spinner for local state
- [ ] Copy follows DESIGN §10; empty, loading and error states designed
- [ ] Reduced motion / transparency checked; gallery updated for primitives and patterns
