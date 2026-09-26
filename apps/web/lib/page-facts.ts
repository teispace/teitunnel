import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { absolute } from "./site.ts";

/** Where the docs' MDX files are, relative to the web app. */
const DOCS = "content/docs";

/** When a file was first added and last changed, from git (ISO 8601). */
export interface FileDates {
  published?: string;
  modified?: string;
}

const dates = new Map<string, FileDates>();

/**
 * `file`'s dates from its git history, for search engines' freshness signals. Empty when
 * git or the history isn't there (a shallow clone gives only the last commit, which is
 * why the docs build checks out the full history).
 */
export function gitDates(file: string): FileDates {
  const cached = dates.get(file);
  if (cached) return cached;
  let found: FileDates = {};
  try {
    const log = execFileSync("git", ["log", "--follow", "--format=%cI", "--", file], {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    })
      .split("\n")
      .filter(Boolean);
    if (log.length > 0) found = { modified: log[0], published: log[log.length - 1] };
  } catch {
    // No git: the dates are optional.
  }
  dates.set(file, found);
  return found;
}

/** A docs page's dates, by its path under `content/docs` (e.g. `guides/protection.mdx`). */
export function docDates(pagePath: string): FileDates {
  return gitDates(join(DOCS, pagePath));
}

/** The screenshots a docs page shows (`<Screenshot name="…">`), light and dark. */
export function screenshotsIn(source: string): string[] {
  const names = [...source.matchAll(/<Screenshot\s[^>]*\bname="([a-z0-9-]+)"/g)].map(
    (match) => match[1],
  );
  return [...new Set(names)].flatMap((name) => [
    absolute(`/screens/${name}-light.webp`),
    absolute(`/screens/${name}-dark.webp`),
  ]);
}

/** The screenshots of a docs page, by its path under `content/docs`. */
export function docScreenshots(pagePath: string): string[] {
  try {
    return screenshotsIn(readFileSync(join(DOCS, pagePath), "utf8"));
  } catch {
    return [];
  }
}
