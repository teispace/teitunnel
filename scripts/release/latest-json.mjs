#!/usr/bin/env node
// Writes the updater manifest (`latest.json`) for a release from the signed files in a
// directory. The updater picks `<os>-<arch>-<installer>` first, then `<os>-<arch>`
// (tauri-plugin-updater 2.12), so each platform gets both keys.
//
// Usage: node scripts/release/latest-json.mjs <dir> <version> <download-base-url> [notes.md]
import { existsSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [dir, version, base, notesFile] = process.argv.slice(2);
if (!dir || !version || !base) {
  console.error("usage: latest-json.mjs <dir> <version> <download-base-url> [notes.md]");
  process.exit(2);
}

/** [platform keys, file-name pattern] for each updatable file. */
const targets = [
  [
    ["darwin-aarch64", "darwin-aarch64-app", "darwin-x86_64", "darwin-x86_64-app"],
    /_universal\.app\.tar\.gz$/,
  ],
  [["windows-x86_64", "windows-x86_64-nsis"], /_x64-setup\.exe$/],
  [["windows-aarch64", "windows-aarch64-nsis"], /_arm64-setup\.exe$/],
  [["linux-x86_64", "linux-x86_64-appimage"], /_amd64\.AppImage$/],
  [["linux-aarch64", "linux-aarch64-appimage"], /_aarch64\.AppImage$/],
  [["linux-x86_64-deb"], /_amd64\.deb$/],
  [["linux-aarch64-deb"], /_arm64\.deb$/],
  [["linux-x86_64-rpm"], /\.x86_64\.rpm$/],
  [["linux-aarch64-rpm"], /\.aarch64\.rpm$/],
];

const files = readdirSync(dir);
const platforms = {};
const missing = [];
for (const [keys, pattern] of targets) {
  const file = files.find((name) => pattern.test(name));
  if (!file) {
    missing.push(pattern.source);
    continue;
  }
  const sigPath = join(dir, `${file}.sig`);
  if (!existsSync(sigPath))
    throw new Error(`${file} has no .sig: sign it before writing latest.json`);
  const entry = {
    url: `${base}/${encodeURIComponent(file)}`,
    signature: readFileSync(sigPath, "utf8").trim(),
  };
  for (const key of keys) platforms[key] = entry;
}
if (missing.length > 0) {
  // A release without every platform would leave some installs without updates.
  console.error(`missing files for: ${missing.join(", ")}`);
  if (process.env.ALLOW_PARTIAL !== "1") process.exit(1);
}

const notes = notesFile && existsSync(notesFile) ? readFileSync(notesFile, "utf8").trim() : "";
const manifest = { version, notes, pub_date: new Date().toISOString(), platforms };
writeFileSync(join(dir, "latest.json"), `${JSON.stringify(manifest, null, 2)}\n`);
process.stdout.write(
  `latest.json: ${Object.keys(platforms).length} platform keys for ${version}` + "\n",
);
