import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { AiClientsView, AiClientView } from "@/lib/ipc/bindings";
import { AiTools } from "./ai-tools";

let view: AiClientsView;
let calls: { cmd: string; args: unknown }[];
let release: (() => void) | null;

function client(id: string, name: string, extra: Partial<AiClientView> = {}): AiClientView {
  return {
    id,
    name,
    path: `/home/me/.${id}/mcp.json`,
    detected: false,
    connected: false,
    problem: null,
    ...extra,
  };
}

beforeEach(() => {
  calls = [];
  release = null;
  view = {
    command: "/Applications/Teitunnel.app/Contents/MacOS/teitunnel-cli",
    clients: [
      client("claude-code", "Claude Code"),
      client("cursor", "Cursor", { detected: true }),
      client("zed", "Zed", { detected: true, problem: "not valid JSON" }),
    ],
  };
  mockIPC(async (cmd, args) => {
    calls.push({ cmd, args });
    if (cmd === "ai_clients_status") return view;
    if (cmd === "ai_clients_connect" || cmd === "ai_clients_disconnect") {
      const { clientId } = args as { clientId: string };
      // Hold the answer until the test lets it go, to see the pending state.
      await new Promise<void>((resolve) => {
        release = resolve;
      });
      view = {
        ...view,
        clients: view.clients.map((c) =>
          c.id === clientId ? { ...c, connected: cmd === "ai_clients_connect" } : c,
        ),
      };
      return view;
    }
    return null;
  });
});

/** Lets the held connect/disconnect answer go. */
function unblock() {
  release?.();
}

function renderTools() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <AiTools />
    </QueryClientProvider>,
  );
}

function row(name: string) {
  // A grouped row: its label sits two levels below the row, next to its buttons.
  const container = screen.getByText(name).parentElement?.parentElement;
  if (!(container instanceof HTMLElement)) throw new Error(`no row for ${name}`);
  return container;
}

describe("AiTools", () => {
  it("lists installed tools first with their state", async () => {
    renderTools();
    expect(await screen.findByText("Cursor")).toBeTruthy();
    const order = ["Cursor", "Zed", "Claude Code"].map((name) => screen.getByText(name));
    for (const [before, after] of [
      [order[0], order[1]],
      [order[1], order[2]],
    ] as const) {
      expect(before?.compareDocumentPosition(after as Node)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    }
    expect(screen.getByText("Not connected")).toBeTruthy();
    expect(screen.getByText("Not found on this computer")).toBeTruthy();
    expect(screen.getByText(/can't be read: not valid JSON/)).toBeTruthy();
    // A tool whose settings can't be read isn't touched.
    expect(within(row("Zed")).getByRole("button", { name: "Connect" })).toHaveProperty(
      "disabled",
      true,
    );
    // The command for other tools.
    expect(screen.getByText(/teitunnel-cli mcp$/)).toBeTruthy();
  });

  it("connects and disconnects a tool, busy while it works", async () => {
    renderTools();
    const connect = within(await waitFor(() => row("Cursor"))).getByRole("button", {
      name: "Connect",
    });
    fireEvent.click(connect);
    await waitFor(() => expect(release).not.toBeNull());
    expect(connect.getAttribute("aria-busy")).toBe("true");
    // Other rows wait meanwhile.
    expect(within(row("Claude Code")).getByRole("button")).toHaveProperty("disabled", true);
    unblock();
    expect(await within(row("Cursor")).findByRole("button", { name: "Disconnect" })).toBeTruthy();
    expect(calls.find((c) => c.cmd === "ai_clients_connect")?.args).toEqual({ clientId: "cursor" });

    release = null;
    fireEvent.click(within(row("Cursor")).getByRole("button", { name: "Disconnect" }));
    await waitFor(() => expect(release).not.toBeNull());
    unblock();
    expect(await within(row("Cursor")).findByRole("button", { name: "Connect" })).toBeTruthy();
  });

  it("explains when this copy has no command line tool", async () => {
    view = { command: null, clients: [] };
    renderTools();
    expect(await screen.findByText(/isn't part of this copy of Teitunnel/)).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });
});
