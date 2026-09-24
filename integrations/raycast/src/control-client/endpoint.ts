// Vendored from integrations/control-client/src by scripts/vendor.mjs. Don't edit here.
/**
 * Where the running app listens: `<data>/control/` holds `token` and either `sock` (a
 * Unix socket, macOS and Linux) or `pipe` (a file naming a Windows named pipe).
 */

import { readFile } from "node:fs/promises";
import { homedir } from "node:os";
import { posix, win32 } from "node:path";
import { ControlError } from "./errors.ts";

/** The app's bundle identifier; its data folder is named after it. */
export const IDENTIFIER = "com.teispace.teitunnel";

const PIPE_PREFIX = "\\\\.\\pipe\\teitunnel-control-";

export interface Environment {
  platform: NodeJS.Platform;
  env: Record<string, string | undefined>;
  home: string;
}

/** This process's platform, environment and home folder. */
export function currentEnvironment(): Environment {
  return { platform: process.platform, env: process.env, home: homedir() };
}

const paths = (platform: NodeJS.Platform) => (platform === "win32" ? win32 : posix);

/**
 * The app's data folder: `TEITUNNEL_DATA_DIR`, else the platform's (as the app and
 * CLI find it): `~/Library/Application Support/…` on macOS, `%APPDATA%\…` on Windows,
 * `$XDG_DATA_HOME/…` or `~/.local/share/…` on Linux.
 */
export function dataDir(environment: Environment = currentEnvironment()): string {
  const { platform, env, home } = environment;
  const path = paths(platform);
  const override = env["TEITUNNEL_DATA_DIR"];
  if (override) return override;
  if (platform === "darwin") return path.join(home, "Library", "Application Support", IDENTIFIER);
  if (platform === "win32") {
    const appData = env["APPDATA"] || path.join(home, "AppData", "Roaming");
    return path.join(appData, IDENTIFIER);
  }
  const xdg = env["XDG_DATA_HOME"];
  const base = xdg?.startsWith("/") ? xdg : path.join(home, ".local", "share");
  return path.join(base, IDENTIFIER);
}

/** How to reach the app. */
export interface Endpoint {
  /** The socket's path or the pipe's name, for `net.connect({ path })`. */
  path: string;
  /** The token for `hello` (never log it). */
  token: string;
}

/** Reads the token file (a missing one means the app never ran here). */
async function readToken(file: string): Promise<string> {
  let text: string;
  try {
    text = await readFile(file, "utf8");
  } catch {
    throw new ControlError("notInstalled");
  }
  const token = text.trim();
  if (!/^[0-9a-fA-F]{64}$/.test(token)) throw new ControlError("notInstalled");
  return token.toLowerCase();
}

/** Where the app listens, and its token (read fresh each time: it may be recreated). */
export async function resolveEndpoint(
  environment: Environment = currentEnvironment(),
  data: string = dataDir(environment),
): Promise<Endpoint> {
  const path = paths(environment.platform);
  const dir = path.join(data, "control");
  const token = await readToken(path.join(dir, "token"));
  if (environment.platform !== "win32") return { path: path.join(dir, "sock"), token };
  let name: string;
  try {
    name = (await readFile(path.join(dir, "pipe"), "utf8")).trim();
  } catch {
    // The app hasn't opened its pipe yet: it isn't running.
    throw new ControlError("notRunning");
  }
  if (!name.startsWith(PIPE_PREFIX) || name.length !== PIPE_PREFIX.length + 32) {
    throw new ControlError("notRunning");
  }
  return { path: name, token };
}
