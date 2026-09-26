---
name: reviewing-pull-requests
description: Reviews a pull request to Teitunnel against the project's bar (correctness, the security rules for secrets, processes and Cloudflare changes, tests, docs, design system and cross-platform behaviour), runs it locally, and writes a clear, kind review. Use when asked to review a pull request, a branch or a diff, including contributions from forks.
argument-hint: "[pr-number]"
---

# Reviewing pull requests

A review protects users' Cloudflare accounts and computers, and it's how contributors learn the
project. Be rigorous about the change and generous to the person.

## Gather

```sh
gh pr view <n> --json title,body,author,headRefName,baseRefOid,files,isCrossRepository
gh pr diff <n>
gh pr checks <n>
```

Read the linked issue and the whole diff, then the code around it: a change is right only
if it fits what's already there. Check it out to run it: `gh pr checkout <n>`.

A pull request from a fork doesn't run CI until a maintainer approves the run. Read the diff
first, especially `.github/`, build scripts, `package.json` scripts and `build.rs`, since CI
runs them.

## Review against

```
- [ ] Solves the stated problem completely, and nothing unrelated
- [ ] Correct: edge cases, errors, concurrency, cancellation, all three platforms
- [ ] Secrets: never in IPC, logs, argv, fixtures or errors; only in the keychain
- [ ] Processes only through crates/cloudflared builders; no shells
- [ ] Cloudflare changes only through the plan → apply engine; only what Teitunnel owns is deleted
- [ ] No business logic in src-tauri; bindings regenerated, not hand-edited
- [ ] Tests that fail without the change; deterministic (no sleeps, network or real clock)
- [ ] User-visible text from locales/en.json, with per-platform wording where needed
- [ ] UI follows DESIGN.md: tokens, components, keyboard, light and dark screenshots
- [ ] Docs updated; generated pages regenerated
- [ ] Skills in .claude/skills and AGENTS.md still true after the change (updated if a flow changed)
- [ ] Conventional Commit title; no AI attribution
- [ ] No new dependency without a reason (and cargo deny passes)
```

Then run it: `pnpm verify`, and try the change the way a person would.

## Write the review

- Start with what the change does well and whether the approach is right.
- For each point, say where (file and line), what's wrong, why it matters, and a suggestion.
  Mark what blocks merging and what's optional ("nit:").
- Separate "this is wrong" from "I'd do it differently"; don't block on taste.
- If the fix is small and the contributor allows edits, offer to push it, or suggest it with
  a GitHub suggestion block.
- Thank them. First-time contributors especially.

Post it with `gh pr review <n> --approve|--request-changes|--comment --body-file <file>`.

## Merging

Squash-merge with a Conventional Commit title once CI is green and every blocking point is
resolved. If maintainers build on the contribution in a follow-up, credit the contributor
in the follow-up's description.
