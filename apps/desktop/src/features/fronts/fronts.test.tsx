import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { FrontView, InboxItem, PlanView } from "@/lib/ipc/bindings";
import { FrontsSection } from "./components/fronts-section";

let calls: { cmd: string; args: Record<string, unknown> }[];
let fronts: FrontView[];
let refused: boolean;

const plan: PlanView = {
  steps: [
    {
      kind: "frontWorker",
      description: { key: "core.front.step.putOffline", args: { hostname: "app.xyz.com" } },
      command: null,
    },
    {
      kind: "frontWorker",
      description: { key: "core.front.step.createRoute", args: { pattern: "app.xyz.com/*" } },
      command: null,
    },
  ],
  warnings: [{ type: "workerRequests", pattern: "app.xyz.com/*" }],
  requiresConfirmation: false,
  fingerprint: "fp-1",
};

const items: InboxItem[] = [
  {
    id: "w1",
    receivedAt: Date.now() - 300_000,
    method: "POST",
    path: "/hooks/github",
    size: 120,
    deliveredAt: null,
    status: null,
    attempts: 1,
    error: "http://localhost:3000 answered 503",
  },
];

beforeEach(() => {
  calls = [];
  fronts = [];
  refused = false;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "accounts_list":
        return [{ id: "a1", name: "Personal", credential: "apiToken", limitedZone: null }];
      case "accounts_capabilities":
        return {
          zonesRead: "yes",
          tunnelsRead: "yes",
          tunnelsEdit: "yes",
          accessEdit: "yes",
          analytics: "yes",
          workersEdit: "yes",
          edgeRules: "yes",
          serviceTokens: "yes",
          d1: refused ? "no" : "yes",
          zones: [],
        };
      case "fronts_list":
        return fronts;
      case "inbox_items":
        return items;
      case "inbox_deliver":
        return [
          { hostname: "app.xyz.com", path: "/hooks/", delivered: 1, waiting: 0, error: null },
        ];
      case "fronts_preview":
        if (refused) {
          throw {
            code: "permissionDenied",
            message: { key: "core.error.observe.workersPermission", args: {} },
            hint: null,
            field: null,
          };
        }
        return plan;
      case "fronts_undo_change":
        return { type: "offline", hostname: "app.xyz.com", page: null };
      case "fronts_apply":
        return { type: "applied", tunnelId: null, verify: [], connectorError: null };
      default:
        return null;
    }
  });
});

function renderSection() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <FrontsSection accountId="a1" hostname="app.xyz.com" />
    </QueryClientProvider>,
  );
}

describe("FrontsSection", () => {
  it("turns the offline page on through a reviewed plan", async () => {
    renderSection();
    fireEvent.click(await screen.findByRole("button", { name: "Turn On…" }));
    const sheet = await screen.findByRole("dialog");
    const title = within(sheet).getByLabelText("Title");
    fireEvent.change(title, { target: { value: "Gone fishing" } });
    fireEvent.click(within(sheet).getByRole("button", { name: "Review" }));
    expect(await within(sheet).findByText(/Run it for app.xyz.com\/\*/)).toBeTruthy();
    expect(within(sheet).getByText(/100,000 free Worker requests a day/)).toBeTruthy();
    expect(calls.find((c) => c.cmd === "fronts_preview")?.args["change"]).toEqual({
      type: "offline",
      hostname: "app.xyz.com",
      page: {
        title: "Gone fishing",
        message: "This site runs on a computer that's offline right now. Try again later.",
        whenAppDown: false,
      },
    });
    fireEvent.click(within(sheet).getByRole("button", { name: "Apply" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "fronts_apply")?.args["fingerprint"]).toBe("fp-1"),
    );
    // Undo is prepared before applying.
    expect(calls.findIndex((c) => c.cmd === "fronts_undo_change")).toBeLessThan(
      calls.findIndex((c) => c.cmd === "fronts_apply"),
    );
  });

  it("shows an inbox's webhooks with arrival and delivery, and delivers now", async () => {
    fronts = [
      {
        accountId: "a1",
        hostname: "app.xyz.com",
        kind: "inbox",
        path: "/hooks/",
        page: null,
        inbox: { maxItems: 500, retentionDays: 7, verify: null },
        script: "tt-inbox-1",
        routed: true,
      },
    ];
    renderSection();
    const list = await screen.findByRole("list", { name: "Webhooks to /hooks/" });
    expect(within(list).getByText("POST /hooks/github")).toBeTruthy();
    expect(
      within(list).getByText(/Arrived .* · Waiting: http:\/\/localhost:3000 answered 503/),
    ).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Deliver Now" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "inbox_deliver")).toBe(true));
  });

  it("offers the permission fix when the token can't manage Workers or D1", async () => {
    refused = true;
    renderSection();
    fireEvent.click(await screen.findByRole("button", { name: "Add Webhook Inbox…" }));
    const sheet = await screen.findByRole("dialog");
    fireEvent.click(within(sheet).getByRole("button", { name: "Review" }));
    expect(await within(sheet).findByText("Account · D1 · Edit")).toBeTruthy();
  });
});
