import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join, relative } from "node:path";
import { describe, it } from "node:test";

const root = new URL("../content/docs/", import.meta.url).pathname;

function mdxFiles(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return mdxFiles(path);
    return entry.name.endsWith(".mdx") ? [path] : [];
  });
}

/** `/docs/guides/inspector` for `guides/inspector.mdx`, `/docs/compare` for `compare/index.mdx`. */
function route(file: string): string {
  const path = relative(root, file)
    .replace(/\.mdx$/, "")
    .replace(/(^|\/)index$/, "");
  return path ? `/docs/${path}` : "/docs";
}

/** Heading ids as the docs site makes them (GitHub's rules). */
function anchors(source: string): Set<string> {
  const seen = new Map<string, number>();
  const ids = new Set<string>();
  const body = source.replace(/```[\s\S]*?```/g, "");
  for (const match of body.matchAll(/^#{1,6}\s+(.+)$/gm)) {
    const text = match[1]
      .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/[`*_]/g, "")
      .trim();
    const base = text
      .toLowerCase()
      .replace(/[^\p{L}\p{M}\p{N}\p{Pc} -]/gu, "")
      .replace(/ /g, "-");
    const count = seen.get(base) ?? 0;
    seen.set(base, count + 1);
    ids.add(count ? `${base}-${count}` : base);
  }
  return ids;
}

const files = mdxFiles(root);
const pages = new Map(files.map((file) => [route(file), anchors(readFileSync(file, "utf8"))]));

describe("docs links", () => {
  it("finds the pages", () => {
    assert.ok(pages.has("/docs"));
    assert.ok(pages.has("/docs/compare"));
    assert.ok(pages.get("/docs/reference/limits")?.has("quick-shares"));
  });

  it("point at pages and headings that exist", () => {
    const broken: string[] = [];
    for (const file of files) {
      const source = readFileSync(file, "utf8").replace(/```[\s\S]*?```/g, "");
      const links = [
        ...source.matchAll(/\]\((\/docs[^)\s]*)\)/g),
        ...source.matchAll(/href="(\/docs[^"]*)"/g),
      ].map((m) => m[1]);
      for (const link of links) {
        const [path, anchor] = link.split("#");
        const target = pages.get(path.replace(/\/$/, "") || "/docs");
        if (!target) broken.push(`${relative(root, file)}: ${link} (no such page)`);
        else if (anchor && !target.has(anchor))
          broken.push(`${relative(root, file)}: ${link} (no such heading)`);
      }
    }
    assert.deepEqual(broken, []);
  });
});
