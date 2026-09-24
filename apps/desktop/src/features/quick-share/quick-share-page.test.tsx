import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { Account, CliShare, DomainShare, QuickShare } from "@/lib/ipc/bindings";
import { QuickSharePage } from "./quick-share-page";

let shares: QuickShare[];
let binaryInstalled: boolean;
let accounts: Account[];
let domainShares: DomainShare[];
let terminalShares: CliShare[];
let calls: { cmd: string; args: Record<string, unknown> }[];
/** Holds `domain_shares_stop` until released, like a slow Cloudflare. */
let stopGate: Promise<void> | null;

beforeEach(() => {
  shares = [];
  binaryInstalled = true;
  accounts = [];
  domainShares = [];
  terminalShares = [];
  calls = [];
  stopGate = null;
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "accounts_list":
        return accounts;
      case "domains_list":
        return [
          {
            id: "z1",
            name: "xyz.com",
            status: "active",
            nameServers: [],
            originalNameServers: [],
            plan: null,
            paused: false,
          },
          {
            id: "z2",
            name: "pending.dev",
            status: "pending",
            nameServers: [],
            originalNameServers: [],
            plan: null,
            paused: false,
          },
        ];
      case "domain_shares_list":
        return domainShares;
      case "quick_share_cli_list":
        return terminalShares;
      case "quick_share_cli_stop":
        terminalShares = [];
        return null;
      case "domain_shares_start":
        domainShares = [
          {
            accountId: String(payload["accountId"]),
            hostname: String(payload["hostname"]),
            origin: String(payload["origin"]),
            owner: "app",
            expiresAt: null,
            createdAt: Date.now(),
          },
        ];
        return { type: "applied", tunnelId: "t1", verify: [], connectorError: null };
      case "domain_shares_stop":
        return (stopGate ?? Promise.resolve()).then(() => {
          domainShares = [];
          return null;
        });
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

  it("shares on a subdomain of your own domain, then removes it", async () => {
    accounts = [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
    renderPage();
    fireEvent.change(screen.getByRole("combobox", { name: "Port or address" }), {
      target: { value: "3000" },
    });
    // Only active domains are offered.
    const address = await screen.findByRole("combobox", { name: "Address" });
    fireEvent.click(address);
    expect(screen.queryByRole("option", { name: "On pending.dev" })).toBeNull();
    fireEvent.click(await screen.findByRole("option", { name: "On xyz.com" }));
    fireEvent.change(screen.getByRole("textbox", { name: "Subdomain" }), {
      target: { value: "demo" },
    });
    fireEvent.click(screen.getByRole("button", { name: "Share" }));

    const card = await screen.findByRole("article", { name: "Share at demo.xyz.com" });
    expect(within(card).getByText("https://demo.xyz.com")).toBeTruthy();
    expect(calls.find((c) => c.cmd === "domain_shares_start")?.args).toMatchObject({
      accountId: "acc",
      hostname: "demo.xyz.com",
      origin: "3000",
    });
    expect(calls.some((c) => c.cmd === "quick_share_start")).toBe(false);

    fireEvent.click(within(card).getByRole("button", { name: "Stop Sharing" }));
    await waitFor(() => expect(screen.queryByRole("article")).toBeNull());
  });

  it("keeps a share that's stopping on screen, busy, until it's gone", async () => {
    let release!: () => void;
    stopGate = new Promise((resolve) => {
      release = resolve;
    });
    domainShares = [
      {
        accountId: "acc",
        hostname: "demo.xyz.com",
        origin: "http://localhost:3000",
        owner: "app",
        expiresAt: null,
        createdAt: Date.now(),
      },
    ];
    renderPage();
    const card = await screen.findByRole("article", { name: "Share at demo.xyz.com" });
    fireEvent.click(within(card).getByRole("button", { name: "Stop Sharing" }));

    await waitFor(() => expect(card.getAttribute("aria-busy")).toBe("true"));
    const stop = within(card).getByRole("button", { name: "Stop Sharing" });
    expect(stop.getAttribute("aria-busy")).toBe("true");
    expect(stop.hasAttribute("disabled")).toBe(true);

    release();
    await waitFor(() => expect(screen.queryByRole("article")).toBeNull());
  });

  it("shows shares running in a terminal and can stop them", async () => {
    terminalShares = [
      {
        owner: "4242-100",
        origin: "http://localhost:8080",
        url: "https://x-y-z.trycloudflare.com",
        startedAt: Date.now(),
        stopAt: null,
      },
    ];
    renderPage();
    const card = await screen.findByRole("article", {
      name: "Quick Share of localhost:8080 from a terminal",
    });
    expect(within(card).getByText("https://x-y-z.trycloudflare.com")).toBeTruthy();
    fireEvent.click(within(card).getByRole("button", { name: "Stop Sharing" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "quick_share_cli_stop")?.args["owner"]).toBe("4242-100"),
    );
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
