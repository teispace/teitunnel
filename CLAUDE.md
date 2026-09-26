@AGENTS.md

## Claude Code

- The skills in `.claude/skills/` load automatically when a task matches; run one directly
  with `/<skill-name>` (for example `/verifying-changes`).
- `.claude/settings.json` holds the project's shared permissions. Personal settings go in
  `.claude/settings.local.json`, and personal instructions in `CLAUDE.local.md`; both are
  git-ignored.
