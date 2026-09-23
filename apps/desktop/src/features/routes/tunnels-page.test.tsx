import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { rawText } from "@/lib/i18n";
import type {
  Change,
  NetworkView,
  PlanView,
  RemoteLogsView,
  RoutesOverview,
  TunnelSummary,
} from "@/lib/ipc/bindings";
import { TunnelsPage } from "./tunnels-page";

let calls: { cmd: string; args: Record<string, unknown> }[];
let remote: RemoteLogsView;
let networks: NetworkView[];

const mac: TunnelSummary = {
  id: "t-mac",
  name: "Mac",
  status: "healthy",
  createdAt: "2026-09-01T00:00:00Z",
  routes: 0,
  connectors: [],
  thisMac: true,
  isDefault: true,
  connector: { state: "healthy", connections: 4 },
};

function networkPlan(change: Change): PlanView {
  if (change.type === "createTunnel") {
    return {
      steps: [
        {
          kind: "createTunnel",
          description: rawText(`Create tunnel “${change.name}”`),
          command: null,
        },
      ],
      warnings: [],
      requiresConfirmation: false,
      fingerprint: "fp",
    };
  }
  const network = change.type === "addNetwork" ? change.network : "";
  return {
    steps: [
      {
        kind: "networkRoute",
        description: rawText(
          change.type === "addNetwork"
            ? `Route private network ${network} to tunnel “Mac”`
            : "Remove the route for private network 192.168.1.0/24",
        ),
        command: null,
      },
    ],
    warnings: network.startsWith("8.") ? [{ type: "publicNetwork", network }] : [],
    requiresConfirmation: network.startsWith("8."),
    fingerprint: "fp",
  };
}

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
  isDefault: false,
  connector: null,
};

beforeEach(() => {
  calls = [];
  networks = [];
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
        return [mac, server];
      case "routes_overview":
        return {
          tunnel: mine,
          tunnels: [mine],
          routes: [],
          zones: [],
          networks,
        } satisfies RoutesOverview;
      case "routes_preview":
        return networkPlan(payload["change"] as Change);
      case "routes_apply": {
        const change = payload["change"] as Change;
        if (change.type === "addNetwork")
          networks = [{ network: "192.168.1.0/24", private: true, owned: true }];
        if (change.type === "removeNetwork") networks = [];
        return { type: "applied", tunnelId: "t-mac", verify: [], connectorError: null };
      }
      case "doctor_run":
        return [
          {
            id: "network.excluded:acc:192.168.1.0/24",
            check: "network.excluded",
            severity: "warning",
            accountId: "acc",
            subject: "192.168.1.0/24",
            label: rawText("192.168.1.0/24"),
            title: {
              key: "core.doctor.networkExcluded.title",
              args: { network: "192.168.1.0/24" },
            },
            detail: rawText(""),
            evidence: [],
            fixes: [],
          },
        ];
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

const mine = {
  id: "t-mac",
  name: "Mac",
  connector: { state: "healthy", connections: 4 },
  isDefault: true,
} as const;

describe("TunnelsPage", () => {
  it("creates another tunnel for this Mac through a reviewed plan", async () => {
    renderPage();
    await screen.findByRole("option", { name: /Mac/ });
    fireEvent.click(screen.getByRole("button", { name: "New tunnel" }));
    const dialog = await screen.findByRole("dialog", { name: "New Tunnel" });
    const review = within(dialog).getByRole("button", { name: "Review" });
    expect(review.hasAttribute("disabled")).toBe(true);
    fireEvent.change(within(dialog).getByRole("textbox", { name: "Name" }), {
      target: { value: "staging" },
    });
    fireEvent.click(review);
    expect(await within(dialog).findByText("Create tunnel “staging”")).toBeTruthy();
    fireEvent.click(within(dialog).getByRole("button", { name: "Create Tunnel" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "routes_apply")?.args["change"]).toEqual({
        type: "createTunnel",
        name: "staging",
      }),
    );
  });

  it("deletes the selected tunnel of this Mac, not the default one", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Delete…" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete Tunnel" });
    await within(dialog).findByRole("button", { name: "Delete Tunnel" });
    expect(calls.find((c) => c.cmd === "routes_preview")?.args["tunnelId"]).toBe("t-mac");
  });

  it("shares a private network and stops sharing it", async () => {
    renderPage();
    expect(await screen.findByText(/reach addresses on this Mac's network/)).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Share Network…" }));
    const sheet = await screen.findByRole("dialog", { name: "Share a Private Network" });
    fireEvent.change(within(sheet).getByRole("textbox", { name: "Network" }), {
      target: { value: "192.168.1.7/24" },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Review" }));
    expect(
      await within(sheet).findByText("Route private network 192.168.1.7/24 to tunnel “Mac”"),
    ).toBeTruthy();
    fireEvent.click(within(sheet).getByRole("button", { name: "Share" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "routes_apply")?.args["change"]).toEqual({
        type: "addNetwork",
        network: "192.168.1.7/24",
      }),
    );

    // Listed, with the Doctor's finding inline.
    expect(await screen.findByText("192.168.1.0/24")).toBeTruthy();
    expect(
      await screen.findByText("WARP clients don't send 192.168.1.0/24 to this Mac"),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Stop sharing 192.168.1.0/24" }));
    const stop = await screen.findByRole("dialog", { name: "Stop Sharing Network" });
    fireEvent.click(await within(stop).findByRole("button", { name: "Stop Sharing" }));
    await waitFor(() =>
      expect(calls.filter((c) => c.cmd === "routes_apply").at(-1)?.args["change"]).toEqual({
        type: "removeNetwork",
        network: "192.168.1.0/24",
      }),
    );
  });

  it("asks before sending public addresses through this Mac", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Share Network…" }));
    const sheet = await screen.findByRole("dialog", { name: "Share a Private Network" });
    fireEvent.change(within(sheet).getByRole("textbox", { name: "Network" }), {
      target: { value: "8.8.8.0/24" },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Review" }));
    expect(await within(sheet).findByText(/isn't a private range/)).toBeTruthy();
    const share = within(sheet).getByRole("button", { name: "Share" });
    expect(share.hasAttribute("disabled")).toBe(true);
    fireEvent.click(
      within(sheet).getByRole("checkbox", { name: "Send these public addresses through this Mac" }),
    );
    expect(share.hasAttribute("disabled")).toBe(false);
  });

  it("lists each machine running a tunnel and streams its logs while open", async () => {
    renderPage();
    fireEvent.mouseDown(await screen.findByRole("option", { name: /home-lab/ }));
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
      state: {
        state: "ended",
        message: rawText("This connector already streams its logs elsewhere."),
      },
      lines: [],
    };
    renderPage();
    fireEvent.mouseDown(await screen.findByRole("option", { name: /home-lab/ }));
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
