import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { mockSummary, mockUptimeSummary } from "@/dev/mock-analytics";
import type { AnalyticsRange } from "@/lib/ipc/bindings";
import { AnalyticsPage } from "./analytics-page";

vi.mock("@tanstack/react-router", async (original) => ({
  ...(await original<typeof import("@tanstack/react-router")>()),
  Link: ({ to, search: _, ...props }: ComponentProps<"a"> & { to: string; search?: unknown }) => (
    <a href={to} {...props} />
  ),
}));

const route = (hostname: string, path: string | null = null) => ({
  hostname,
  path,
  origin: "http://localhost:3000",
  local: true,
  zone: "teispace.com",
  dns: { state: "ok" },
  access: null,
  client: null,
  tunnelId: "t",
  temporary: false,
  balanced: false,
  options: {},
});

let edge: "ok" | "denied";
const asked: { cmd: string; range?: AnalyticsRange }[] = [];

beforeEach(() => {
  edge = "ok";
  asked.length = 0;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    asked.push({ cmd, range: payload["range"] as AnalyticsRange });
    switch (cmd) {
      case "accounts_list":
        return [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
      case "routes_overview":
        return {
          tunnel: null,
          tunnels: [],
          routes: [route("app.teispace.com"), route("docs.teispace.com")],
          zones: [],
          networks: [],
        };
      case "analytics_summary":
        if (edge === "denied") {
          throw {
            code: "permissionDenied",
            message: { key: "core.error.analytics.permission", args: {} },
            hint: null,
            field: null,
          };
        }
        return mockSummary(payload["hostnames"] as string[], payload["range"] as AnalyticsRange);
      case "uptime_list":
        return [mockUptimeSummary("app.teispace.com", null)];
      case "accounts_capabilities":
        return {
          zonesRead: "yes",
          tunnelsRead: "yes",
          tunnelsEdit: "yes",
          accessEdit: "yes",
          analytics: "no",
          zones: [],
        };
      default:
        return null;
    }
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <AnalyticsPage />
    </QueryClientProvider>,
  );
}

const routeNames = () =>
  within(screen.getAllByRole("rowgroup")[1] as HTMLElement)
    .getAllByRole("link")
    .map((link) => link.textContent);

describe("AnalyticsPage", () => {
  it("lists every route with traffic, errors, P95 and uptime", async () => {
    renderPage();
    expect(await screen.findByRole("link", { name: "app.teispace.com" })).toBeTruthy();
    const table = screen.getByRole("table", { name: "Routes" });
    expect(within(table).getAllByRole("row")).toHaveLength(3);
    // Uptime from this Mac's checks; the other route isn't checked here.
    expect(await within(table).findByText("99.7%")).toBeTruthy();
    expect(within(table).getByText("184 ms")).toBeTruthy();
    expect(screen.getAllByRole("img", { name: /^Requests to/ })).toHaveLength(2);
  });

  it("sorts by a column and switches the range", async () => {
    renderPage();
    await screen.findByRole("link", { name: "app.teispace.com" });
    // Busiest first by default (the fixture gives the first hostname the most).
    await waitFor(() => expect(routeNames()).toEqual(["app.teispace.com", "docs.teispace.com"]));
    fireEvent.click(screen.getByRole("button", { name: "Sort by Requests" }));
    await waitFor(() => expect(routeNames()).toEqual(["docs.teispace.com", "app.teispace.com"]));
    expect(screen.getAllByRole("columnheader")[1]?.getAttribute("aria-sort")).toBe("ascending");

    fireEvent.click(screen.getByRole("radio", { name: "Week" }));
    await waitFor(() =>
      expect(asked.some((a) => a.cmd === "analytics_summary" && a.range === "week")).toBe(true),
    );
  });

  it("asks for the analytics permission in place when the token lacks it", async () => {
    edge = "denied";
    renderPage();
    expect(await screen.findByRole("region", { name: /needs 2 more permissions/ })).toBeTruthy();
    expect(screen.getByText("Zone · Analytics · Read")).toBeTruthy();
    // Uptime still shows without it.
    expect(await screen.findByText("99.7%")).toBeTruthy();
  });
});
