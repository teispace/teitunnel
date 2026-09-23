#!/usr/bin/env node
// One version for the whole product (D-074): the Cargo workspace, every workspace crate
// in Cargo.lock, and the desktop app's package.json (which Tauri reads) must agree, and
// release-please must bump every workspace crate in Cargo.lock. Run in CI.
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
const config = readFileSync("release-please-config.json", "utf8");

if (manifest !== app) fail(`.release-please-manifest.json has ${manifest}, the app ${app}`);
for (const crate of metadata.packages) {
  if (crate.version !== app) fail(`${crate.name} is ${crate.version}, the app ${app}`);
  if (!config.includes(`@.name=='${crate.name}'`))
    fail(`release-please-config.json doesn't bump ${crate.name} in Cargo.lock`);
}
if (!process.exitCode)
  process.stdout.write(
    `check-versions: ${metadata.packages.length} crates and the app at ${app}` + "\n",
  );
