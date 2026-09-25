import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { DomainShare, QuickShare, RouteSchedule } from "@/lib/ipc/bindings";
import { QuickSharePage } from "./quick-share-page";

let domainShares: DomainShare[];
let schedules: RouteSchedule[];
let shares: QuickShare[];
let calls: { cmd: string; args: Record<string, unknown> }[];

const demo = (patch: Partial<DomainShare> = {}): DomainShare => ({
  accountId: "acc",
  hostname: "demo.xyz.com",
  origin: "http://127.0.0.1:52790",
  owner: "app",
  expiresAt: null,
  createdAt: Date.now(),
  source: "http://localhost:3000",
  folder: false,
  paused: false,
  schedule: null,
  ...patch,
});

beforeEach(() => {
  domainShares = [];
  schedules = [];
  shares = [];
  calls = [];
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: payload });
    switch (cmd) {
      case "accounts_list":
        return [{ id: "acc", name: "Me", credential: "apiToken", limitedZone: null }];
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
        ];
      case "binary_status":
        return { path: "/bin/cloudflared", source: "system", version: "2026.9.1", supported: true };
      case "domain_shares_list":
        return domainShares;
      case "quick_share_list":
        return shares;
      case "sharing_schedules":
        return schedules;
      case "sharing_set_paused":
        domainShares = domainShares.map((s) => ({ ...s, paused: Boolean(payload["paused"]) }));
        return null;
      case "sharing_set_schedule":
        schedules = payload["schedule"]
          ? [
              {
                accountId: "acc",
                hostname: "demo.xyz.com",
                schedule: payload["schedule"] as RouteSchedule["schedule"],
                on: true,
                nextChange: Date.now() + 3_600_000,
              },
            ]
          : [];
        return null;
      case "services_list":
        return [
          {
            port: 5173,
            allInterfaces: false,
            pid: 1,
            process: "node",
            kind: "vite",
            project: "shop",
            folder: "/Users/me/shop",
            origin: "http://localhost:5173",
          },
        ];
      case "sharing_name_suggestions":
        return [
          { template: "{project}.xyz.com", hostname: "shop.xyz.com", remembered: false },
          {
            template: "{branch}-{project}.xyz.com",
            hostname: "login-fix-shop.xyz.com",
            remembered: false,
          },
        ];
      case "sharing_expand_name":
        return String(payload["hostname"])
          .replace("{branch}", "login-fix")
          .replace("{project}", "shop");
      case "sharing_choose_folder":
        return "/Users/me/site/dist";
      case "sharing_folder":
        return { path: payload["path"], listing: payload["listing"], spa: payload["spa"] };
      case "quick_share_start_folder": {
        const share: QuickShare = {
          id: "qs-f",
          origin: "http://127.0.0.1:52811",
          url: "https://folder-share.trycloudflare.com",
          status: { status: "live" },
          startedAt: Date.now(),
          stopAt: null,
          inspected: true,
          folder: payload["folder"] as QuickShare["folder"],
          hostHeader: null,
          check: null,
        };
        shares = [share];
        return share;
      }
      case "domain_shares_start":
        return { type: "applied", tunnelId: "t1", verify: [], connectorError: null };
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

describe("sharing extras", () => {
  it("pauses and resumes a share on your domain at the same address", async () => {
    domainShares = [demo()];
    renderPage();
    const card = await screen.findByRole("article", { name: "Share at demo.xyz.com" });
    expect(within(card).getByText("localhost:3000")).toBeTruthy();
    fireEvent.click(within(card).getByRole("button", { name: "Pause" }));
    expect(await within(card).findByText("Paused")).toBeTruthy();
    expect(calls.find((c) => c.cmd === "sharing_set_paused")?.args).toMatchObject({
      accountId: "acc",
      hostname: "demo.xyz.com",
      paused: true,
    });
    fireEvent.click(within(card).getByRole("button", { name: "Resume" }));
    await waitFor(() => expect(within(card).queryByText("Paused")).toBeNull());
    expect(within(card).getByText("https://demo.xyz.com")).toBeTruthy();
  });

  it("puts a share on a schedule", async () => {
    domainShares = [demo()];
    renderPage();
    const card = await screen.findByRole("article", { name: "Share at demo.xyz.com" });
    fireEvent.click(within(card).getByRole("button", { name: "Schedule" }));
    // Office hours to start with; Saturday added.
    fireEvent.click(await screen.findByRole("button", { name: "Saturday" }));
    fireEvent.change(screen.getByLabelText("From"), { target: { value: "08:30" } });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "sharing_set_schedule")?.args["schedule"]).toEqual({
        days: ["mon", "tue", "wed", "thu", "fri", "sat"],
        from: "08:30",
        to: "18:00",
        timeZone: null,
      }),
    );
    expect(await within(card).findByText(/Mon.Sat/)).toBeTruthy();
  });

  it("shares a chosen folder", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Share a Folder…" }));
    expect(await screen.findByText("dist")).toBeTruthy();
    // The folder's options replace the Host header ones.
    fireEvent.click(screen.getByRole("button", { name: "Advanced" }));
    expect(await screen.findByText(/never shared/)).toBeTruthy();
    fireEvent.click(screen.getByLabelText(/Single-page app/));
    fireEvent.click(screen.getByRole("button", { name: "Share" }));
    const card = await screen.findByRole("article", {
      name: "Quick Share of /Users/me/site/dist",
    });
    expect(within(card).getByText("https://folder-share.trycloudflare.com")).toBeTruthy();
    expect(calls.find((c) => c.cmd === "quick_share_start_folder")?.args["folder"]).toEqual({
      path: "/Users/me/site/dist",
      listing: false,
      spa: true,
    });
  });

  it("suggests names from the project and shows what a template becomes", async () => {
    renderPage();
    fireEvent.change(screen.getByRole("combobox", { name: "Port or address" }), {
      target: { value: "5173" },
    });
    fireEvent.click(await screen.findByRole("combobox", { name: "Address" }));
    fireEvent.click(await screen.findByRole("option", { name: "On xyz.com" }));
    fireEvent.click(await screen.findByRole("button", { name: "login-fix-shop" }));
    const subdomain = screen.getByRole("textbox", { name: "Subdomain" }) as HTMLInputElement;
    expect(subdomain.value).toBe("{branch}-{project}");
    expect(await screen.findByText("It's login-fix-shop.xyz.com here.")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Share" }));
    await waitFor(() =>
      expect(calls.find((c) => c.cmd === "domain_shares_start")?.args).toMatchObject({
        hostname: "{branch}-{project}.xyz.com",
        folder: "/Users/me/shop",
      }),
    );
  });
});
