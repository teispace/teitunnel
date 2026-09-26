import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, it } from "node:test";

const DOCS = "content/docs";

/** Every docs page's frontmatter, by path. */
function pages(): [string, Record<string, string>][] {
  return (readdirSync(DOCS, { recursive: true }) as string[])
    .filter((file) => file.endsWith(".mdx"))
    .map((file) => {
      const text = readFileSync(join(DOCS, file), "utf8");
      const block = text.match(/^---\n([\s\S]*?)\n---/)?.[1] ?? "";
      const fields = Object.fromEntries(
        [...block.matchAll(/^(\w+): *"?(.*?)"?$/gm)].map((m) => [m[1], m[2]]),
      );
      return [file, fields];
    });
}

describe("docs frontmatter", () => {
  it("gives search results a description they show whole", () => {
    for (const [file, fields] of pages()) {
      assert.ok(fields.title, `${file}: no title`);
      assert.ok(fields.description, `${file}: no description`);
      const shown = fields.searchDescription ?? fields.description;
      assert.ok(
        shown.length >= 70 && shown.length <= 160,
        `${file}: search results show ${shown.length} characters; add a searchDescription of 70–160`,
      );
    }
  });
});
