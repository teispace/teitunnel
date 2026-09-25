/**
 * Builds the extension for Chromium browsers and Firefox into `dist/<browser>/`, with no
 * bundler: Node strips the TypeScript types (the sources only use erasable syntax) and
 * import paths are pointed at the `.js` files.
 */

import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { stripTypeScriptTypes } from "node:module";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const out = process.argv[2] ? resolve(process.argv[2]) : join(root, "dist");
const version: string = JSON.parse(readFileSync(join(root, "package.json"), "utf8")).version;

/** Manifest for `browser`: Firefox has its own id and no Chrome `key`. */
export function manifestFor(browser: "chrome" | "firefox"): Record<string, unknown> {
  const manifest = JSON.parse(readFileSync(join(root, "static/manifest.json"), "utf8"));
  manifest.version = version;
  if (browser === "firefox") {
    delete manifest.key;
    delete manifest.minimum_chrome_version;
    manifest.browser_specific_settings = {
      gecko: { id: "browser@teitunnel.teispace.com", strict_min_version: "128.0" },
    };
  }
  return manifest;
}

for (const browser of ["chrome", "firefox"] as const) {
  const dir = join(out, browser);
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(join(dir, "icons"), { recursive: true });
  for (const file of ["popup.html", "popup.css"])
    cpSync(join(root, "static", file), join(dir, file));
  cpSync(join(root, "icons"), join(dir, "icons"), { recursive: true });
  for (const file of readdirSync(join(root, "src"))) {
    if (!file.endsWith(".ts") || file.endsWith(".d.ts")) continue;
    const source = readFileSync(join(root, "src", file), "utf8");
    const js = stripTypeScriptTypes(source, { mode: "strip" }).replace(
      /(from\s+["']\.\/[\w-]+)\.ts(["'])/g,
      "$1.js$2",
    );
    writeFileSync(join(dir, file.replace(/\.ts$/, ".js")), js);
  }
  writeFileSync(join(dir, "manifest.json"), `${JSON.stringify(manifestFor(browser), null, 2)}\n`);
  process.stdout.write(`built ${dir}\n`);
}
