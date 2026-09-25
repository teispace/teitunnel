import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ShareComposer } from "@/features/quick-share/components/share-composer";
import type { ExposureReport } from "@/lib/ipc/bindings";

const core = (key: string) => ({ key: `core.${key}`, args: {} });

const leaky: ExposureReport = {
  origin: "http://localhost:3000",
  findings: [
    {
      kind: "envFile",
      severity: "high",
      path: "/.env",
      title: core("exposure.envFile.title"),
      advice: core("exposure.envFile.advice"),
      detail: "APP_KEY, DB_PASSWORD",
    },
    {
      kind: "sourceMap",
      severity: "low",
      path: "/assets/index.js.map",
      title: core("exposure.sourceMap.title"),
      advice: core("exposure.sourceMap.advice"),
      detail: "2",
    },
  ],
  requests: 21,
  incomplete: false,
  elapsedMs: 180,
};

let report: ExposureReport | null;
let calls: string[];

beforeEach(() => {
  report = leaky;
  calls = [];
  mockWindows("main");
  mockIPC((cmd) => {
    calls.push(cmd);
    switch (cmd) {
      case "accounts_list":
        return [];
      case "services_list":
        return [];
      case "exposure_check":
        return report;
      case "quick_share_start":
        return {
          id: "q1",
          origin: "http://localhost:3000",
          url: null,
          status: { state: "starting" },
          startedAt: Date.now(),
          stopAt: null,
          hostHeader: null,
          check: null,
        };
      default:
        return null;
    }
  });
});

function share() {
  render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>
        <ShareComposer />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  const field = screen.getByLabelText("Port or address");
  fireEvent.change(field, { target: { value: "3000" } });
  fireEvent.click(screen.getByRole("button", { name: "Share" }));
}

describe("the exposure check before sharing", () => {
  it("shows what it found and waits for Share Anyway", async () => {
    share();
    const alert = await screen.findByRole("alert", {
      name: "http://localhost:3000 may expose more than you mean to share",
    });
    expect(alert.textContent).toContain("Its .env file can be downloaded");
    expect(alert.textContent).toContain("APP_KEY, DB_PASSWORD");
    expect(alert.textContent).toContain("at /.env");
    expect(calls).not.toContain("quick_share_start");
    fireEvent.click(screen.getByRole("button", { name: "Share Anyway" }));
    await waitFor(() => expect(calls).toContain("quick_share_start"));
  });

  it("can be cancelled", async () => {
    share();
    await screen.findByRole("button", { name: "Share Anyway" });
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("button", { name: "Share Anyway" })).toBeNull();
    expect(calls).not.toContain("quick_share_start");
  });

  it("shares at once when nothing is found or the check is off", async () => {
    report = null;
    share();
    await waitFor(() => expect(calls).toContain("quick_share_start"));
    expect(screen.queryByRole("button", { name: "Share Anyway" })).toBeNull();
  });
});
