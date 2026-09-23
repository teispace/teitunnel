import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { ActivityEntry, RoutesOverview } from "@/lib/ipc/bindings";
import { ActivityPage } from "./activity-page";

const step = (description: string, command: string | null) => ({
  kind: "putConfig" as const,
  description,
  command,
});

const entries: ActivityEntry[] = [
  {
    id: 2,
    at: Date.now(),
    summary: "Add app.xyz.com → http://localhost:3000",
    outcome: "rolledBack",
    detail: ["Failed: record already exists"],
    record: {
      kind: "addRoute",
      hostnames: ["app.xyz.com"],
      tunnel: "Mac",
      steps: [
        { step: step("Update tunnel “Mac”", "curl -X PUT …"), state: { state: "undone" } },
        {
          step: step("Add DNS record app.xyz.com", null),
          state: { state: "failed", message: "record already exists" },
        },
      ],
      changes: [
        {
          area: "route",
          hostname: "app.xyz.com",
          path: null,
          before: null,
          after: "http://localhost:3000",
        },
      ],
    },
  },
  {
    id: 1,
    at: Date.now() - 60_000,
    summary: "Add api.yx.com → http://localhost:8000",
    outcome: "applied",
    detail: [],
    record: {
      kind: "addRoute",
      hostnames: ["api.yx.com"],
      tunnel: "Mac",
      steps: [{ step: step("Update tunnel “Mac”", "curl -X PUT …"), state: { state: "done" } }],
      changes: [],
    },
  },
];

let verified: string[];

beforeEach(() => {
  verified = [];
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "accounts_list":
        return [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
      case "routes_activity":
        return entries;
      case "routes_overview":
        return {
          tunnel: null,
          routes: [
            {
              hostname: "api.yx.com",
              path: null,
              origin: "http://localhost:8000",
              local: true,
              zone: "yx.com",
              dns: { state: "ok" },
              access: null,
            },
          ],
          zones: [
            { id: "z1", name: "xyz.com" },
            { id: "z2", name: "yx.com" },
          ],
        } satisfies RoutesOverview;
      case "routes_verify":
        verified.push(String(payload["hostname"]));
        return { hostname: payload["hostname"], status: 200, failure: null, message: null };
      default:
        return null;
    }
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>
        <ActivityPage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("ActivityPage", () => {
  it("doesn't present a rolled-back change as made", async () => {
    renderPage();
    expect(await screen.findByText("Attempted Changes")).toBeTruthy();
    expect(screen.getByText("Nothing was changed: every step was undone.")).toBeTruthy();
    expect(screen.getByText("record already exists")).toBeTruthy();
    // The removed route isn't routed now, so there's nothing to check again.
    expect(screen.queryByText("Check Again")).toBeNull();
  });

  it("copies every command and checks routed hostnames again", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.assign(navigator, { clipboard: { writeText } });
    renderPage();
    // Rows select on mouse down, like native lists.
    fireEvent.mouseDown(await screen.findByText("Add api.yx.com → http://localhost:8000"));
    fireEvent.click(await screen.findByRole("button", { name: "Copy All as Commands" }));
    expect(writeText).toHaveBeenCalledWith("# Update tunnel “Mac”\ncurl -X PUT …\n");

    fireEvent.click(await screen.findByRole("button", { name: "Check" }));
    await waitFor(() => expect(verified).toEqual(["api.yx.com"]));
    expect(await screen.findByText("Works (HTTP 200)")).toBeTruthy();
  });
});
