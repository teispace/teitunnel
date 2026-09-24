/**
 * Activates the built bundle (`dist/extension.js`, from `pnpm build`) with a stand-in
 * `vscode` module and a fake Teitunnel app, and checks that every command the manifest
 * contributes is registered and the status bar follows the app.
 */

import assert from "node:assert/strict";
import { existsSync, readFileSync } from "node:fs";
import Module, { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { after, describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { FakeApp } from "@teitunnel/control-client/testing";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const bundle = join(root, "dist", "extension.js");
const manifest = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
  contributes: { commands: { command: string }[] };
};

/** A `vscode` stand-in: records commands and the status bar, answers everything else inertly. */
function fakeVscode() {
  const commands = new Map<string, (...args: unknown[]) => unknown>();
  const status = { text: "", tooltip: "", command: "", visible: false };
  const inert: unknown = new Proxy(() => inert, {
    get: (_target, key) => (key === "then" ? undefined : inert),
    apply: () => inert,
    construct: () => inert as object,
  });
  class EventEmitter {
    event = () => ({ dispose() {} });
    fire() {}
    dispose() {}
  }
  const api = {
    EventEmitter,
    StatusBarAlignment: { Left: 1, Right: 2 },
    TreeItemCollapsibleState: { None: 0, Collapsed: 1, Expanded: 2 },
    ProgressLocation: { Notification: 15 },
    ThemeIcon: class {},
    TreeItem: class {},
    Uri: { parse: (s: string) => ({ toString: () => s }), joinPath: () => inert },
    commands: {
      registerCommand: (id: string, handler: (...args: unknown[]) => unknown) => {
        commands.set(id, handler);
        return { dispose() {} };
      },
      executeCommand: async () => undefined,
    },
    window: {
      createOutputChannel: () => ({ appendLine() {}, dispose() {} }),
      createStatusBarItem: () =>
        Object.assign(status, {
          show() {
            status.visible = true;
          },
          hide() {
            status.visible = false;
          },
          dispose() {},
        }),
      registerTreeDataProvider: () => ({ dispose() {} }),
      setStatusBarMessage: () => ({ dispose() {} }),
      showInformationMessage: async () => undefined,
      showWarningMessage: async () => undefined,
      showErrorMessage: async () => undefined,
    },
    workspace: {
      getConfiguration: () => ({ get: (_key: string, fallback: unknown) => fallback }),
      onDidChangeConfiguration: () => ({ dispose() {} }),
      workspaceFolders: [],
    },
    env: { openExternal: async () => true, clipboard: { writeText: async () => {} } },
  };
  return { api, commands, status };
}

describe("the built extension", {
  skip: existsSync(bundle) ? false : "run `pnpm build` first",
}, () => {
  let app: FakeApp | undefined;
  const subscriptions: { dispose(): unknown }[] = [];

  after(async () => {
    for (const s of subscriptions) s.dispose();
    await app?.dispose();
  });

  it("registers every contributed command and shows the app's shares", async () => {
    app = await FakeApp.start();
    app.handlers.set("shares.list", () => [
      {
        id: "qs-1",
        kind: "quick",
        url: "https://a.trycloudflare.com",
        origin: "http://localhost:3000",
        status: "live",
        startedAt: 1,
        expiresAt: null,
        requests: 0,
        accountId: null,
      },
    ]);
    process.env["TEITUNNEL_DATA_DIR"] = app.dataDir;
    const { api, commands, status } = fakeVscode();
    const load = (Module as unknown as { _load: (request: string, ...rest: unknown[]) => unknown })
      ._load;
    (Module as unknown as { _load: typeof load })._load = (request, ...rest) =>
      request === "vscode" ? api : load(request, ...rest);
    const extension = createRequire(import.meta.url)(bundle) as {
      activate: (context: unknown) => void;
      deactivate: () => void;
    };
    extension.activate({ subscriptions, extension: { packageJSON: { version: "0.1.0" } } });
    subscriptions.push({ dispose: () => extension.deactivate() });

    for (const { command } of manifest.contributes.commands) {
      assert.ok(commands.has(command), `${command} is registered`);
    }
    const deadline = Date.now() + 3000;
    while (status.text !== "$(broadcast) 1" && Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    assert.equal(status.text, "$(broadcast) 1");
    assert.equal(status.command, "teitunnel.shares.focus");
    assert.deepEqual(
      app.hellos.map((h) => (h as { client: unknown }).client),
      [{ name: "vscode", version: "0.1.0" }],
    );
  });
});
