import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { QuickShare } from "@/lib/ipc/bindings";
import { QuickSharePage } from "./quick-share-page";

let shares: QuickShare[];
let binaryInstalled: boolean;

beforeEach(() => {
  shares = [];
  binaryInstalled = true;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "binary_status":
        return binaryInstalled
          ? {
              path: "/opt/homebrew/bin/cloudflared",
              source: "system",
              version: "2026.9.1",
              supported: true,
            }
          : null;
      case "binary_install":
        binaryInstalled = true;
        return {
          path: "/Users/me/Library/Application Support/com.teispace.teitunnel/bin/cloudflared",
          source: "managed",
          version: "2026.9.1",
          supported: true,
        };
      case "quick_share_list":
        return shares;
      case "services_list":
        return [];
      case "quick_share_start": {
        if (payload["origin"] === "99999") {
          throw {
            code: "invalidInput",
            message: { key: "core.error.origin.invalidPort", args: {} },
            hint: null,
            field: "origin",
          };
        }
        const share: QuickShare = {
          id: "qs-a",
          origin: `http://localhost:${String(payload["origin"])}`,
          url: "https://a-b-c.trycloudflare.com",
          status: { status: "live" },
          startedAt: Date.now(),
          stopAt: null,
        };
        shares = [share];
        return share;
      }
      case "quick_share_stop":
        shares = [];
        return null;
      case "quick_share_stats":
        return { requests: 5, errors: 0 };
      default:
        return null;
    }
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>
        <QuickSharePage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("QuickSharePage", () => {
  it("starts a share, shows its URL and stops it", async () => {
    renderPage();
    const field = screen.getByRole("combobox", { name: "Port or address" });
    fireEvent.change(field, { target: { value: "3000" } });
    fireEvent.click(screen.getByRole("button", { name: "Share" }));

    const card = await screen.findByRole("article", { name: "Quick Share of localhost:3000" });
    expect(within(card).getByText("https://a-b-c.trycloudflare.com")).toBeTruthy();
    expect(await within(card).findByText("5 requests")).toBeTruthy();

    fireEvent.click(within(card).getByRole("button", { name: "Stop Sharing" }));
    await waitFor(() => expect(screen.queryByRole("article")).toBeNull());
  });

  it("shows validation errors next to the field", async () => {
    renderPage();
    const field = screen.getByRole("combobox", { name: "Port or address" });
    fireEvent.change(field, { target: { value: "99999" } });
    fireEvent.click(screen.getByRole("button", { name: "Share" }));
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toContain("between 1 and 65535");
    expect(field.getAttribute("aria-invalid")).toBe("true");
  });

  it("installs cloudflared when it's missing", async () => {
    binaryInstalled = false;
    renderPage();
    expect(await screen.findByText("cloudflared isn't installed")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Share" }).hasAttribute("disabled")).toBe(true);

    fireEvent.click(screen.getByRole("button", { name: "Install cloudflared" }));
    await waitFor(() => expect(screen.queryByText("cloudflared isn't installed")).toBeNull());
    expect(screen.getByRole("button", { name: "Share" }).hasAttribute("disabled")).toBe(false);
  });
});
