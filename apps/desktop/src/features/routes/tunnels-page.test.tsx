import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { RemoteLogsView, TunnelSummary } from "@/lib/ipc/bindings";
import { TunnelsPage } from "./tunnels-page";

let calls: { cmd: string; args: Record<string, unknown> }[];
let remote: RemoteLogsView;

const server: TunnelSummary = {
  id: "t-server",
  name: "home-lab",
  status: "healthy",
  createdAt: "2026-06-01T00:00:00Z",
  routes: 2,
  connectors: [
    {
      id: "c-server",
      version: "2026.8.0",
      originIp: "198.51.100.24",
      thisMac: false,
      connections: [
        { colo: "lhr01", openedAt: "" },
        { colo: "cdg02", openedAt: "" },
      ],
    },
  ],
  thisMac: false,
  connector: null,
};

beforeEach(() => {
  calls = [];
  remote = {
    state: { state: "streaming" },
    lines: [
      {
        time: "2026-09-23T00:00:00Z",
        level: "error",
        message: "Request failed",
        error: "dial tcp 127.0.0.1:8123: connect: connection refused",
      },
    ],
  };
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "accounts_list":
        return [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
      case "tunnels_list":
        return [server];
      case "tunnels_remote_logs":
        return remote;
      default:
        return null;
    }
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>
        <TunnelsPage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("TunnelsPage", () => {
  it("lists each machine running a tunnel and streams its logs while open", async () => {
    renderPage();
    expect(await screen.findByText("198.51.100.24")).toBeTruthy();
    expect(screen.getByText("cloudflared 2026.8.0 · LHR01, CDG02")).toBeTruthy();

    fireEvent.click(screen.getByRole("button", { name: "Logs" }));
    const sheet = await screen.findByRole("dialog", { name: "Connector Logs" });
    expect(await within(sheet).findByText(/connection refused/)).toBeTruthy();
    expect(calls.find((c) => c.cmd === "tunnels_remote_logs")?.args).toMatchObject({
      accountId: "acc",
      tunnelId: "t-server",
      connectorId: "c-server",
    });

    fireEvent.click(within(sheet).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "tunnels_remote_logs_stop")).toBe(true));
  });

  it("says why a stream ended and can try again", async () => {
    remote = {
      state: { state: "ended", message: "This connector already streams its logs elsewhere." },
      lines: [],
    };
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Logs" }));
    const sheet = await screen.findByRole("dialog", { name: "Connector Logs" });
    expect(await within(sheet).findByRole("alert")).toBeTruthy();
    remote = { state: { state: "streaming" }, lines: [] };
    fireEvent.click(within(sheet).getByRole("button", { name: "Try Again" }));
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === "tunnels_remote_logs_stop").length).toBe(1),
    );
    await waitFor(() => expect(within(sheet).queryByRole("alert")).toBeNull());
  });
});
