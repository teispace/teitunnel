import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { mockRouteStats, mockUptimeDetail } from "@/dev/mock-analytics";
import type { AnalyticsRange, RouteView } from "@/lib/ipc/bindings";
import { RouteAnalytics } from "./components/route-analytics";

const route = {
  hostname: "app.teispace.com",
  path: null,
  origin: "http://localhost:5173",
  local: true,
  zone: "teispace.com",
  dns: { state: "ok" },
  access: null,
  client: null,
  tunnelId: "t",
  temporary: false,
  balanced: false,
  paused: false,
  options: {},
} as unknown as RouteView;

let failures: number;
let checked: boolean;

beforeEach(() => {
  failures = 0;
  checked = true;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    const range = payload["range"] as AnalyticsRange;
    switch (cmd) {
      case "analytics_route":
        if (failures > 0) {
          failures--;
          throw {
            code: "unavailable",
            message: { key: "core.error.analytics.rateLimited", args: {} },
            hint: null,
            field: null,
          };
        }
        return mockRouteStats(String(payload["hostname"]), null, range);
      case "uptime_route": {
        const detail = mockUptimeDetail(String(payload["hostname"]), null, range);
        return checked ? detail : { ...detail, summary: { ...detail.summary, lastChecked: null } };
      }
      case "accounts_list":
        return [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
      default:
        return null;
    }
  });
});

function renderSection() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <RouteAnalytics accountId="acc" route={route} />
    </QueryClientProvider>,
  );
}

describe("RouteAnalytics", () => {
  it("shows uptime, its strip, incidents, traffic and the top lists", async () => {
    renderSection();
    expect(await screen.findByText("99.7% over the last 24 hours")).toBeTruthy();
    expect(
      screen.getByRole("img", { name: "Uptime over the last 24 hours, 90 slices" }),
    ).toBeTruthy();
    expect(screen.getByText("Service unreachable")).toBeTruthy();
    expect(
      await screen.findByRole("img", { name: "Requests over the last 24 hours" }),
    ).toBeTruthy();
    expect(screen.getByText("/api/session")).toBeTruthy();
    expect(screen.getByText("People and other clients")).toBeTruthy();
    expect(screen.getByText("P50 42 ms · P95 184 ms · P99 402 ms")).toBeTruthy();
  });

  it("offers Try Again when Cloudflare can't answer, busy until it has", async () => {
    failures = 1;
    renderSection();
    const button = await screen.findByRole("button", { name: "Try Again" });
    expect(
      screen.getByText(
        "Cloudflare's analytics limit is used up for a few minutes. Try again shortly.",
      ),
    ).toBeTruthy();
    fireEvent.click(button);
    await waitFor(() => expect(screen.queryByRole("button", { name: "Try Again" })).toBeNull());
    expect(await screen.findByText("/api/session")).toBeTruthy();
  });

  it("says when a route hasn't been checked yet", async () => {
    checked = false;
    renderSection();
    expect(await screen.findByText(/Not checked yet/)).toBeTruthy();
  });
});
