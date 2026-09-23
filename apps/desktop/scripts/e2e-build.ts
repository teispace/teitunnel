// Builds the E2E app and the test doubles into target/e2e (never over the normal debug
// app), on any platform. `tauri build` resolves relative paths from src-tauri, so the
// target directory is passed as an absolute path.
import { spawnSync } from "node:child_process";
import { resolve } from "node:path";

const env = {
  ...process.env,
  CARGO_TARGET_DIR: resolve(import.meta.dirname, "../../../target/e2e"),
};
const shell = process.platform === "win32"; // resolves cargo.exe and pnpm's .cmd shims

function run(command: string, args: string[]) {
  const result = spawnSync(command, args, { stdio: "inherit", env, shell });
  if (result.status !== 0) process.exit(result.status ?? 1);
}

run("cargo", ["build", "-p", "fake-cloudflared", "-p", "fake-cloudflare"]);
run("pnpm", [
  "tauri",
  "build",
  "--debug",
  "--no-bundle",
  "--features",
  "e2e",
  "--config",
  "src-tauri/e2e/tauri.e2e.json",
]);
