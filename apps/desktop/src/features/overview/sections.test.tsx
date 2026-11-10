import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { act, render, screen, within } from "@testing-library/react";
import type { ComponentProps, ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { mockUptimeSummary } from "@/dev/mock-analytics";
import { mockRows } from "@/dev/mock-inspector";
import { RecentRequests } from "@/features/inspector";
import type { LiveBatch, Traffic } from "@/lib/ipc/bindings";
import { emptySeries } from "@/lib/traffic";
import { OverviewStats, ResponsesCard, StatusBanner } from "./components/sections";

vi.mock("@tanstack/react-router", async (original) => ({
  ...(await original<typeof import("@tanstack/react-router")>()),
  Link: ({
    to,
    search,
    ...props
  }: ComponentProps<"a"> & { to: string; search?: Record<string, string> }) => (
    <a href={search ? `${to}?${new URLSearchParams(search)}` : to} {...props} />
  ),
}));

let channel: { onmessage: (batch: LiveBatch) => void } | null;
let limits: unknown[];

function trafficOf(samples: { requests: number; serverErrors: number }[]): Traffic {
  const series = emptySeries();
  const now = Date.now();
  samples.forEach((sample, i) => {
    series.at.push(now - (samples.length - i) * 1000);
    series.span.push(1);
    series.requests.push(sample.requests);
    series.serverErrors.push(sample.serverErrors);
    series.errors.push(0);
  });
  return { series, totalRequests: 1234, totalErrors: 3, connections: 4, rttMs: 18, locations: [] };
}

beforeEach(() => {
  channel = null;
  limits = [];
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "inspect_exchanges": {
        const query = payload["query"] as { limit: number };
        limits.push(query.limit);
        return { items: mockRows(20).slice(0, query.limit), next: "older" };
      }
      case "inspect_subscribe":
        channel = payload["onBatch"] as typeof channel;
        return 1;
      default:
        return null;
    }
  });
});

const wrap = (children: ReactNode) =>
  render(<QueryClientProvider client={createQueryClient()}>{children}</QueryClientProvider>);

describe("Overview dashboard", () => {
  it("shows traffic, the share of failed requests and which routes are down", async () => {
    const live = trafficOf([
      { requests: 40, serverErrors: 0 },
      { requests: 60, serverErrors: 10 },
    ]);
    const list = [
      { ...mockUptimeSummary("docs.xyz.com", null), accountId: "acc", up: true, uptimeDay: 0.99 },
      { ...mockUptimeSummary("api.xyz.com", null), accountId: "acc", up: false },
    ];
    wrap(
      <OverviewStats
        traffic={live}
        uptimes={list}
        accountId="acc"
        routes={[]}
        tunnel={null}
        issues={[]}
        shares={[]}
      />,
    );
    const errors = (await screen.findByText("Errors")).closest("a") as HTMLElement;
    expect(within(errors).getByText("10%").className).toContain("text-error");
    expect(within(errors).getByText("10 of 100 requests in 5 min")).toBeTruthy();
    expect(screen.getByText("1 down")).toBeTruthy();
    expect(screen.getByText("api.xyz.com")).toBeTruthy();
    expect(screen.getByText("1,234 since start")).toBeTruthy();
    expect(screen.getByText("18")).toBeTruthy();
  });

  it("says what isn't known yet instead of guessing", () => {
    wrap(
      <OverviewStats
        traffic={undefined}
        uptimes={[]}
        accountId="acc"
        routes={[]}
        tunnel={null}
        issues={[]}
        shares={[]}
      />,
    );
    expect(screen.getByText("Checked once a route is live")).toBeTruthy();
    expect(screen.getByText("No routes in this account yet.")).toBeTruthy();
    expect(screen.getByText("No requests in 5 min")).toBeTruthy();
  });

  it("breaks responses down by class", () => {
    const live = trafficOf([{ requests: 100, serverErrors: 25 }]);
    live.series.ok.push(75);
    live.series.redirects.push(0);
    live.series.clientErrors.push(0);
    wrap(<ResponsesCard traffic={live} />);
    expect(
      screen.getByRole("img", { name: "Responses: Success (2xx) 75%, Server errors (5xx) 25%" }),
    ).toBeTruthy();
    expect(screen.getByText("1,234")).toBeTruthy();
  });

  it("leads with problems, and says when all is well", () => {
    const { unmount } = wrap(
      <StatusBanner
        problems={2}
        warnings={1}
        connections={4}
        locations={["ams01"]}
        account={null}
      />,
    );
    expect(screen.getByText("2 problems need your attention")).toBeTruthy();
    expect(screen.getByText("AMS01")).toBeTruthy();
    unmount();
    wrap(<StatusBanner problems={0} warnings={0} connections={0} locations={[]} account="Work" />);
    expect(screen.getByText("Everything is running")).toBeTruthy();
    expect(screen.getByText("Connector offline")).toBeTruthy();
  });

  it("keeps the newest requests live, each opening in the Inspector", async () => {
    wrap(<RecentRequests limit={6}>{(rows) => <ul>{rows}</ul>}</RecentRequests>);
    const links = await screen.findAllByRole("link");
    expect(links).toHaveLength(6);
    expect(limits).toEqual([6]);
    expect(links[0]?.getAttribute("href")).toBe("/inspector?exchange=ex-20");

    const [fresh] = mockRows(21);
    if (!fresh) throw new Error("no row");
    act(() =>
      channel?.onmessage({ exchanges: [fresh], cleared: [], tapsChanged: false, lagged: false }),
    );
    const updated = screen.getAllByRole("link");
    expect(updated).toHaveLength(6);
    expect(updated[0]?.getAttribute("href")).toBe("/inspector?exchange=ex-21");
  });
});
