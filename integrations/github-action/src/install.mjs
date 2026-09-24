// Installs the released Teitunnel CLI for this runner, verified against the release's
// SHA256SUMS.txt before it's unpacked.

import { chmodSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { assetFor, parseSums, RELEASE_REPO, verifyChecksum } from "./lib.mjs";
import { log, run } from "./runner.mjs";

async function download(url, token) {
  const headers = { "user-agent": "teitunnel-action" };
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(url, { headers, redirect: "follow" });
  if (!response.ok) throw new Error(`Downloading ${url} failed: HTTP ${response.status}.`);
  return Buffer.from(await response.arrayBuffer());
}

/** `latest` → the newest release's version (e.g. `0.3.0`); anything else as given. */
async function resolveVersion(version, token) {
  if (version !== "latest") return version.replace(/^v/, "");
  const api = process.env.GITHUB_API_URL ?? "https://api.github.com";
  const headers = { accept: "application/vnd.github+json", "user-agent": "teitunnel-action" };
  if (token) headers.authorization = `Bearer ${token}`;
  const response = await fetch(`${api}/repos/${RELEASE_REPO}/releases/latest`, { headers });
  if (!response.ok) throw new Error(`Finding the latest release failed: HTTP ${response.status}.`);
  const release = await response.json();
  return String(release.tag_name).replace(/^v/, "");
}

/** Downloads, verifies and unpacks the CLI; returns the path of `teitunnel`. */
export async function installCli({ version, token, dir }) {
  const resolved = await resolveVersion(version, token);
  const asset = assetFor(process.platform, process.arch, resolved);
  const target = join(dir, resolved);
  const binary = join(target, asset.binary);
  if (existsSync(binary)) return binary;
  const base = `https://github.com/${RELEASE_REPO}/releases/download/v${resolved}`;
  log(`Installing the Teitunnel CLI ${resolved} (${asset.name})…`);
  const [sums, archive] = await Promise.all([
    download(`${base}/SHA256SUMS.txt`, token),
    download(`${base}/${asset.name}`, token),
  ]);
  verifyChecksum(archive, asset.name, parseSums(sums.toString("utf8")));
  mkdirSync(target, { recursive: true });
  const file = join(target, asset.name);
  writeFileSync(file, archive);
  // bsdtar (macOS, Windows) and GNU tar (Linux) both read these archives.
  const unpacked = await run("tar", ["-xf", file, "-C", target], process.env);
  if (unpacked.code !== 0) throw new Error(`Unpacking ${asset.name} failed.`);
  if (process.platform !== "win32") chmodSync(binary, 0o755);
  return binary;
}
