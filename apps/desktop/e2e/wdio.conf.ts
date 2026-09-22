// End-to-end tests: drive the real app (debug build with the `e2e` feature) through
// the embedded WebDriver, with the fake cloudflared and an isolated data directory.
//
//   pnpm e2e:build && pnpm e2e
import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");
const bin = process.platform === "win32" ? ".exe" : "";
const app = join(root, "target/debug", `Teitunnel${bin}`);
const fake = join(root, "target/debug", `fake-cloudflared${bin}`);
const dataDir = mkdtempSync(join(tmpdir(), "teitunnel-e2e-"));

// A stand-in for the Cloudflare API and edge; E2E builds point at it (never at Cloudflare).
const CLOUDFLARE_PORT = 18787;
const fakeCloudflare = spawn(join(root, "target/debug", `fake-cloudflare${bin}`), [
  String(CLOUDFLARE_PORT),
]);

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: [process.env["E2E_SPEC"] ?? "./**/*.e2e.ts"],
  maxInstances: 1,
  logLevel: "warn",
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { timeout: 60_000 },
  services: [
    [
      "@wdio/tauri-service",
      {
        driverProvider: "embedded",
        env: {
          TEITUNNEL_CLOUDFLARED: fake,
          TEITUNNEL_DATA_DIR: dataDir,
          TEITUNNEL_LOG: "info",
          TEITUNNEL_API_BASE: `http://127.0.0.1:${CLOUDFLARE_PORT}`,
          TEITUNNEL_EDGE: `127.0.0.1:${CLOUDFLARE_PORT}`,
        },
      },
    ],
  ],
  capabilities: [
    { browserName: "tauri", "tauri:options": { application: app } } as WebdriverIO.Capabilities,
  ],
  // The service ends the app abruptly, so no exit hook runs: stop any connector the
  // test left behind (only the fake binary can be running in E2E builds).
  afterSession: () => {
    try {
      execFileSync("pkill", ["-f", fake]);
    } catch {
      // nothing to stop
    }
    rmSync(dataDir, { recursive: true, force: true });
  },
  onComplete: () => {
    fakeCloudflare.kill();
  },
};
