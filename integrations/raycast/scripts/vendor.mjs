// Copies the shared control client (../control-client/src) into src/control-client, so
// the extension stays self-contained for the Raycast Store. `--check` fails if the copy
// is out of date instead of writing it (the tests run it).
import { mkdir, readdir, readFile, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const source = join(root, "..", "control-client", "src");
const target = join(root, "src", "control-client");
const header =
  "// Vendored from integrations/control-client/src by scripts/vendor.mjs. Don't edit here.\n";

const check = process.argv.includes("--check");
await mkdir(target, { recursive: true });
const stale = [];
for (const name of (await readdir(source)).filter((n) => n.endsWith(".ts"))) {
  const wanted = header + (await readFile(join(source, name), "utf8"));
  const current = await readFile(join(target, name), "utf8").catch(() => "");
  if (current === wanted) continue;
  if (check) stale.push(name);
  else await writeFile(join(target, name), wanted);
}
if (stale.length > 0) {
  console.error(`src/control-client is out of date (${stale.join(", ")}): run npm run vendor`);
  process.exit(1);
}
