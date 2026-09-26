#!/usr/bin/env node
// Builds the CLI (crate teitunnel-cli, command `teitunnel`) for a target and puts it where
// Tauri bundles it next to the app (`bundle.externalBin`). Linux packages
// carry it as `teitunnel` (tauri.cli.linux.conf.json); macOS and Windows as `teitunnel-cli`
// (tauri.cli.conf.json), because their file systems ignore case and the app itself is
// `Teitunnel`. What people type is always `teitunnel`.
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
const name = target.includes("linux") ? "teitunnel" : "teitunnel-cli";

const dir = "apps/desktop/src-tauri/binaries";
mkdirSync(dir, { recursive: true });
/** Puts a built CLI where Tauri looks for it for `triple`. */
const place = (built, triple) => {
  const sidecar = join(dir, `${name}-${triple}${exe}`);
  copyFileSync(built, sidecar);
  process.stdout.write(`sidecar: ${sidecar}\n`);
};

if (target === "universal-apple-darwin") {
  // Tauri builds each architecture on its own (and wants that one's CLI), then bundles
  // the universal app with the universal CLI.
  const parts = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
  for (const t of parts) {
    run("cargo", ["build", "--release", "--locked", "-p", "teitunnel-cli", "--target", t]);
    place(`target/${t}/release/teitunnel-cli`, t);
  }
  mkdirSync("target/universal-apple-darwin/release", { recursive: true });
  const universal = "target/universal-apple-darwin/release/teitunnel-cli";
  run("lipo", [
    "-create",
    "-output",
    universal,
    ...parts.map((t) => `target/${t}/release/teitunnel-cli`),
  ]);
  place(universal, target);
} else {
  run("cargo", ["build", "--release", "--locked", "-p", "teitunnel-cli", "--target", target]);
  place(`target/${target}/release/teitunnel-cli${exe}`, target);
}
