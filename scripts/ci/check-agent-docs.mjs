#!/usr/bin/env node
// Keeps the guidance for agents and contributors true to the code: every skill in
// .claude/skills has valid frontmatter (a name matching its folder, a description that
// says what and when), stays under 500 lines, is listed in AGENTS.md, and every repository
// path it or AGENTS.md mentions still exists. Run by `pnpm check`, so a change that moves
// or renames something a skill describes has to update the skill too.
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";

const SKILLS = ".claude/skills";
// Where a relative path in the guidance may start from: the repository root, or the
// package a skill is about (`src-tauri/…` in the desktop app, `engine/…` in core).
const BASES = [
  "",
  "apps/desktop/",
  "apps/desktop/src/",
  "apps/cli/",
  "apps/cli/src/",
  "apps/web/",
  "crates/core/src/",
  "crates/mcp/src/",
  "crates/cf-api/src/",
];
const EXTENSIONS = /\.(rs|ts|tsx|js|mjs|json|md|mdx|yml|yaml|toml|css|html)$/;

let failed = false;
const fail = (file, message) => {
  console.error(`check-agent-docs: ${file}: ${message}`);
  failed = true;
};

/** Paths in backticks that name a file or folder in the repository. */
function repositoryPaths(text) {
  const paths = new Set();
  for (const [, raw] of text.matchAll(/`([^`\s]+)`/g)) {
    const path = raw.replace(/[.,:;)]+$/, "").replace(/\/$/, "");
    if (!path.includes("/") || /^(https?:|@|\/|~|\.\.)/.test(path)) continue;
    if (/[<>*{}…$=]|\.\.\./.test(path)) continue;
    const isFolder = raw.endsWith("/");
    if (!isFolder && !EXTENSIONS.test(path)) continue;
    paths.add(path);
  }
  return paths;
}

function checkPaths(file, text) {
  for (const path of repositoryPaths(text)) {
    if (!BASES.some((base) => existsSync(base + path)))
      fail(file, `mentions \`${path}\`, which doesn't exist`);
  }
}

const agents = readFileSync("AGENTS.md", "utf8");
checkPaths("AGENTS.md", agents);

const skills = readdirSync(SKILLS, { withFileTypes: true }).filter((e) => e.isDirectory());
for (const { name: folder } of skills) {
  const file = join(SKILLS, folder, "SKILL.md");
  if (!existsSync(file)) {
    fail(file, "is missing");
    continue;
  }
  const text = readFileSync(file, "utf8");
  const front = text.match(/^---\n([\s\S]*?)\n---\n/);
  if (!front) {
    fail(file, "has no frontmatter");
    continue;
  }
  const field = (key) => front[1].match(new RegExp(`^${key}:\\s*(.+)$`, "m"))?.[1]?.trim();
  const name = field("name");
  const description = field("description") ?? "";
  if (name !== folder) fail(file, `name "${name}" must match its folder "${folder}"`);
  if (!/^[a-z0-9-]{1,64}$/.test(folder))
    fail(file, "name must be lowercase letters, digits and hyphens");
  if (/claude|anthropic/.test(folder)) fail(file, "name must not contain a reserved word");
  if (description.length < 40 || description.length > 1024)
    fail(file, `description must be 40–1024 characters (is ${description.length})`);
  if (!/\bUse (when|for|before|whenever)\b/.test(description))
    fail(file, 'description must say when to use it ("Use when …")');
  const lines = text.split("\n").length;
  if (lines > 500)
    fail(file, `is ${lines} lines; keep it under 500 and move detail to a reference file`);
  if (!agents.includes(`\`${folder}\``)) fail(file, "isn't listed in AGENTS.md's skill table");
  checkPaths(file, text);
}

if (failed) process.exitCode = 1;
else
  process.stdout.write(`check-agent-docs: AGENTS.md and ${skills.length} skills are up to date\n`);
