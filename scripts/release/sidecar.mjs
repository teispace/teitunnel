#!/usr/bin/env node
// Builds teitunnel-cli for a target and puts it where Tauri bundles it next to the app
// (`bundle.externalBin` in apps/desktop/src-tauri/tauri.cli.conf.json, D-077).
//
// Usage: node scripts/release/sidecar.mjs [target]   (default: this machine)
// `universal-apple-darwin` builds both Mac architectures and joins them with lipo.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { join } from "node:path";

const run = (cmd, args) => execFileSync(cmd, args, { stdio: "inherit" });
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)?.[1];
const target = process.argv[2] ?? host;
if (!target) throw new Error("couldn't tell the target");
const exe = target.includes("windows") ? ".exe" : "";

let built;
if (target === "universal-apple-darwin") {
  const parts = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  for (const t of parts) {
    run("cargo", ["build", "--release", "--locked", "-p", "teitunnel-cli", "--target", t]);
  }
  mkdirSync("target/universal-apple-darwin/release", { recursive: true });
  built = "target/universal-apple-darwin/release/teitunnel-cli";
  run("lipo", [
    "-create",
    "-output",
    built,
    ...parts.map((t) => `target/${t}/release/teitunnel-cli`),
  ]);
} else {
  run("cargo", ["build", "--release", "--locked", "-p", "teitunnel-cli", "--target", target]);
  built = `target/${target}/release/teitunnel-cli${exe}`;
}

const dir = "apps/desktop/src-tauri/binaries";
mkdirSync(dir, { recursive: true });
const sidecar = join(dir, `teitunnel-cli-${target}${exe}`);
copyFileSync(built, sidecar);
process.stdout.write(`sidecar: ${sidecar}\n`);
