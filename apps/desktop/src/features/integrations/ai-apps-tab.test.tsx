import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { AiClientsView, AiClientView } from "@/lib/ipc/bindings";
import { AiAppsTab } from "./ai-apps-tab";

const cli = "/Applications/Teitunnel.app/Contents/MacOS/teitunnel-cli";
let view: AiClientsView;
let calls: { cmd: string; args: unknown }[];
let testFails: boolean;

function client(id: string, name: string, extra: Partial<AiClientView> = {}): AiClientView {
  return {
    id,
    name,
    state: "notInstalled",
    path: `/Users/me/.${id}/mcp.json`,
    installedAt: null,
    command: null,
    lastUsedAt: null,
    snippet: `{"mcpServers":{"teitunnel":{"command":"${cli}","args":["mcp"]}}}`,
    problem: null,
    ...extra,
  };
}

beforeEach(() => {
  calls = [];
  testFails = false;
  view = {
    command: cli,
    clients: [
      client("claude-code", "Claude Code", {
        state: "connected",
        installedAt: "/Users/me/.local/bin/claude",
        command: `${cli} mcp`,
        lastUsedAt: Date.now() - 2 * 60_000,
      }),
      client("cursor", "Cursor", {
        state: "needsUpdate",
        installedAt: "/Applications/Cursor.app",
        command: "/Volumes/Teitunnel/teitunnel-cli mcp",
      }),
      client("vscode", "VS Code", {
        state: "notConnected",
        installedAt: "/Applications/Visual Studio Code.app",
      }),
      client("zed", "Zed", {
        state: "unreadable",
        installedAt: "/Applications/Zed.app",
        problem: "not valid JSON",
      }),
      client("windsurf", "Windsurf"),
    ],
  };
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "ai_clients_status") return view;
    if (cmd === "ai_clients_connect" || cmd === "ai_clients_disconnect") {
      const { clientId } = args as { clientId: string };
      view = {
        ...view,
        clients: view.clients.map((c) =>
          c.id === clientId
            ? {
                ...c,
                state: cmd === "ai_clients_connect" ? "connected" : "notConnected",
                command: cmd === "ai_clients_connect" ? `${cli} mcp` : null,
              }
            : c,
        ),
      };
      return view;
    }
    if (cmd === "ai_clients_test") {
      if (testFails) {
        throw {
          code: "internal",
          message: {
            key: "core.raw",
            args: { text: "The connection didn't work. The server stopped before answering." },
          },
          hint: null,
          field: null,
        };
      }
      return { server: "teitunnel 0.4.1", protocol: "2025-11-25", tools: 41, millis: 180 };
    }
    return null;
  });
});

function renderTab() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <AiAppsTab />
    </QueryClientProvider>,
  );
}

/** The row of `name`: its label sits two levels below the row, next to its buttons. */
function row(name: string) {
  const container = screen.getByText(name).parentElement?.parentElement;
  if (!(container instanceof HTMLElement)) throw new Error(`no row for ${name}`);
  return container;
}

describe("AiAppsTab", () => {
  it("lists apps found on this computer with where each stands", async () => {
    renderTab();
    expect(await screen.findByText("Claude Code")).toBeTruthy();
    expect(screen.getByText("Connected · last used 2 min ago")).toBeTruthy();
    expect(screen.getByText(/moved or changed: update it · not used yet/)).toBeTruthy();
    expect(screen.getByText("Not connected")).toBeTruthy();
    expect(screen.getByText(/can't be read: not valid JSON/)).toBeTruthy();
    // A file that can't be read is never touched.
    expect(within(row("Zed")).queryByRole("button")).toBeNull();
    // Apps that aren't installed are apart, with their settings to add by hand.
    expect(screen.getByText("Other Supported Apps (1)")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Connect" })).toBeTruthy();
    expect(screen.getByText(`${cli} mcp`)).toBeTruthy();
  });

  it("connects, updates and disconnects an app", async () => {
    renderTab();
    fireEvent.click(
      within(await waitFor(() => row("VS Code"))).getByRole("button", { name: "Connect" }),
    );
    await waitFor(() =>
      expect(calls).toContainEqual({ cmd: "ai_clients_connect", args: { clientId: "vscode" } }),
    );
    expect(await within(row("VS Code")).findByRole("button", { name: "Disconnect" })).toBeTruthy();

    fireEvent.click(within(row("Cursor")).getByRole("button", { name: "Update" }));
    await waitFor(() =>
      expect(calls).toContainEqual({ cmd: "ai_clients_connect", args: { clientId: "cursor" } }),
    );

    fireEvent.click(within(row("Claude Code")).getByRole("button", { name: "Disconnect" }));
    await waitFor(() =>
      expect(calls).toContainEqual({
        cmd: "ai_clients_disconnect",
        args: { clientId: "claude-code" },
      }),
    );
  });

  it("tests a connection and says what answered, or why it didn't", async () => {
    renderTab();
    fireEvent.click(
      within(await waitFor(() => row("Claude Code"))).getByRole("button", {
        name: "Test Connection",
      }),
    );
    expect(
      await screen.findByText("Works: teitunnel 0.4.1, 41 tools, answered in 180 ms."),
    ).toBeTruthy();
    testFails = true;
    fireEvent.click(within(row("Claude Code")).getByRole("button", { name: "Test Connection" }));
    expect(await screen.findByRole("alert")).toHaveProperty(
      "textContent",
      "Doesn't work. The connection didn't work. The server stopped before answering.",
    );
  });

  it("explains when this copy has no command line tool", async () => {
    view = { command: null, clients: [] };
    renderTab();
    expect(await screen.findByText(/isn't part of this copy of Teitunnel/)).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
