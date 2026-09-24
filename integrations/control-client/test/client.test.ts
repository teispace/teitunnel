import assert from "node:assert/strict";
import { rm } from "node:fs/promises";
import { join } from "node:path";
import { afterEach, describe, it } from "node:test";
import {
  ControlClient,
  ControlError,
  type ControlEvent,
  dataDir,
  ErrorCode,
  LineReader,
  openAppCommand,
  shareLabel,
} from "../src/index.ts";
import { FakeApp, RpcFailure } from "./fake-server.ts";

const client = { name: "test-extension", version: "1.2.3" };
const apps: FakeApp[] = [];
const clients: ControlClient[] = [];

async function fakeApp(options?: { listen?: boolean }): Promise<FakeApp> {
  const app = await FakeApp.start(options);
  apps.push(app);
  return app;
}

function connectTo(
  app: FakeApp,
  options: Partial<ConstructorParameters<typeof ControlClient>[0]> = {},
) {
  const control = new ControlClient({
    client,
    environment: {
      platform: process.platform,
      env: { TEITUNNEL_DATA_DIR: app.dataDir },
      home: "/nowhere",
    },
    backoff: { initialMs: 20, maxMs: 100 },
    ...options,
  });
  clients.push(control);
  return control;
}

function until(check: () => boolean, ms = 3000): Promise<void> {
  const deadline = Date.now() + ms;
  return new Promise((resolve, reject) => {
    const poll = () => {
      if (check()) return resolve();
      if (Date.now() > deadline) return reject(new Error("timed out waiting"));
      setTimeout(poll, 10);
    };
    poll();
  });
}

afterEach(async () => {
  for (const control of clients.splice(0)) control.close();
  for (const app of apps.splice(0)) await app.dispose();
});

describe("dataDir", () => {
  it("finds the app's folder on each platform", () => {
    assert.equal(
      dataDir({ platform: "darwin", env: {}, home: "/Users/a" }),
      "/Users/a/Library/Application Support/com.teispace.teitunnel",
    );
    assert.equal(
      dataDir({
        platform: "win32",
        env: { APPDATA: "C:\\Users\\a\\AppData\\Roaming" },
        home: "C:\\Users\\a",
      }),
      "C:\\Users\\a\\AppData\\Roaming\\com.teispace.teitunnel",
    );
    assert.equal(
      dataDir({ platform: "linux", env: {}, home: "/home/a" }),
      "/home/a/.local/share/com.teispace.teitunnel",
    );
    assert.equal(
      dataDir({ platform: "linux", env: { XDG_DATA_HOME: "/data" }, home: "/home/a" }),
      "/data/com.teispace.teitunnel",
    );
    assert.equal(
      dataDir({ platform: "darwin", env: { TEITUNNEL_DATA_DIR: "/tmp/tt" }, home: "/Users/a" }),
      "/tmp/tt",
    );
  });
});

describe("LineReader", () => {
  it("splits lines across chunks and skips blank ones", () => {
    const reader = new LineReader(64);
    assert.deepEqual(reader.push(Buffer.from('{"a":1}\r\n\n{"b"')), ['{"a":1}']);
    assert.deepEqual(reader.push(Buffer.from(":2}\n")), ['{"b":2}']);
  });

  it("refuses a line longer than the limit without buffering it", () => {
    const reader = new LineReader(8);
    assert.throws(() => reader.push(Buffer.from("x".repeat(9))), RangeError);
  });
});

describe("ControlClient", () => {
  it("says hello with the token and who it is, then answers typed calls", async () => {
    const app = await fakeApp();
    app.handlers.set("shares.list", () => [
      {
        id: "qs-1",
        kind: "quick",
        url: "https://a.trycloudflare.com",
        origin: "http://localhost:3000",
        status: "live",
        startedAt: 1,
        expiresAt: null,
        requests: 2,
        accountId: null,
      },
    ]);
    app.handlers.set("shares.start", (params) => ({
      id: "qs-2",
      kind: "quick",
      url: "https://b.trycloudflare.com",
      origin: (params as { origin: string }).origin,
      status: "live",
      startedAt: 2,
      expiresAt: null,
      requests: 0,
      accountId: null,
    }));
    app.handlers.set("shares.stop", () => ({}));
    app.handlers.set("open", () => ({}));
    const control = connectTo(app);
    const hello = await control.connect();
    assert.equal(hello.app.version, "9.9.9");
    assert.equal(control.state, "connected");
    assert.deepEqual(app.hellos[0], { protocol: 1, token: app.token, client });

    const shares = await control.listShares();
    assert.equal(shares[0] && shareLabel(shares[0]), "a.trycloudflare.com");
    const started = await control.startShare({ origin: "5173", hostHeader: { mode: "auto" } });
    assert.equal(started.url, "https://b.trycloudflare.com");
    await control.stopShare("qs-2");
    await control.open({ view: "inspector", share: "qs-1" });
    assert.deepEqual(
      app.received.map((r) => [r.method, r.params]),
      [
        ["shares.list", undefined],
        ["shares.start", { origin: "5173", hostHeader: { mode: "auto" } }],
        ["shares.stop", { id: "qs-2" }],
        ["open", { view: "inspector", share: "qs-1" }],
      ],
    );
  });

  it("reports a declined change quietly and other errors with the app's words", async () => {
    const app = await fakeApp();
    app.handlers.set("shares.stop", () => {
      throw new RpcFailure(ErrorCode.declined, "The change wasn't allowed in Teitunnel.");
    });
    app.handlers.set("routes.list", () => {
      throw new RpcFailure(ErrorCode.notFound, "No Cloudflare account is connected.");
    });
    const control = connectTo(app);
    const declined = await control.stopShare("x").catch((e: unknown) => e);
    assert.ok(declined instanceof ControlError);
    assert.equal(declined.declined, true);
    const missing = await control.listRoutes().catch((e: unknown) => e);
    assert.ok(missing instanceof ControlError);
    assert.equal(missing.kind, "rpc");
    assert.equal(missing.code, ErrorCode.notFound);
    assert.equal(missing.message, "No Cloudflare account is connected.");
  });

  it("explains when the app was never set up or isn't running", async () => {
    const nowhere = new ControlClient({
      client,
      reconnect: false,
      environment: {
        platform: process.platform,
        env: { TEITUNNEL_DATA_DIR: join("/nonexistent", "tt") },
        home: "/",
      },
    });
    const notInstalled = await nowhere.connect().catch((e: unknown) => e);
    assert.ok(notInstalled instanceof ControlError);
    assert.equal(notInstalled.kind, "notInstalled");
    assert.equal(notInstalled.appUnavailable, true);

    const app = await fakeApp({ listen: false });
    const control = connectTo(app, { reconnect: false });
    const notRunning = await control.status().catch((e: unknown) => e);
    assert.ok(notRunning instanceof ControlError);
    assert.equal(notRunning.kind, "notRunning");
    assert.match(notRunning.message, /isn't running/);
    assert.deepEqual(openAppCommand("darwin"), { command: "open", args: ["teitunnel://open"] });
  });

  it("is refused with a wrong token, and never logs or shows the token", async () => {
    const app = await fakeApp();
    const logged: string[] = [];
    const control = connectTo(app, {
      reconnect: false,
      resolve: async () => ({ path: join(app.dataDir, "control", "sock"), token: "0".repeat(64) }),
      log: (message) => logged.push(message),
    });
    if (process.platform === "win32") return; // the pipe path differs there
    const refused = await control.connect().catch((e: unknown) => e);
    assert.ok(refused instanceof ControlError);
    assert.equal(refused.kind, "unauthorized");
    const ok = connectTo(app, { log: (message) => logged.push(message) });
    await ok.connect();
    for (const line of [...logged, refused.message]) {
      assert.ok(!line.includes(app.token) && !line.includes("0".repeat(64)), line);
    }
  });

  it("subscribes to events and passes them on, ignoring nothing it doesn't know", async () => {
    const app = await fakeApp();
    const control = connectTo(app, { events: ["sharesChanged", "requestArrived"] });
    const events: ControlEvent[] = [];
    control.onEvent((event) => events.push(event));
    await control.connect();
    await until(() => app.received.some((r) => r.method === "events.subscribe"));
    assert.deepEqual(app.received.at(-1), {
      method: "events.subscribe",
      params: { events: ["sharesChanged", "requestArrived"] },
    });
    app.emit({ type: "sharesChanged", id: "qs-1" });
    app.emit({ type: "routesChanged" }); // not subscribed
    app.emit({
      type: "requestArrived",
      share: "qs-1",
      method: "GET",
      path: "/",
      status: 200,
      durationMs: 4,
    });
    await until(() => events.length === 2);
    assert.deepEqual(
      events.map((e) => e.type),
      ["sharesChanged", "requestArrived"],
    );
  });

  it("reconnects with backoff when the app restarts, and subscribes again", async () => {
    const app = await fakeApp();
    const control = connectTo(app, { events: "all" });
    const states: string[] = [];
    control.onState((state) => states.push(state));
    await control.connect();
    app.handlers.set("doctor.run", () => new Promise(() => {})); // never answers
    const pending = control.runDoctor().catch((e: unknown) => e);
    await until(() => app.received.some((r) => r.method === "doctor.run"));
    await app.stop();
    const dropped = await pending;
    assert.ok(dropped instanceof ControlError);
    assert.equal(dropped.kind, "disconnected");
    await until(() => control.state === "disconnected");
    await app.listen();
    await until(() => control.state === "connected");
    await until(() => app.received.filter((r) => r.method === "events.subscribe").length === 2);
    const events: ControlEvent[] = [];
    control.onEvent((event) => events.push(event));
    app.emit({ type: "sharesChanged" });
    await until(() => events.length === 1);
    assert.ok(states.includes("connecting") && states.at(-1) === "connected");
  });

  it("keeps trying while the app isn't running, then connects when it starts", async () => {
    const app = await fakeApp({ listen: false });
    const control = connectTo(app);
    await control.connect().catch(() => {});
    assert.equal(control.state, "disconnected");
    assert.equal(control.lastError?.kind, "notRunning");
    await app.listen();
    await until(() => control.state === "connected");
    assert.equal(app.clients, 1);
  });

  it("times requests out", async () => {
    const app = await fakeApp();
    app.handlers.set("doctor.run", () => new Promise(() => {}));
    const control = connectTo(app, { requestTimeoutMs: 50 });
    const late = await control.runDoctor().catch((e: unknown) => e);
    assert.ok(late instanceof ControlError);
    assert.equal(late.kind, "timeout");
  });

  it("stays closed after close()", async () => {
    const app = await fakeApp();
    const control = connectTo(app);
    await control.connect();
    control.close();
    assert.equal(control.state, "closed");
    await rm(join(app.dataDir, "control", "token"));
    const after = await control.status().catch((e: unknown) => e);
    assert.ok(after instanceof ControlError);
    assert.equal(after.kind, "disconnected");
  });
});
