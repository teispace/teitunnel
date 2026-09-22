// Fails when the initial JS (entry chunk + its static imports) exceeds the budget
// (ARCHITECTURE §14). Run after `vite build`.
import { readFileSync } from "node:fs";
import { gzipSync } from "node:zlib";

const BUDGET_KB = 250;
const dist = new URL("../dist/", import.meta.url);
const html = readFileSync(new URL("index.html", dist), "utf8");
const scripts = [...html.matchAll(/(?:src|href)="\/(assets\/[^"]+\.js)"/g)].map((m) => m[1] ?? "");

let total = 0;
for (const file of scripts) {
  const size = gzipSync(readFileSync(new URL(file, dist))).length;
  total += size;
  process.stdout.write(`${file}  ${(size / 1024).toFixed(1)} KB gzip\n`);
}
const totalKb = total / 1024;
process.stdout.write(`initial JS: ${totalKb.toFixed(1)} KB gzip (budget ${BUDGET_KB} KB)\n`);
if (scripts.length === 0 || totalKb > BUDGET_KB) {
  process.stderr.write("bundle size check failed\n");
  process.exit(1);
}
