// End-to-end tests: drive the real app (debug build with the `e2e` feature) through
// the embedded WebDriver, with the fake cloudflared and an isolated data directory.
//
//   pnpm e2e:build && pnpm e2e
import { execFileSync, spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(import.meta.dirname, "../../..");
const bin = process.platform === "win32" ? ".exe" : "";
const app = join(root, "target/e2e/debug", `Teitunnel${bin}`);
const fake = join(root, "target/e2e/debug", `fake-cloudflared${bin}`);
const dataDir = mkdtempSync(join(tmpdir(), "teitunnel-e2e-"));
const artifacts = join(import.meta.dirname, "artifacts");

// A stand-in for the Cloudflare API and edge; E2E builds point at it (never at Cloudflare).
const CLOUDFLARE_PORT = 18787;
const fakeCloudflare = spawn(join(root, "target/e2e/debug", `fake-cloudflare${bin}`), [
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
        // The app's stderr and console into e2e/artifacts (CI uploads them on failure).
        captureBackendLogs: true,
        captureFrontendLogs: true,
        logDir: artifacts,
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
  // A screenshot of every failing test, for platforms nobody is watching (CI uploads
  // e2e/artifacts).
  afterTest: async (test, _context, { passed }) => {
    if (passed) return;
    mkdirSync(artifacts, { recursive: true });
    const name = `${test.parent} ${test.title}`.replace(/[^\w-]+/g, "_");
    await browser.saveScreenshot(join(artifacts, `${name}.png`));
    // What the page holds: a blank screenshot alone says little.
    const page = await browser
      .execute(() => `${location.href}\n\n${document.documentElement.outerHTML}`)
      .catch((err: unknown) => `couldn't read the page: ${String(err)}`);
    writeFileSync(join(artifacts, `${name}.html.txt`), page);
  },
  afterSession: () => {
    // Stop connectors the app left running (the fake cloudflared).
    try {
      if (process.platform === "win32") {
        execFileSync("taskkill", ["/F", "/T", "/IM", "fake-cloudflared.exe"]);
      } else {
        execFileSync("pkill", ["-f", fake]);
      }
    } catch {
      // Nothing was running.
    }
    rmSync(dataDir, { recursive: true, force: true });
  },
  onComplete: () => {
    fakeCloudflare.kill();
  },
};
