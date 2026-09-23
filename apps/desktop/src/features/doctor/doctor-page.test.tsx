import { QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";
import { createQueryClient } from "@/app/query-client";
import { useUiStore } from "@/app/ui-store";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { Issue } from "@/lib/ipc/bindings";
import { DoctorBadge } from "./doctor-badge";
import { DoctorPage } from "./doctor-page";

const issues: Issue[] = [
  {
    id: "dns.missing:acc:app.xyz.com",
    check: "dns.missing",
    severity: "error",
    accountId: "acc",
    subject: "app.xyz.com",
    title: "app.xyz.com has no DNS record",
    detail: "Nothing points it at the tunnel.",
    evidence: [],
    fixes: [
      {
        type: "change",
        label: "Fix the DNS Record",
        change: {
          type: "addRoute",
          route: { hostname: "app.xyz.com", path: null, origin: "http://localhost:3000" },
        },
      },
    ],
  },
  {
    id: "zone.pending:acc:yx.com",
    check: "zone.pending",
    severity: "warning",
    accountId: "acc",
    subject: "yx.com",
    title: "yx.com is waiting for its nameservers",
    detail: "Routes won't work yet.",
    evidence: ["Nameserver ada.ns.cloudflare.com"],
    fixes: [],
  },
];

let previewed: unknown[];
let ignored: string[];

const settings = () => ({
  theme: "system",
  showInMenuBar: true,
  notifyConnectors: true,
  notifyQuickShares: true,
  notifyDoctor: true,
  ignoredIssues: ignored,
});

beforeEach(() => {
  previewed = [];
  ignored = [];
  useUiStore.setState({ legacyIgnoredIssues: [] });
  mockWindows("main");
  mockIPC((cmd, args) => {
    const payload = (args ?? {}) as Record<string, unknown>;
    switch (cmd) {
      case "doctor_run":
        return issues;
      case "settings_get":
        return settings();
      case "doctor_set_ignored": {
        const ids = payload["ids"] as string[];
        ignored = payload["ignored"]
          ? [...new Set([...ignored, ...ids])]
          : ignored.filter((id) => !ids.includes(id));
        return settings();
      }
      case "routes_preview":
        previewed.push(payload["change"]);
        return { steps: [], warnings: [], requiresConfirmation: false, fingerprint: "fp" };
      default:
        return null;
    }
  });
});

function renderPage() {
  return render(
    <QueryClientProvider client={createQueryClient()}>
      <TooltipProvider>
        <DoctorBadge />
        <DoctorPage />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("DoctorPage", () => {
  it("lists issues worst first and counts errors in the badge", async () => {
    renderPage();
    expect(await screen.findByRole("option", { name: /has no DNS record/ })).toBeTruthy();
    expect(screen.getByRole("option", { name: /waiting for its nameservers/ })).toBeTruthy();
    expect(screen.getByRole("status", { name: "1 problem" })).toBeTruthy();
  });

  it("reviews a fix as a plan", async () => {
    renderPage();
    fireEvent.click(await screen.findByRole("button", { name: "Fix the DNS Record…" }));
    await waitFor(() => expect(previewed).toHaveLength(1));
    expect(previewed[0]).toMatchObject({ type: "addRoute" });
    expect(await screen.findByRole("dialog", { name: "Fix the DNS Record" })).toBeTruthy();
  });

  it("hides ignored issues and can bring them back", async () => {
    renderPage();
    await screen.findByRole("option", { name: /has no DNS record/ });
    fireEvent.click(screen.getByRole("button", { name: "Ignore" }));
    await waitFor(() =>
      expect(screen.queryByRole("option", { name: /has no DNS record/ })).toBeNull(),
    );
    expect(screen.queryByRole("status", { name: "1 problem" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Ignore" }));
    expect(await screen.findByText("Everything looks good")).toBeTruthy();
    // Ignores live in the settings, where background Doctor runs see them too.
    expect(ignored).toHaveLength(2);
    fireEvent.click(screen.getByRole("button", { name: "Show Ignored Issues" }));
    expect(await screen.findByRole("option", { name: /has no DNS record/ })).toBeTruthy();
    expect(ignored).toEqual([]);
  });

  it("moves ignores kept in this window before they moved to settings", async () => {
    useUiStore.setState({ legacyIgnoredIssues: ["zone.pending:acc:yx.com"] });
    renderPage();
    await waitFor(() => expect(ignored).toEqual(["zone.pending:acc:yx.com"]));
    await waitFor(() => expect(useUiStore.getState().legacyIgnoredIssues).toEqual([]));
    expect(screen.queryByRole("option", { name: /waiting for its nameservers/ })).toBeNull();
  });
});
