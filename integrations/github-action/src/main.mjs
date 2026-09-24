// The action's main step: publish (or remove) a pull request preview with the
// Teitunnel CLI on the user's own Cloudflare account, and comment its URL on the PR.

import { spawn } from "node:child_process";
import { closeSync, existsSync, mkdirSync, openSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { installCli } from "./install.mjs";
import {
  commentBody,
  EXIT_HELD,
  lastJson,
  machineName,
  ownerLabel,
  readInputs,
  renderHostname,
  shareArgs,
  snapshotArgs,
  snapshotName,
  templateContext,
  upsertComment,
} from "./lib.mjs";
import {
  error,
  log,
  mask,
  notice,
  readEvent,
  run,
  saveState,
  setOutput,
  warning,
} from "./runner.mjs";

/** The environment the CLI runs with: the token for this job only, never stored. */
export function cliEnv(inputs, dataDir) {
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith("INPUT_")),
  );
  env.CLOUDFLARE_API_TOKEN = inputs.token;
  env.TEITUNNEL_DATA_DIR = dataDir;
  env.TEITUNNEL_OWNER ??= ownerLabel(process.env);
  env.TEITUNNEL_MACHINE_NAME ??= machineName(process.env);
  if (inputs.password) env.TEITUNNEL_SNAPSHOT_PASSWORD = inputs.password;
  return env;
}

export function paths() {
  const base = process.env.RUNNER_TEMP || tmpdir();
  return { data: join(base, "teitunnel-data"), cli: join(base, "teitunnel-cli") };
}

function held(stderr) {
  return new Error(
    `${
      stderr
        .trim()
        .split("\n")
        .pop()
        ?.replace(/^teitunnel: /, "") ?? "The hostname is held."
    } Choose another hostname template, or release the name.`,
  );
}

/** Comments on the pull request, if asked and possible (fork PRs have no write token). */
export async function comment(inputs, event, hostname, state, url) {
  const number = event?.pull_request?.number ?? event?.number;
  if (!inputs.comment) return;
  if (!number) {
    log("Not a pull request: no comment.");
    return;
  }
  if (!inputs.githubToken) {
    warning("No github-token, so no comment.");
    return;
  }
  const body = commentBody({
    hostname,
    url,
    state,
    sha: (event.pull_request?.head?.sha ?? process.env.GITHUB_SHA ?? "").slice(0, 7),
    mode: inputs.mode,
  });
  try {
    const done = await upsertComment({
      fetch,
      apiUrl: process.env.GITHUB_API_URL ?? "https://api.github.com",
      repository: process.env.GITHUB_REPOSITORY ?? "",
      number,
      token: inputs.githubToken,
      hostname,
      body,
    });
    log(`Pull request comment ${done}.`);
  } catch (err) {
    warning(
      `Couldn't comment on the pull request (${err.message}). Pull requests from forks get a read-only token; give the job \`pull-requests: write\`.`,
    );
  }
}

/** Ensures a cloudflared the CLI can run (the managed, verified copy if none is found). */
async function ensureCloudflared(cli, env) {
  if ((await run(cli, ["cloudflared", "status"], env)).code === 0) return;
  const installed = await run(cli, ["cloudflared", "install"], env);
  if (installed.code !== 0) throw new Error("Couldn't install cloudflared.");
}

/**
 * Starts `teitunnel share … --json` in the background (it keeps the share up for the
 * rest of the job; the post step stops it) and waits for its URL.
 */
async function startShare(cli, args, env, dataDir, seconds) {
  const out = join(dataDir, "share.out");
  const err = join(dataDir, "share.err");
  const [outFd, errFd] = [openSync(out, "w"), openSync(err, "w")];
  const child = spawn(cli, args, { env, detached: true, stdio: ["ignore", outFd, errFd] });
  closeSync(outFd);
  closeSync(errFd);
  let exited = null;
  child.on("exit", (code) => {
    exited = code ?? 1;
  });
  child.unref();
  saveState("pid", child.pid);
  const deadline = Date.now() + seconds * 1000;
  while (Date.now() < deadline) {
    const live = lastJson(readFileSync(out, "utf8"));
    if (live?.url) return live;
    if (exited !== null) {
      const stderr = readFileSync(err, "utf8");
      process.stderr.write(stderr);
      if (exited === EXIT_HELD) throw held(stderr);
      throw new Error(`The share ended before it was live (exit code ${exited}).`);
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
  process.stderr.write(readFileSync(err, "utf8"));
  throw new Error(`The share wasn't live after ${seconds} s.`);
}

export async function main() {
  const inputs = readInputs(process.env);
  mask(inputs.token);
  if (inputs.password) mask(inputs.password);
  const event = readEvent();
  const context = templateContext(process.env, event, inputs.zone);
  const hostname = renderHostname(inputs.hostname, context);
  setOutput("hostname", hostname);
  const dirs = paths();
  if (!existsSync(dirs.data)) mkdirSync(dirs.data, { recursive: true });
  const env = cliEnv(inputs, dirs.data);
  const cli =
    inputs.cliPath ||
    (await installCli({ version: inputs.version, token: inputs.githubToken, dir: dirs.cli }));
  saveState("cli", cli);
  saveState("mode", inputs.mode);
  saveState("hostname", hostname);

  if (inputs.mode === "cleanup") {
    const args = ["snapshot", "rm", hostname, "--missing-ok", "--yes"];
    if (inputs.account) args.push("--account", inputs.account);
    const removed = await run(cli, args, env);
    if (removed.code !== 0) throw new Error(`Removing the preview at ${hostname} failed.`);
    setOutput("url", "");
    await comment(inputs, event, hostname, "removed", "");
    return;
  }

  let url;
  if (inputs.mode === "snapshot") {
    const name = snapshotName(inputs.name, context);
    const published = await run(cli, snapshotArgs(inputs, hostname, name), env);
    if (published.code === EXIT_HELD) throw held(published.stderr);
    if (published.code !== 0) throw new Error("Publishing the Snapshot failed (see above).");
    url = lastJson(published.stdout)?.url || `https://${hostname}`;
  } else {
    await ensureCloudflared(cli, env);
    saveState("machine", env.TEITUNNEL_MACHINE_NAME);
    const live = await startShare(cli, shareArgs(inputs, hostname), env, dirs.data, inputs.wait);
    url = live.url;
    saveState("url", url);
  }
  setOutput("url", url);
  notice(`Preview: ${url}`);
  await comment(inputs, event, hostname, "live", url);
}

if (process.argv[1]?.endsWith("main.mjs")) {
  main().catch((err) => {
    error(err.message);
    process.exitCode = 1;
  });
}
