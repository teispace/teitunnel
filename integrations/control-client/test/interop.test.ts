/**
 * Against the real Rust server (`crates/control`, over its scripted host): set
 * `TEITUNNEL_FAKE_APP` to the `fake_app` example binary
 * (`cargo build -p teitunnel-control --features testing --example fake_app`,
 * then `target/debug/examples/fake_app`). Skipped without it.
 */

import assert from "node:assert/strict";
import { type ChildProcess, spawn } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { after, before, describe, it } from "node:test";
import { ControlClient, ControlError, type ControlEvent } from "../src/index.ts";

const binary = process.env["TEITUNNEL_FAKE_APP"];

describe("against the app's control server", {
  skip: binary ? false : "TEITUNNEL_FAKE_APP isn't set",
}, () => {
  let dir = "";
  let app: ChildProcess | undefined;
  let control: ControlClient;

  before(async () => {
    dir = await mkdtemp(join(tmpdir(), "tt-"));
    const child = spawn(binary ?? "", [dir], { stdio: ["ignore", "pipe", "inherit"] });
    app = child;
    await new Promise<void>((resolve, reject) => {
      child.once("error", reject);
      child.stdout?.on("data", (data: Buffer) => {
        if (data.toString().includes("listening")) resolve();
      });
    });
    control = new ControlClient({
      client: { name: "interop-test", version: "1.0.0" },
      environment: { platform: process.platform, env: { TEITUNNEL_DATA_DIR: dir }, home: dir },
      events: ["sharesChanged"],
    });
  });

  after(async () => {
    control.close();
    app?.kill();
    await rm(dir, { recursive: true, force: true });
  });

  it("says hello and reads the app", async () => {
    const hello = await control.connect();
    assert.equal(hello.protocol, 1);
    assert.equal(hello.app.version, "9.9.9");
    assert.ok(hello.methods.includes("shares.start"));
    const status = await control.status();
    assert.deepEqual(status.accounts, [{ id: "a1", name: "Personal" }]);
    const [share] = await control.listShares();
    assert.equal(share?.url, "https://qs-1.trycloudflare.com");
    assert.equal(share?.kind, "quick");
    const routes = await control.listRoutes();
    assert.equal(routes.account.name, "Personal");
    const plan = await control.previewRoutes({
      change: { type: "addRoute", route: { hostname: "a.example.com", origin: "3000" } },
    });
    assert.equal(plan.fingerprint, "f1");
    assert.deepEqual(await control.runDoctor(), []);
    await control.open({ view: "doctor" });
  });

  it("gets a change allowed once, then declined", async () => {
    const started = await control.startShare({ origin: "5173", hostHeader: { mode: "auto" } });
    assert.equal(started.id, "qs-2");
    const declined = await control.stopShare("qs-1").catch((e: unknown) => e);
    assert.ok(declined instanceof ControlError);
    assert.equal(declined.kind, "declined");
  });

  it("receives the events it subscribed to", async () => {
    const event = await new Promise<ControlEvent>((resolve) => {
      const stop = control.onEvent((e) => {
        stop();
        resolve(e);
      });
    });
    assert.deepEqual(event, { type: "sharesChanged", id: "qs-1" });
  });

  it("maps the server's errors", async () => {
    const wrong = await control
      .request("open", { view: "nowhere" } as never)
      .catch((e: unknown) => e);
    assert.ok(wrong instanceof ControlError);
    assert.equal(wrong.code, -32602);
  });
});
