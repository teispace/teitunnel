# Editor and launcher integrations (checked 2026-09-25)

Facts the VS Code, Raycast and JetBrains integrations (`integrations/`, M12-07) rely on.

## VS Code (and Cursor, Windsurf)

- **No stable API reaches the Ports view's data.** `@types/vscode` 1.138.0 (the current
  stable API) has no `openTunnel`, `TunnelDescription` or `registerPortAttributesProvider`;
  the only port-related stable call is `env.asExternalUri`, which forwards a port for a
  remote window. Forwarded ports (`workspace.openTunnel`, `workspace.tunnels`) are the
  **proposed** `tunnels` API, and port attributes the **proposed** `portsAttributes` API
  (issue #115616). Proposed APIs can't be used by extensions published to the Marketplace
  or Open VSX.
  Sources: `@types/vscode@1.138.0/index.d.ts`;
  <https://github.com/microsoft/vscode/blob/main/src/vscode-dts/vscode.proposed.tunnels.d.ts>;
  <https://github.com/microsoft/vscode/blob/main/src/vscode-dts/vscode.proposed.portsAttributes.d.ts>.
- **The Ports view's item context menu is a stable contribution point:** `ports/item/context`
  (also `ports/item/origin/inline`, `ports/item/port/inline`), with no `proposed` flag in
  `menusExtensionPoint.ts`. The view calls commands with `shouldForwardArgs` and the item
  (`node.strip()`: `remoteHost`, `remotePort`, `localAddress`, …). So the extension adds
  "Share with Teitunnel" to that menu and reads the port from the argument.
  Source: <https://github.com/microsoft/vscode/blob/main/src/vs/workbench/services/actions/common/menusExtensionPoint.ts>,
  `src/vs/workbench/contrib/remote/browser/tunnelView.ts` (`menuId: MenuId.TunnelContext`,
  `getActionsContext: () => node?.strip()`).
- **Where it runs:** `"extensionKind": ["ui"]` keeps the extension on the local machine in
  remote windows (SSH, containers, WSL), where the Teitunnel app and its socket are; a
  forwarded port's `localAddress` is then a local port the app can share.
- **Cursor and Windsurf** install extensions from Open VSX; a plain `vsce package` `.vsix`
  with a `publisher`, `repository` and `license` publishes there unchanged (`ovsx publish`).
  The engine range is kept at `^1.90.0` because forks trail VS Code's version.

## Raycast

- Manifest (package.json): required `name`, `title`, `description`, `icon` (PNG, at least
  512×512), `author` (Store handle), `platforms` (`"macOS"`, `"Windows"`), `categories`,
  `commands` (`name` = entry file in `src/`, `title`, `description`, `mode`: `view`,
  `no-view` or `menu-bar`; optional `arguments`, `preferences`). Store extensions are MIT
  and use npm with a committed `package-lock.json`.
  Source: <https://developers.raycast.com/information/manifest>.
- Extensions run in Node inside Raycast, so `node:net` reaches the Unix socket or named pipe.

## JetBrains

- IntelliJ Platform Gradle Plugin 2.x (`org.jetbrains.intellij.platform`) is the current
  build plugin; the 1.x `org.jetbrains.intellij` plugin is deprecated.
  Source: <https://plugins.jetbrains.com/docs/intellij/tools-intellij-platform-gradle-plugin.html>.
- Java 16+ `UnixDomainSocketAddress` with `SocketChannel.open(StandardProtocolFamily.UNIX)`
  reaches a Unix socket (macOS, Linux); a Windows named pipe opens as a file
  (`RandomAccessFile("\\\\.\\pipe\\…", "rw")`).
