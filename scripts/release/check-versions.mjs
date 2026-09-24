#!/usr/bin/env node
// One version for the whole product (D-074): the Cargo workspace, every workspace crate
// in Cargo.lock, and the desktop app's package.json (which Tauri reads) must agree, and
// release-please must bump every workspace crate in Cargo.lock. Run by `pnpm check` and
// the release build.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const fail = (message) => {
  console.error(`check-versions: ${message}`);
  process.exitCode = 1;
};

const metadata = JSON.parse(
  execFileSync("cargo", ["metadata", "--format-version", "1", "--no-deps", "--locked"], {
    encoding: "utf8",
  }),
);
const app = JSON.parse(readFileSync("apps/desktop/package.json", "utf8")).version;
const manifest = JSON.parse(readFileSync(".release-please-manifest.json", "utf8"))["."];
const config = JSON.parse(readFileSync("release-please-config.json", "utf8"));

if (manifest !== app) fail(`.release-please-manifest.json has ${manifest}, the app ${app}`);
for (const crate of metadata.packages) {
  if (crate.version !== app) fail(`${crate.name} is ${crate.version}, the app ${app}`);
}

// release-please bumps the Cargo.lock packages without a `source` (its TOML parser can't
// match filters on names), which must be exactly the workspace crates.
const LOCK_PATH = "$.package[?(!@.source)].version";
const lockEntry = config.packages["."]["extra-files"].find((f) => f.path === "Cargo.lock");
if (lockEntry?.jsonpath !== LOCK_PATH)
  fail(`release-please-config.json must bump Cargo.lock with ${LOCK_PATH}`);
const sourceless = readFileSync("Cargo.lock", "utf8")
  .split("[[package]]")
  .slice(1)
  .filter((block) => !/^source = /m.test(block))
  .map((block) => /^name = "([^"]+)"/m.exec(block)?.[1])
  .sort();
const crates = metadata.packages.map((p) => p.name).sort();
if (sourceless.join() !== crates.join())
  fail(`Cargo.lock packages without a source (${sourceless}) aren't the workspace (${crates})`);

if (!process.exitCode)
  process.stdout.write(`check-versions: ${crates.length} crates and the app at ${app}\n`);
