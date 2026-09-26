import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { McpConnection } from "@/lib/ipc/bindings";
import { ServersTab } from "./servers-tab";

let connections: McpConnection[];
let disconnected: string[];

beforeEach(() => {
  disconnected = [];
  connections = [];
  mockIPC((cmd, args) => {
    if (cmd === "mcp_connections") return connections;
    if (cmd === "mcp_disconnect") {
      const id = (args as { id: string }).id;
      disconnected.push(id);
      connections = connections.filter((c) => c.id !== id);
      return null;
    }
    return null;
  });
});

const renderIt = () =>
  render(
    <QueryClientProvider client={createQueryClient()}>
      <ServersTab />
    </QueryClientProvider>,
  );

describe("apps signed in to shared MCP servers", () => {
  it("explains shared servers and where tokens are kept before any app signs in", async () => {
    renderIt();
    expect(
      await screen.findByText("No app has signed in to a server shared from this computer yet."),
    ).toBeTruthy();
    expect(screen.getByText(/only fingerprints \(SHA-256 hashes\)/)).toBeTruthy();
  });

  it("lists them and disconnects one after asking", async () => {
    connections = [
      {
        id: "ttgr_1",
        host: "mcp.teispace.com",
        clientName: "Claude",
        redirectHost: "claude.ai",
        createdAt: Date.now() - 3_600_000,
        lastUsedAt: Date.now(),
      },
    ];
    renderIt();
    expect(await screen.findByText("Claude")).toBeTruthy();
    expect(screen.getByText("Apps Signed In to mcp.teispace.com")).toBeTruthy();
    expect(screen.getByText(/Signs in through claude\.ai · approved/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Disconnect" }));
    const dialog = await screen.findByRole("dialog");
    expect(within(dialog).getByText("Disconnect Claude from mcp.teispace.com?")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Disconnect" }));
    await waitFor(() => expect(disconnected).toEqual(["ttgr_1"]));
    await waitFor(() => expect(screen.queryByText("Claude")).toBeNull());
  });
});
