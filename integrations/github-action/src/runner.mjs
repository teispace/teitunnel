// The GitHub Actions runner's file-based commands, and running the CLI without a shell.

import { spawn } from "node:child_process";
import { appendFileSync, readFileSync } from "node:fs";

/** Prints a workflow command (`::name::message`). */
function command(name, message) {
  const escaped = String(message).replace(/%/g, "%25").replace(/\r/g, "%0D").replace(/\n/g, "%0A");
  process.stdout.write(`::${name}::${escaped}\n`);
}

export const log = (message) => process.stdout.write(`${message}\n`);
export const notice = (message) => command("notice", message);
export const warning = (message) => command("warning", message);
export const error = (message) => command("error", message);

/** Keeps a secret out of the logs (every line of it). */
export function mask(secret) {
  for (const line of String(secret).split(/\r?\n/)) {
    if (line.trim()) process.stdout.write(`::add-mask::${line}\n`);
  }
}

function append(file, name, value) {
  if (!file) return;
  if (/[\r\n]/.test(value)) throw new Error(`${name} can't span lines.`);
  appendFileSync(file, `${name}=${value}\n`);
}

/** Sets a step output. */
export const setOutput = (name, value) => append(process.env.GITHUB_OUTPUT, name, String(value));

/** Saves a value for the post step (read back as `STATE_<name>`). */
export const saveState = (name, value) => append(process.env.GITHUB_STATE, name, String(value));

/** Reads a value the main step saved. */
export const getState = (name) => process.env[`STATE_${name}`] ?? "";

/** The workflow's event payload, or `{}`. */
export function readEvent(env = process.env) {
  try {
    return env.GITHUB_EVENT_PATH ? JSON.parse(readFileSync(env.GITHUB_EVENT_PATH, "utf8")) : {};
  } catch {
    return {};
  }
}

/**
 * Runs `program` with discrete arguments (never a shell), echoing its output, and
 * resolves with its exit code and output.
 */
export function run(program, args, env) {
  return new Promise((resolve, reject) => {
    const child = spawn(program, args, { env, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
      process.stdout.write(chunk);
    });
    child.stderr.on("data", (chunk) => {
      stderr += chunk;
      process.stderr.write(chunk);
    });
    child.on("error", reject);
    child.on("close", (code) => resolve({ code: code ?? 1, stdout, stderr }));
  });
}
