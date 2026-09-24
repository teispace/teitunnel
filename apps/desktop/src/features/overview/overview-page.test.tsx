import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import type { ComponentProps } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createQueryClient } from "@/app/query-client";
import type { Account } from "@/lib/ipc/bindings";
import { OverviewPage } from "./overview-page";

vi.mock("@tanstack/react-router", async (original) => ({
  ...(await original<typeof import("@tanstack/react-router")>()),
  Link: ({ to, search: _, ...props }: ComponentProps<"a"> & { to: string; search?: unknown }) => (
    <a href={to} {...props} />
  ),
}));

let accounts: Account[];
/** Holds `routes_overview` until released, like a slow Cloudflare. */
let overviewGate: Promise<void>;
let release: () => void;
let overviewFails: boolean;

beforeEach(() => {
  accounts = [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
  overviewGate = new Promise((resolve) => {
    release = resolve;
  });
  overviewFails = false;
  mockWindows("main");
  mockIPC((cmd) => {
    switch (cmd) {
      case "accounts_list":
        return accounts;
      case "quick_share_list":
        return [];
      case "binary_status":
        return {
          path: "/usr/bin/cloudflared",
          source: "system",
          version: "2026.9.1",
          supported: true,
        };
      case "routes_overview":
        return overviewGate.then(() => {
          if (overviewFails) {
            throw {
              code: "network",
              message: { key: "core.error.cloudflare.network", args: { detail: "timed out" } },
              hint: null,
              field: null,
            };
          }
          return { tunnel: null, tunnels: [], routes: [], zones: [], networks: [] };
        });
      case "settings_get":
        return {
          theme: "system",
          showInMenuBar: true,
          notifyConnectors: true,
          notifyQuickShares: true,
          notifyDoctor: true,
          checkForUpdates: true,
          cliOfferDismissed: true,
          ignoredIssues: [],
        };
      case "doctor_run":
        return [];
      default:
        return null;
    }
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <OverviewPage />
    </QueryClientProvider>,
  );
}

describe("OverviewPage", () => {
  it("doesn't say nothing is running before it knows", async () => {
    renderPage();
    expect(await screen.findByLabelText("Loading what's running")).toBeTruthy();
    expect(screen.queryByText("Nothing running yet")).toBeNull();

    release();
    expect(await screen.findByText("Nothing running yet")).toBeTruthy();
  });

  it("says the routes couldn't load instead of claiming there are none", async () => {
    overviewFails = true;
    release();
    renderPage();
    expect((await screen.findByRole("alert")).textContent).toContain("Couldn't load your routes.");
    expect(screen.queryByText("Nothing running yet")).toBeNull();

    overviewFails = false;
    fireEvent.click(screen.getByRole("button", { name: "Try Again" }));
    await waitFor(() => expect(screen.queryByRole("alert")).toBeNull());
    expect(screen.getByText("Nothing running yet")).toBeTruthy();
  });
});
