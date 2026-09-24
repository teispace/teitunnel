import { connect } from "node:net";
import {
  ControlClient,
  ControlError,
  OPEN_APP_URL,
  type RouteInfo,
  type ShareInfo,
  shareLabel,
  shortUrl,
} from "@teitunnel/control-client";
import * as vscode from "vscode";
import {
  guessFromFiles,
  guessFromPackageJson,
  type PortGuess,
  parsePortInput,
  portFromPortsItem,
  unique,
} from "./detect.ts";
import { RequestNotifier, routeRow, shareRow, statusBar } from "./present.ts";

type Node =
  | { kind: "group"; group: "shares" | "routes" }
  | { kind: "share"; share: ShareInfo }
  | { kind: "route"; route: RouteInfo }
  | { kind: "message"; text: string };

let client: ControlClient;
let shares: ShareInfo[] = [];
let routes: RouteInfo[] | string | undefined;
const changed = new vscode.EventEmitter<Node | undefined>();
let output: vscode.OutputChannel;

export function activate(context: vscode.ExtensionContext): void {
  output = vscode.window.createOutputChannel("Teitunnel");
  const version = String(context.extension.packageJSON.version ?? "0.0.0");
  client = new ControlClient({
    client: { name: "vscode", version },
    events: "all",
    log: (message) => output.appendLine(message),
  });

  const status = vscode.window.createStatusBarItem(
    "teitunnel.status",
    vscode.StatusBarAlignment.Left,
    50,
  );
  status.name = "Teitunnel";
  const renderStatus = () => {
    const { text, tooltip } = statusBar(client.state, shares);
    status.text = text;
    status.tooltip = tooltip;
    status.command = client.state === "connected" ? "teitunnel.shares.focus" : "teitunnel.openApp";
    if (vscode.workspace.getConfiguration("teitunnel").get<boolean>("statusBar", true))
      status.show();
    else status.hide();
  };

  const notifier = new RequestNotifier((message) => {
    void vscode.window.showInformationMessage(message, "Open Inspector").then((choice) => {
      if (choice) void vscode.commands.executeCommand("teitunnel.openInspector");
    });
  });

  const refreshShares = async () => {
    try {
      shares = await client.listShares();
    } catch {
      shares = [];
    }
    renderStatus();
    changed.fire(undefined);
  };
  const refreshRoutes = async () => {
    try {
      routes = (await client.listRoutes()).routes;
    } catch (error) {
      routes = error instanceof ControlError ? error.message : String(error);
    }
    changed.fire(undefined);
  };

  context.subscriptions.push(
    output,
    status,
    changed,
    { dispose: () => client.close() },
    { dispose: () => notifier.dispose() },
    disposable(
      client.onState((state) => {
        void vscode.commands.executeCommand(
          "setContext",
          "teitunnel.connected",
          state === "connected",
        );
        if (state === "connected") {
          void refreshShares();
          routes = undefined;
        } else {
          shares = [];
          renderStatus();
          changed.fire(undefined);
        }
      }),
    ),
    disposable(
      client.onEvent((event) => {
        if (event.type === "sharesChanged") void refreshShares();
        else if (event.type === "routesChanged") void refreshRoutes();
        else if (event.type === "requestArrived") {
          if (!vscode.workspace.getConfiguration("teitunnel").get<boolean>("notifyRequests", false))
            return;
          const share = shares.find((s) => s.id === event.share);
          notifier.push(event, share ? shareLabel(share) : event.share);
        }
      }),
    ),
    vscode.workspace.onDidChangeConfiguration((e) => {
      if (e.affectsConfiguration("teitunnel")) renderStatus();
    }),
    vscode.window.registerTreeDataProvider("teitunnel.shares", {
      onDidChangeTreeData: changed.event,
      getTreeItem: treeItem,
      getChildren: async (node?: Node): Promise<Node[]> => {
        if (client.state !== "connected") return [];
        if (!node)
          return [
            { kind: "group", group: "shares" },
            { kind: "group", group: "routes" },
          ];
        if (node.kind !== "group") return [];
        if (node.group === "shares") {
          return shares.length === 0
            ? [{ kind: "message", text: "No shares. Use Share Port… to add one." }]
            : shares.map((share) => ({ kind: "share", share }));
        }
        if (routes === undefined) await refreshRoutes();
        if (typeof routes === "string") return [{ kind: "message", text: routes }];
        return (routes ?? []).length === 0
          ? [{ kind: "message", text: "No routes on this computer." }]
          : (routes ?? []).map((route) => ({ kind: "route", route }));
      },
    }),
    command("teitunnel.openApp", openApp),
    command("teitunnel.refresh", async () => {
      await client.connect().catch(() => {});
      await Promise.all([refreshShares(), refreshRoutes()]);
    }),
    command("teitunnel.sharePort", async (port?: unknown) => {
      const origin =
        typeof port === "string" || typeof port === "number"
          ? String(port)
          : await vscode.window.showInputBox({
              title: "Share a Port with Teitunnel",
              prompt: "A port, host:port or local URL. Anyone with the address can open it.",
              placeHolder: "3000",
              validateInput: (value) =>
                parsePortInput(value) ? undefined : "Enter a port like 3000.",
            });
      const parsed = origin && parsePortInput(origin);
      if (parsed) await share(parsed);
    }),
    command("teitunnel.sharePortsItem", async (item?: unknown) => {
      const port = portFromPortsItem(item);
      if (port) await share(String(port));
      else await vscode.commands.executeCommand("teitunnel.sharePort");
    }),
    command("teitunnel.shareDevServer", shareDevServer),
    command("teitunnel.stopShare", async (node?: Node) => {
      const target = node?.kind === "share" ? node.share : await pickShare("Stop which share?");
      if (!target) return;
      await run(async () => {
        await client.stopShare(target.id);
        vscode.window.setStatusBarMessage(`Stopped sharing ${shortUrl(target.origin)}`, 4000);
      });
    }),
    command("teitunnel.copyUrl", async (node?: Node) => {
      const url =
        node?.kind === "share"
          ? node.share.url
          : node?.kind === "route"
            ? `https://${node.route.hostname}`
            : (await pickShare("Copy which address?"))?.url;
      if (!url) return;
      await vscode.env.clipboard.writeText(url);
      vscode.window.setStatusBarMessage(`Copied ${shortUrl(url)}`, 3000);
    }),
    command("teitunnel.openInBrowser", async (node?: Node) => {
      const url =
        node?.kind === "share"
          ? node.share.url
          : node?.kind === "route"
            ? `https://${node.route.hostname}`
            : undefined;
      if (url) await vscode.env.openExternal(vscode.Uri.parse(url));
    }),
    command("teitunnel.openInspector", async (node?: Node) => {
      const target =
        node?.kind === "share" ? node.share : await pickShare("Open the requests of which share?");
      if (target) await run(() => client.open({ view: "inspector", share: target.id }));
    }),
    command("teitunnel.openInApp", async (node?: Node) => {
      await run(() =>
        client.open(
          node?.kind === "share"
            ? { view: "share", id: node.share.id }
            : node?.kind === "route"
              ? { view: "route", hostname: node.route.hostname }
              : { view: "overview" },
        ),
      );
    }),
    command("teitunnel.runDoctor", runDoctor),
  );

  renderStatus();
  void vscode.commands.executeCommand("setContext", "teitunnel.connected", false);
  client.connect().catch(() => {
    // Not running yet: the client keeps trying in the background.
  });
}

export function deactivate(): void {
  client?.close();
}

function disposable(stop: () => unknown): vscode.Disposable {
  return { dispose: () => void stop() };
}

function command(id: string, handler: (...args: never[]) => unknown): vscode.Disposable {
  return vscode.commands.registerCommand(id, handler as (...args: unknown[]) => unknown);
}

function treeItem(node: Node): vscode.TreeItem {
  switch (node.kind) {
    case "group": {
      const item = new vscode.TreeItem(
        node.group === "shares" ? "Shares" : "Routes",
        vscode.TreeItemCollapsibleState.Expanded,
      );
      item.contextValue = `group.${node.group}`;
      return item;
    }
    case "share": {
      const row = shareRow(node.share);
      const item = new vscode.TreeItem(row.label);
      item.description = row.description;
      item.tooltip = row.tooltip;
      item.contextValue = row.contextValue;
      item.iconPath = new vscode.ThemeIcon(
        node.share.status === "live"
          ? "broadcast"
          : node.share.status === "failed"
            ? "warning"
            : "loading~spin",
      );
      if (node.share.url) {
        item.command = { command: "teitunnel.copyUrl", title: "Copy Address", arguments: [node] };
      }
      return item;
    }
    case "route": {
      const row = routeRow(node.route);
      const item = new vscode.TreeItem(row.label);
      item.description = row.description;
      item.tooltip = row.tooltip;
      item.contextValue = "route";
      item.iconPath = new vscode.ThemeIcon(node.route.status === "live" ? "globe" : "warning");
      return item;
    }
    case "message": {
      const item = new vscode.TreeItem(node.text);
      item.contextValue = "message";
      return item;
    }
  }
}

async function openApp(): Promise<void> {
  await vscode.env.openExternal(vscode.Uri.parse(OPEN_APP_URL));
  client.connect().catch(() => {});
}

/** Runs a request, explaining failures (a declined change is quiet). */
async function run<T>(task: () => Promise<T>): Promise<T | undefined> {
  try {
    return await task();
  } catch (error) {
    await explain(error);
    return undefined;
  }
}

async function explain(error: unknown): Promise<void> {
  if (!(error instanceof ControlError)) {
    void vscode.window.showErrorMessage(`Teitunnel: ${String(error)}`);
    return;
  }
  if (error.declined) {
    vscode.window.setStatusBarMessage("Teitunnel: not allowed", 4000);
    return;
  }
  if (error.appUnavailable) {
    const choice = await vscode.window.showWarningMessage(error.message, "Open Teitunnel");
    if (choice) await openApp();
    return;
  }
  void vscode.window.showErrorMessage(error.message);
}

async function share(origin: string): Promise<void> {
  const shared = await vscode.window.withProgress(
    {
      location: vscode.ProgressLocation.Notification,
      title: `Sharing ${origin} with Teitunnel…`,
    },
    () => run(() => client.startShare({ origin })),
  );
  if (!shared?.url) return;
  await vscode.env.clipboard.writeText(shared.url);
  const choice = await vscode.window.showInformationMessage(
    `${shortUrl(shared.origin)} is shared at ${shared.url} (address copied).`,
    "Open in Browser",
    "Open Inspector",
  );
  if (choice === "Open in Browser") await vscode.env.openExternal(vscode.Uri.parse(shared.url));
  if (choice === "Open Inspector")
    await run(() => client.open({ view: "inspector", share: shared.id }));
}

async function pickShare(title: string): Promise<ShareInfo | undefined> {
  const list = (await run(() => client.listShares())) ?? [];
  if (list.length === 0) {
    void vscode.window.showInformationMessage("Nothing is shared with Teitunnel right now.");
    return undefined;
  }
  if (list.length === 1) return list[0];
  const picked = await vscode.window.showQuickPick(
    list.map((s) => ({ label: shareLabel(s), description: shortUrl(s.origin), share: s })),
    { title },
  );
  return picked?.share;
}

/** Whether something accepts connections on a local port (within 300 ms). */
function listening(port: number): Promise<boolean> {
  return new Promise((resolve) => {
    const socket = connect({ host: "127.0.0.1", port });
    const done = (up: boolean) => {
      socket.destroy();
      resolve(up);
    };
    socket.setTimeout(300, () => done(false));
    socket.once("connect", () => done(true));
    socket.once("error", () => done(false));
  });
}

async function workspaceGuesses(): Promise<PortGuess[]> {
  const guesses: PortGuess[] = [];
  for (const folder of vscode.workspace.workspaceFolders ?? []) {
    try {
      const bytes = await vscode.workspace.fs.readFile(
        vscode.Uri.joinPath(folder.uri, "package.json"),
      );
      guesses.push(...guessFromPackageJson(new TextDecoder().decode(bytes)));
    } catch {
      // No package.json here.
    }
    try {
      const entries = await vscode.workspace.fs.readDirectory(folder.uri);
      const names = entries.map(([name]) => name);
      const bin = names.includes("bin")
        ? (await vscode.workspace.fs.readDirectory(vscode.Uri.joinPath(folder.uri, "bin"))).map(
            ([n]) => `bin/${n}`,
          )
        : [];
      guesses.push(...guessFromFiles([...names, ...bin]));
    } catch {
      // Unreadable folder.
    }
  }
  return unique(guesses);
}

async function shareDevServer(): Promise<void> {
  const guesses = await workspaceGuesses();
  const running: PortGuess[] = [];
  for (const guess of guesses) if (await listening(guess.port)) running.push(guess);
  const alreadyShared = (port: number) =>
    shares.find(
      (s) => new RegExp(`(localhost|127\\.0\\.0\\.1):${port}/?$`).test(s.origin) && s.url,
    );
  let target: PortGuess | undefined;
  if (running.length === 1) target = running[0];
  else if (running.length > 1) {
    const picked = await vscode.window.showQuickPick(
      running.map((g) => ({ label: `localhost:${g.port}`, description: g.source, guess: g })),
      { title: "Share which dev server?" },
    );
    target = picked?.guess;
    if (!picked) return;
  }
  if (!target) {
    const hint = guesses[0];
    const choice = await vscode.window.showWarningMessage(
      hint
        ? `No dev server is running on port ${hint.port} (from ${hint.source}). Start it, then try again.`
        : "Couldn't find this workspace's dev server.",
      "Share a Port…",
    );
    if (choice) await vscode.commands.executeCommand("teitunnel.sharePort");
    return;
  }
  const existing = alreadyShared(target.port);
  if (existing?.url) {
    await vscode.env.clipboard.writeText(existing.url);
    void vscode.window.showInformationMessage(
      `localhost:${target.port} is already shared at ${existing.url} (address copied).`,
    );
    return;
  }
  await share(String(target.port));
}

async function runDoctor(): Promise<void> {
  const issues = await vscode.window.withProgress(
    { location: vscode.ProgressLocation.Notification, title: "Teitunnel is checking…" },
    () => run(() => client.runDoctor()),
  );
  if (!issues) return;
  if (issues.length === 0) {
    void vscode.window.showInformationMessage("Teitunnel's Doctor found no problems.");
    return;
  }
  const icon = (severity: string) =>
    severity === "error" ? "$(error)" : severity === "warning" ? "$(warning)" : "$(info)";
  const picked = await vscode.window.showQuickPick(
    issues.map((issue) => ({
      label: `${icon(issue.severity)} ${issue.title}`,
      description: issue.subject,
      detail: issue.detail,
    })),
    {
      title: `Teitunnel's Doctor found ${issues.length} ${issues.length === 1 ? "problem" : "problems"}`,
      placeHolder: "Pick one to fix it in Teitunnel",
    },
  );
  if (picked) await run(() => client.open({ view: "doctor" }));
}
