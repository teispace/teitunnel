# Autonomous work protocol

For sessions where the maintainer says "start" or "continue" and then leaves. The agent drives the roadmap alone, to production quality.

## Loop (repeat until the session ends)
1. **Orient:** read `docs/STATUS.md`, then the current plan file in `docs/plans/`, then `git status` / `git log -5`. If a previous session left work half-done, finish or repair it first.
2. **Pick** the next unchecked task in the current milestone (the plan order is the default; reorder only if there's a dependency reason, and note why).
3. **Re-verify externals** the task depends on (versions, APIs) if `docs/research/*` is older than about 2 weeks for that item. Update the research doc.
4. **Implement** per ARCHITECTURE, CONVENTIONS, DESIGN and SECURITY_MODEL. No placeholders or fake logic. Anything that can't be finished becomes an explicit TODO in STATUS, not hidden in code.
5. **Validate**, all of which must pass before committing:
   - `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace` (or `cargo test`)
   - `pnpm biome ci`, `pnpm tsc --noEmit`, `pnpm vitest run`
   - bindings regenerated with no diff
   - for runtime/app changes: actually launch the app (`pnpm tauri dev`, and `pnpm tauri build` for anything visual, window-level or material-related) and check it works
6. **Verify the UI visually** for UI tasks: capture the app window (`screencapture -x -o -l <windowId> out.png`, getting the window id via a small Swift/osascript helper, or `screencapture -x -R x,y,w,h`). Read the screenshot and critique it against the DESIGN.md §12 checklist: alignment on the 4 px grid, type sizes, colours in light and dark (toggle with `osascript -e 'tell app "System Events" to tell appearance preferences to set dark mode to true/false'`), active vs inactive window, and spacing consistency. **Iterate until it's right.** Save the final screenshots under `docs/screenshots/<task-id>/` (keep them small; PNG optimised).
7. **Optimise:** check the bundle size budget and avoid needless re-renders and allocations. Measure startup and memory when the milestone touches them.
8. **Commit** (Conventional Commits, **no AI attribution**) and push.
9. **Update docs:** tick the plan and ROADMAP, update STATUS (completed, next, notes), DECISIONS, research. Update memory if something durable was learned. Commit.
10. Continue with the next task. At the end of a milestone, go through its exit criteria one by one, fix gaps, then move to the next milestone.

## Git flow
- One branch per milestone: `milestone/m0-foundations`. Open a **draft PR** to `main` early so CI runs on every push.
- Commit per task (or smaller). Push after each task.
- When all exit criteria pass and CI is green, mark the PR ready and **squash-merge** it, with the PR title as the milestone summary. Start the next milestone branch from the updated `main`.
- If CI fails: fix before starting new work. Never merge red.

## When blocked
- **Missing a maintainer-only input** (OAuth client, Apple ID, test account secrets, payment): skip that task, record it in STATUS → Blockers, and continue with the next task or milestone that doesn't depend on it. Use fakes or feature flags so the rest can progress.
- **A technical dead end** (library bug, platform crash): time-box it (roughly an hour of attempts). Then pick the documented fallback (see DECISIONS), or record the problem and choose the most conservative working alternative. Log a decision.
- **Ambiguity:** choose the option most consistent with VISION principles and the existing docs, and record it as a decision. Don't wait for an answer.

## Never do without the maintainer
- Publish a release or tag `v*`, or publish to Homebrew or other channels.
- Change repo visibility or settings, rotate or add secrets, or register external apps/clients.
- Spend money, sign up for services, or use the maintainer's Cloudflare account for anything beyond read-only checks.
- Force-push `main`, rewrite published history, or delete branches/tags other than merged milestone branches.
- Weaken a security rule from SECURITY_MODEL.md to get something working.

## Session hygiene
- Keep STATUS "Notes for the next session" as a precise resume point: branch, last commit, what's half-done, and the exact next step.
- Long-running processes (dev servers, the app) must be stopped before the session ends. Leave no orphaned `cloudflared` processes.
