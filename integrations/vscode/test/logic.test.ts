import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { ShareInfo } from "@teitunnel/control-client";
import {
  explicitPort,
  guessFromFiles,
  guessFromPackageJson,
  parsePortInput,
  portFromPortsItem,
} from "../src/detect.ts";
import { RequestNotifier, routeRow, shareRow, statusBar } from "../src/present.ts";

const share = (over: Partial<ShareInfo> = {}): ShareInfo => ({
  id: "qs-1",
  kind: "quick",
  url: "https://calm-river.trycloudflare.com",
  origin: "http://localhost:5173",
  status: "live",
  startedAt: 1,
  expiresAt: null,
  requests: 3,
  accountId: null,
  ...over,
});

describe("dev server detection", () => {
  it("reads explicit ports in scripts", () => {
    assert.equal(explicitPort("vite --port 5174"), 5174);
    assert.equal(explicitPort("next dev -p 3001"), 3001);
    assert.equal(explicitPort("PORT=4000 node server.js"), 4000);
    assert.equal(explicitPort("astro dev --port=4322"), 4322);
    assert.equal(explicitPort("vite"), undefined);
  });

  it("guesses from package.json scripts, dev scripts first", () => {
    const pkg = JSON.stringify({
      scripts: {
        build: "vite build",
        preview: "vite preview",
        dev: "vite",
        "dev:api": "PORT=4000 tsx watch api.ts",
      },
    });
    assert.deepEqual(guessFromPackageJson(pkg), [
      { port: 5173, source: "dev (vite)" },
      { port: 4173, source: "preview (vite preview)" },
      { port: 4000, source: "dev:api" },
    ]);
    assert.deepEqual(guessFromPackageJson(JSON.stringify({ scripts: { dev: "next dev" } })), [
      { port: 3000, source: "dev (next)" },
    ]);
    assert.deepEqual(guessFromPackageJson(JSON.stringify({ scripts: { start: "ng serve" } })), [
      { port: 4200, source: "start (angular)" },
    ]);
    assert.deepEqual(guessFromPackageJson("not json"), []);
    assert.deepEqual(guessFromPackageJson("{}"), []);
  });

  it("knows other frameworks by their files", () => {
    assert.deepEqual(guessFromFiles(["manage.py", "README.md"]), [
      { port: 8000, source: "manage.py (django)" },
    ]);
    assert.deepEqual(guessFromFiles(["bin/rails", "Gemfile"]), [{ port: 3000, source: "rails" }]);
  });

  it("reads the port of a Ports view item", () => {
    assert.equal(
      portFromPortsItem({
        remoteHost: "localhost",
        remotePort: 3000,
        localAddress: "localhost:3001",
      }),
      3001,
    );
    assert.equal(portFromPortsItem({ remoteHost: "localhost", remotePort: 8080 }), 8080);
    assert.equal(portFromPortsItem({ localAddress: "http://127.0.0.1:5173/" }), 5173);
    assert.equal(portFromPortsItem(undefined), undefined);
  });

  it("accepts ports people type", () => {
    assert.equal(parsePortInput(" 3000 "), "3000");
    assert.equal(parsePortInput(":8080"), "8080");
    assert.equal(parsePortInput("localhost:5173"), "localhost:5173");
    assert.equal(parsePortInput("http://127.0.0.1:3000"), "http://127.0.0.1:3000");
    assert.equal(parsePortInput("70000"), undefined);
    assert.equal(parsePortInput("hello world"), undefined);
  });
});

describe("presentation", () => {
  it("counts live shares in the status bar and explains when the app is away", () => {
    assert.equal(
      statusBar("connected", [share(), share({ id: "qs-2", status: "starting", url: null })]).text,
      "$(loading~spin) 1",
    );
    assert.equal(statusBar("connected", []).text, "$(broadcast) Teitunnel");
    const away = statusBar("disconnected", []);
    assert.equal(away.text, "$(debug-disconnect) Teitunnel");
    assert.match(away.tooltip, /isn't running/);
    assert.match(
      statusBar("connected", [share()]).tooltip,
      /calm-river\.trycloudflare\.com → localhost:5173 \(live\)/,
    );
  });

  it("describes shares and routes in the view", () => {
    const row = shareRow(share());
    assert.equal(row.label, "calm-river.trycloudflare.com");
    assert.equal(row.description, "localhost:5173 · live");
    assert.equal(row.contextValue, "share.live");
    assert.match(row.tooltip, /3 requests/);
    assert.equal(shareRow(share({ url: null, status: "starting" })).contextValue, "share");
    const route = routeRow({
      hostname: "app.example.com",
      path: null,
      origin: "http://localhost:3000",
      status: "live",
      statusText: "Live",
      login: null,
      connect: null,
      tunnelId: "t1",
      tunnelName: "laptop",
      temporary: false,
    });
    assert.deepEqual(
      [route.label, route.description],
      ["app.example.com", "localhost:3000 · Live"],
    );
  });

  it("shows requests at most once per interval, counting bursts", () => {
    let now = 0;
    const shown: string[] = [];
    const notifier = new RequestNotifier(
      (m) => shown.push(m),
      5000,
      () => now,
    );
    const request = (path: string) => ({
      type: "requestArrived" as const,
      share: "qs-1",
      method: "POST",
      path,
      status: 200,
      durationMs: 12,
    });
    notifier.push(request("/hook"), "calm-river.trycloudflare.com");
    assert.deepEqual(shown, ["POST /hook → 200 in 12 ms on calm-river.trycloudflare.com"]);
    now = 1000;
    notifier.push(request("/a"), "calm-river.trycloudflare.com");
    notifier.push(request("/b"), "calm-river.trycloudflare.com");
    assert.equal(shown.length, 1, "waits for the interval");
    notifier.flush();
    assert.equal(shown[1], "2 requests on calm-river.trycloudflare.com");
    notifier.dispose();
  });
});
