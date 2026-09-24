// Pure helpers for the Teitunnel preview action: inputs, hostname templates, release
// assets and checksums, CLI arguments and the pull request comment. No I/O except
// through what callers pass in (`fetch`), so everything here is unit tested.

import { createHash } from "node:crypto";

/** The repository the CLI is released from. */
export const RELEASE_REPO = "teispace/teitunnel";

/** Exit code the CLI uses when someone else holds the hostname. */
export const EXIT_HELD = 3;

const MODES = new Set(["share", "snapshot", "cleanup"]);

/** Reads the action's inputs from the environment (`INPUT_<NAME>`, as the runner sets them). */
export function readInputs(env) {
  const get = (name, fallback = "") => {
    const value = env[`INPUT_${name.toUpperCase()}`];
    return value === undefined || value.trim() === "" ? fallback : value.trim();
  };
  const bool = (name, fallback) => {
    const value = get(name, String(fallback)).toLowerCase();
    if (["true", "yes", "1", "on"].includes(value)) return true;
    if (["false", "no", "0", "off"].includes(value)) return false;
    throw new Error(`Input ${name} must be true or false, not "${value}".`);
  };
  const inputs = {
    token: get("cloudflare-api-token"),
    account: get("account"),
    mode: get("mode", "snapshot").toLowerCase(),
    port: get("port"),
    url: get("url"),
    path: get("path", "."),
    build: bool("build", false),
    hostname: get("hostname", "pr-{number}.preview.{zone}"),
    zone: get("zone"),
    name: get("name", "{repo}-pr-{number}"),
    expires: get("expires"),
    password: get("password"),
    allow: list(get("allow")),
    comment: bool("comment", true),
    githubToken: get("github-token"),
    version: get("version", "latest"),
    cliPath: get("cli-path"),
    wait: Number.parseInt(get("wait", "120"), 10),
  };
  if (!inputs.token) throw new Error("Input cloudflare-api-token is required.");
  if (!MODES.has(inputs.mode)) {
    throw new Error(`Input mode must be share, snapshot or cleanup, not "${inputs.mode}".`);
  }
  if (inputs.mode === "share" && !inputs.port && !inputs.url) {
    throw new Error("Share mode needs the port (or url) of a server the job started.");
  }
  if (!Number.isFinite(inputs.wait) || inputs.wait < 5) {
    throw new Error("Input wait must be a number of seconds (5 or more).");
  }
  return inputs;
}

/** `a, b` or one per line → `["a", "b"]`. */
export function list(value) {
  return value
    .split(/[\n,]/)
    .map((item) => item.trim())
    .filter(Boolean);
}

/** Text as one DNS label: lowercase letters, digits and dashes, at most `max` long. */
export function label(value, max = 63) {
  return String(value)
    .toLowerCase()
    .replace(/[^a-z0-9-]+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-+/, "")
    .slice(0, max)
    .replace(/-+$/, "");
}

/** What the placeholders stand for, from the workflow's event. */
export function templateContext(env, event, zone) {
  const pr = event?.pull_request;
  const repo = (env.GITHUB_REPOSITORY ?? "").split("/");
  const branch =
    pr?.head?.ref ?? env.GITHUB_HEAD_REF ?? (env.GITHUB_REF ?? "").replace(/^refs\/heads\//, "");
  return {
    number: pr?.number ?? event?.number ?? "",
    branch,
    sha: (pr?.head?.sha ?? env.GITHUB_SHA ?? "").slice(0, 7),
    repo: repo[1] ?? "",
    owner: repo[0] ?? "",
    zone,
  };
}

/**
 * Fills `{number}`, `{branch}`, `{sha}`, `{repo}` and `{owner}` (each made a DNS label)
 * and `{zone}` (as given) into a hostname template, then checks the result.
 */
export function renderHostname(template, context) {
  if (template.includes("{zone}") && !context.zone) {
    throw new Error("The hostname template uses {zone}: set the zone input (e.g. example.com).");
  }
  if (template.includes("{number}") && context.number === "") {
    throw new Error(
      "The hostname template uses {number}, but this run isn't for a pull request. Use {branch} or {sha}, or run on pull_request.",
    );
  }
  const filled = template.replace(/\{(\w+)\}/g, (whole, name) => {
    if (name === "zone") return String(context.zone).toLowerCase();
    if (!(name in context)) throw new Error(`Unknown placeholder ${whole} in the hostname.`);
    return label(context[name]);
  });
  const hostname = filled.toLowerCase().replace(/\.+$/, "");
  const labels = hostname.split(".");
  const valid =
    hostname.length <= 253 &&
    labels.length >= 2 &&
    labels.every((part) => /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/.test(part));
  if (!valid) throw new Error(`"${hostname}" isn't a valid hostname.`);
  return hostname;
}

/** A Snapshot name from its template: DNS-safe, at most 40 characters. */
export function snapshotName(template, context) {
  const filled = template.replace(/\{(\w+)\}/g, (_, name) => String(context[name] ?? ""));
  const name = label(filled, 40);
  if (!name) throw new Error("The Snapshot name came out empty; set the name input.");
  return name;
}

/** The release asset for this runner: `teitunnel-cli_<v>_<os>-<arch>.<ext>`. */
export function assetFor(platform, arch, version) {
  const v = version.replace(/^v/, "");
  const cpu = arch === "arm64" ? "arm64" : arch === "x64" ? "x64" : null;
  switch (platform) {
    case "linux":
      if (!cpu) break;
      return { name: `teitunnel-cli_${v}_linux-${cpu}.tar.gz`, binary: "teitunnel" };
    case "darwin":
      return { name: `teitunnel-cli_${v}_macos-universal.zip`, binary: "teitunnel" };
    case "win32":
      if (!cpu) break;
      return { name: `teitunnel-cli_${v}_windows-${cpu}.zip`, binary: "teitunnel.exe" };
  }
  throw new Error(`No Teitunnel CLI is released for ${platform} ${arch}.`);
}

/** `SHA256SUMS.txt` (`<hash>  <file>` per line) as a map from file to hash. */
export function parseSums(text) {
  const sums = new Map();
  for (const line of text.split("\n")) {
    const match = /^([0-9a-f]{64})\s+\*?(.+)$/i.exec(line.trim());
    if (match) sums.set(match[2].trim(), match[1].toLowerCase());
  }
  return sums;
}

/** Throws unless `bytes` hash to what the release's checksums say for `name`. */
export function verifyChecksum(bytes, name, sums) {
  const expected = sums.get(name);
  if (!expected) throw new Error(`The release's SHA256SUMS.txt doesn't list ${name}.`);
  const actual = createHash("sha256").update(bytes).digest("hex");
  if (actual !== expected) {
    throw new Error(`${name} doesn't match its checksum (expected ${expected}, got ${actual}).`);
  }
}

/** The last line of `stdout` that is a JSON object (the CLI prints it after its plan). */
export function lastJson(stdout) {
  const lines = stdout.split("\n").reverse();
  for (const line of lines) {
    const text = line.trim();
    if (!text.startsWith("{")) continue;
    try {
      return JSON.parse(text);
    } catch {
      // Not JSON after all; keep looking.
    }
  }
  return null;
}

/** `teitunnel share …` for share mode. */
export function shareArgs(inputs, hostname) {
  const args = ["share", inputs.url || inputs.port, "--on", hostname, "--json", "--no-qr"];
  if (inputs.account) args.push("--account", inputs.account);
  if (inputs.expires) args.push("--for", inputs.expires);
  for (const who of inputs.allow) args.push("--allow", who);
  return args;
}

/** `teitunnel snapshot publish …` for snapshot mode (the password goes by environment). */
export function snapshotArgs(inputs, hostname, name) {
  const args = [
    "snapshot",
    "publish",
    inputs.path,
    "--name",
    name,
    "--on",
    hostname,
    "--or-update",
    "--yes",
    "--json",
  ];
  if (inputs.build) args.push("--build");
  if (inputs.account) args.push("--account", inputs.account);
  if (inputs.expires) args.push("--expires", inputs.expires);
  if (inputs.password) args.push("--password");
  for (const who of inputs.allow) args.push("--allow", who);
  return args;
}

/** The hidden marker that finds this preview's comment again (one per hostname). */
export function marker(hostname) {
  return `<!-- teitunnel-preview:${hostname} -->`;
}

/** The pull request comment for a preview that is live, stopped or removed. */
export function commentBody({ hostname, url, state, sha, mode }) {
  const kind = mode === "share" ? "Live preview (runs while the job runs)" : "Preview";
  const lines = [marker(hostname)];
  if (state === "live") {
    lines.push(
      `**${kind}:** ${url}`,
      "",
      `Updated for ${sha ? `\`${sha}\`` : "the latest commit"}.`,
    );
  } else if (state === "stopped") {
    lines.push(`**${kind}:** ~~${url}~~`, "", "The job ended, so this preview stopped.");
  } else {
    lines.push(`**Preview:** ~~https://${hostname}~~`, "", "Removed: the pull request was closed.");
  }
  lines.push("", "<sub>Published with Teitunnel on your own Cloudflare account.</sub>");
  return lines.join("\n");
}

/**
 * Creates the preview's comment on the pull request, or edits the one already there
 * (found by its hidden marker), so a pull request never gets a second one. Returns
 * `created` or `updated`.
 */
export async function upsertComment({ fetch, apiUrl, repository, number, token, hostname, body }) {
  const headers = {
    authorization: `Bearer ${token}`,
    accept: "application/vnd.github+json",
    "x-github-api-version": "2022-11-28",
    "content-type": "application/json",
    "user-agent": "teitunnel-action",
  };
  const base = `${apiUrl.replace(/\/$/, "")}/repos/${repository}/issues`;
  const tag = marker(hostname);
  let existing = null;
  for (let page = 1; page <= 20 && !existing; page += 1) {
    const response = await fetch(`${base}/${number}/comments?per_page=100&page=${page}`, {
      headers,
    });
    if (!response.ok) throw new Error(`Listing comments failed: HTTP ${response.status}.`);
    const comments = await response.json();
    existing = comments.find((c) => typeof c.body === "string" && c.body.includes(tag)) ?? null;
    if (comments.length < 100) break;
  }
  const response = existing
    ? await fetch(`${base}/comments/${existing.id}`, {
        method: "PATCH",
        headers,
        body: JSON.stringify({ body }),
      })
    : await fetch(`${base}/${number}/comments`, {
        method: "POST",
        headers,
        body: JSON.stringify({ body }),
      });
  if (!response.ok) {
    throw new Error(
      `${existing ? "Editing" : "Posting"} the comment failed: HTTP ${response.status}.`,
    );
  }
  return existing ? "updated" : "created";
}

/** The tunnel name for a share-mode job (deleted again by the post step). */
export function machineName(env) {
  return label(
    `gh-${(env.GITHUB_REPOSITORY ?? "repo").split("/").pop()}-${env.GITHUB_RUN_ID ?? "0"}-${env.GITHUB_RUN_ATTEMPT ?? "1"}`,
  );
}

/** Who holds the names this action writes (every run of the repository, the same). */
export function ownerLabel(env) {
  return `github-actions/${env.GITHUB_REPOSITORY ?? "unknown"}`;
}
